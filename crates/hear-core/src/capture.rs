use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime};

const PACKET_FRAMES: usize = 1024;
const MAX_BUFFER_BYTES: usize = 8 * 1024 * 1024;

/// Fixed counters only: callbacks never allocate or acquire a lock for metrics.
pub struct Metrics {
    origin: Instant,
    queued_ns: AtomicU64,
    high_water_ns: AtomicU64,
    callback_ns: AtomicU64,
    consumed_ns: AtomicU64,
    durable_ns: AtomicU64,
    callback_frames: AtomicU64,
    dropped_frames: AtomicU64,
    seal_ms: [AtomicU64; 8],
    callback_us: [AtomicU64; 8],
    callback_max_ns: AtomicU64,
    callback_deadline_misses: AtomicU64,
    input_rate: AtomicU64,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct MetricsView {
    pub callback_frames: u64,
    pub dropped_frames: u64,
    pub queued_ms: u64,
    pub ring_high_water_ms: u64,
    pub callback_age_ms: Option<u64>,
    pub consumed_age_ms: Option<u64>,
    pub durable_age_ms: Option<u64>,
    /// <=1/5/10/50/100/500/1000 ms, then overflow; lifetime counts.
    pub seal_histogram: [u64; 8],
    pub callback_histogram_us: [u64; 8],
    pub callback_max_ns: u64,
    pub callback_deadline_misses: u64,
    pub input_rate: u64,
}
impl Metrics {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            queued_ns: AtomicU64::new(0),
            high_water_ns: AtomicU64::new(0),
            callback_ns: AtomicU64::new(0),
            consumed_ns: AtomicU64::new(0),
            durable_ns: AtomicU64::new(0),
            callback_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            seal_ms: std::array::from_fn(|_| AtomicU64::new(0)),
            callback_us: std::array::from_fn(|_| AtomicU64::new(0)),
            callback_max_ns: AtomicU64::new(0),
            callback_deadline_misses: AtomicU64::new(0),
            input_rate: AtomicU64::new(0),
        }
    }
    fn now(&self) -> u64 {
        self.origin.elapsed().as_nanos() as u64 + 1
    }
    pub fn sealed(&self, elapsed: Duration) {
        self.durable_ns.store(self.now(), Ordering::Relaxed);
        let bucket = [1, 5, 10, 50, 100, 500, 1000]
            .iter()
            .position(|v| elapsed.as_millis() <= *v)
            .unwrap_or(7);
        self.seal_ms[bucket].fetch_add(1, Ordering::Relaxed);
    }
    pub fn view(&self) -> MetricsView {
        let now = self.now();
        let age = |v: &AtomicU64| {
            let value = v.load(Ordering::Relaxed);
            (value != 0).then(|| now.saturating_sub(value) / 1_000_000)
        };
        MetricsView {
            callback_frames: self.callback_frames.load(Ordering::Relaxed),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
            queued_ms: self.queued_ns.load(Ordering::Relaxed) / 1_000_000,
            ring_high_water_ms: self.high_water_ns.load(Ordering::Relaxed) / 1_000_000,
            callback_age_ms: age(&self.callback_ns),
            consumed_age_ms: age(&self.consumed_ns),
            durable_age_ms: age(&self.durable_ns),
            seal_histogram: std::array::from_fn(|i| self.seal_ms[i].load(Ordering::Relaxed)),
            callback_histogram_us: std::array::from_fn(|i| {
                self.callback_us[i].load(Ordering::Relaxed)
            }),
            callback_max_ns: self.callback_max_ns.load(Ordering::Relaxed),
            callback_deadline_misses: self.callback_deadline_misses.load(Ordering::Relaxed),
            input_rate: self.input_rate.load(Ordering::Relaxed),
        }
    }
}

/// Arrival time checks wall-clock discontinuity; start_at maps actual samples.
pub struct AudioPacket {
    pub at: Instant,
    pub wall_at: SystemTime,
    pub start_at: Option<Instant>,
    pub sample_start_ns: Option<i64>,
    pub callback_end_ns: Option<i64>,
    pub samples: Vec<f32>,
    pub rate: u32,
    pub channels: usize,
}

/// Each endpoint has one owner. Native callback serialization is required;
/// neither endpoint is Sync. Buffers return through a second SPSC ring.
pub struct AudioSender {
    ready: Producer<AudioPacket>,
    free: Consumer<AudioPacket>,
    spare: Option<AudioPacket>,
    packet_samples: usize,
    metrics: Arc<Metrics>,
}

pub struct AudioReceiver {
    ready: Consumer<AudioPacket>,
    free: Producer<AudioPacket>,
    metrics: Arc<Metrics>,
}

pub fn channel(rate: u32, channels: usize) -> Result<(AudioSender, AudioReceiver), String> {
    if rate == 0 || channels == 0 || channels > 256 {
        return Err("invalid audio source format".into());
    }
    let packet_samples = PACKET_FRAMES * channels;
    // Two seconds at the configured format, capped in bytes even for unusual
    // high-rate multichannel devices. Actual occupancy never allocates more.
    let count = (rate as usize * 2 / PACKET_FRAMES)
        .min(MAX_BUFFER_BYTES / (packet_samples * size_of::<f32>()))
        .max(2);
    let (ready, ready_rx) = RingBuffer::new(count);
    let (mut free_tx, free) = RingBuffer::new(count);
    for _ in 0..count {
        let packet = AudioPacket {
            at: Instant::now(),
            wall_at: SystemTime::now(),
            start_at: None,
            sample_start_ns: None,
            callback_end_ns: None,
            samples: Vec::with_capacity(packet_samples),
            rate,
            channels,
        };
        // The rings are not shared until all buffers are preallocated.
        if free_tx.push(packet).is_err() {
            unreachable!();
        }
    }
    let metrics = Arc::new(Metrics::new());
    Ok((
        AudioSender {
            ready,
            free,
            spare: None,
            packet_samples,
            metrics: Arc::clone(&metrics),
        },
        AudioReceiver {
            ready: ready_rx,
            free: free_tx,
            metrics,
        },
    ))
}

impl AudioSender {
    /// No allocation, locks, waiting, logging, or filesystem work in callbacks.
    /// A full queue rejects this callback atomically, never overwrites audio.
    pub fn enqueue<T: Copy>(
        &mut self,
        data: &[T],
        rate: u32,
        channels: usize,
        captured_ns: Option<i64>,
        convert: impl Fn(T) -> f32,
    ) -> bool {
        let began = Instant::now();
        self.metrics
            .input_rate
            .store(rate as u64, Ordering::Relaxed);
        self.metrics
            .callback_ns
            .store(self.metrics.now(), Ordering::Relaxed);
        let frames = if channels == 0 {
            0
        } else {
            (data.len() / channels) as u64
        };
        self.metrics
            .callback_frames
            .fetch_add(frames, Ordering::Relaxed);
        let success = self.enqueue_inner(data, rate, channels, captured_ns, convert);
        if !success {
            self.metrics
                .dropped_frames
                .fetch_add(frames, Ordering::Relaxed);
        }
        let elapsed = began.elapsed();
        let ns = elapsed.as_nanos() as u64;
        self.metrics
            .callback_max_ns
            .fetch_max(ns, Ordering::Relaxed);
        let bucket = [10, 50, 100, 250, 500, 1000, 5000]
            .iter()
            .position(|v| elapsed.as_micros() <= *v)
            .unwrap_or(7);
        self.metrics.callback_us[bucket].fetch_add(1, Ordering::Relaxed);
        if rate > 0 && frames > 0 && ns > frames * 1_000_000_000 / rate as u64 {
            self.metrics
                .callback_deadline_misses
                .fetch_add(1, Ordering::Relaxed);
        }
        success
    }
    fn enqueue_inner<T: Copy>(
        &mut self,
        data: &[T],
        rate: u32,
        channels: usize,
        captured_ns: Option<i64>,
        convert: impl Fn(T) -> f32,
    ) -> bool {
        if rate == 0
            || channels == 0
            || data.is_empty()
            || data.len() % channels != 0
            || channels > self.packet_samples
            || self.ready.is_abandoned()
        {
            return false;
        }
        let step = self.packet_samples / channels * channels;
        let packets = data.len().div_ceil(step);
        if self.ready.slots() < packets
            || self.free.slots() + usize::from(self.spare.is_some()) < packets
        {
            return false;
        }
        let duration_ns = (data.len() / channels) as u64 * 1_000_000_000 / rate as u64;
        if self
            .metrics
            .queued_ns
            .load(Ordering::Acquire)
            .saturating_add(duration_ns)
            > 2_000_000_000
        {
            return false;
        }
        let at = Instant::now();
        let wall_at = SystemTime::now();
        let duration = Duration::from_secs_f64((data.len() / channels) as f64 / rate as f64);
        let start = at.checked_sub(duration);
        for (index, chunk) in data.chunks(step).enumerate() {
            let Some(mut packet) = self.spare.take().or_else(|| self.free.pop().ok()) else {
                return false;
            };
            packet.samples.clear();
            packet.samples.extend(chunk.iter().copied().map(&convert));
            packet.at = at;
            packet.wall_at = wall_at;
            packet.start_at = start.and_then(|start| {
                start.checked_add(Duration::from_secs_f64(
                    (index * step / channels) as f64 / rate as f64,
                ))
            });
            packet.sample_start_ns = captured_ns.and_then(|ns| {
                ns.checked_add(
                    ((index * step / channels) as u64 * 1_000_000_000 / rate as u64) as i64,
                )
            });
            packet.callback_end_ns = captured_ns.and_then(|ns| ns.checked_add(duration_ns as i64));
            packet.rate = rate;
            packet.channels = channels;
            let chunk_ns = (chunk.len() / channels) as u64 * 1_000_000_000 / rate as u64;
            let queued = self
                .metrics
                .queued_ns
                .fetch_add(chunk_ns, Ordering::Release)
                + chunk_ns;
            self.metrics
                .high_water_ns
                .fetch_max(queued, Ordering::Relaxed);
            if let Err(rtrb::PushError::Full(packet)) = self.ready.push(packet) {
                self.metrics
                    .queued_ns
                    .fetch_sub(chunk_ns, Ordering::Release);
                self.spare = Some(packet);
                return false;
            }
        }
        true
    }
}

impl AudioReceiver {
    pub fn metrics(&self) -> Arc<Metrics> {
        Arc::clone(&self.metrics)
    }
    /// Only the normalizer may wait. Short sleeps avoid callback-side wakeups.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<AudioPacket, RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(packet) = self.ready.pop() {
                let ns = (packet.samples.len() / packet.channels) as u64 * 1_000_000_000
                    / packet.rate as u64;
                self.metrics.queued_ns.fetch_sub(ns, Ordering::AcqRel);
                self.metrics
                    .consumed_ns
                    .store(self.metrics.now(), Ordering::Relaxed);
                return Ok(packet);
            }
            if self.ready.is_abandoned() {
                return Err(RecvTimeoutError::Disconnected);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(RecvTimeoutError::Timeout);
            }
            std::thread::sleep(left.min(Duration::from_millis(1)));
        }
    }

    pub fn recycle(&mut self, mut packet: AudioPacket) {
        packet.samples.clear();
        // The callback cannot own more than the preallocated pool. A full
        // return ring is only possible after caller misuse; drop off callback.
        let _ = self.free.push(packet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_are_reused_and_overflow_never_overwrites() {
        let (mut tx, mut rx) = channel(48000, 2).unwrap();
        let samples = vec![0.25; 2048];
        let mut pointers = std::collections::HashSet::new();
        let count = tx.free.slots();
        for _ in 0..count {
            assert!(tx.enqueue(&samples, 48000, 2, None, |x| x));
        }
        assert!(!tx.enqueue(&samples, 48000, 2, None, |x| x));
        for _ in 0..count {
            let packet = rx.recv_timeout(Duration::ZERO).unwrap();
            pointers.insert(packet.samples.as_ptr() as usize);
            assert_eq!(packet.samples, samples);
            rx.recycle(packet);
        }
        for _ in 0..count * 10 {
            assert!(tx.enqueue(&samples, 48000, 2, None, |x| x));
            let packet = rx.recv_timeout(Duration::ZERO).unwrap();
            assert!(pointers.contains(&(packet.samples.as_ptr() as usize)));
            rx.recycle(packet);
        }
        assert_eq!(pointers.len(), count);
        let view = rx.metrics().view();
        assert_eq!(view.dropped_frames, 1024);
        assert_eq!(view.callback_frames, (count * 11 + 1) as u64 * 1024);
        assert_eq!(view.queued_ms, 0);
        assert!(view.ring_high_water_ms <= 2000);
        assert_eq!(
            view.callback_histogram_us.iter().sum::<u64>(),
            (count * 11 + 1) as u64
        );
    }

    #[test]
    fn split_callbacks_preserve_samples_and_monotonic_sample_time() {
        let (mut tx, mut rx) = channel(44100, 2).unwrap();
        let input: Vec<i16> = (0..10000).map(|n| n as i16).collect();
        let mut start = None;
        assert!(tx.enqueue(&input, 44100, 2, None, |v| v as f32));
        let mut recovered = Vec::new();
        while let Ok(packet) = rx.recv_timeout(Duration::ZERO) {
            let start = *start.get_or_insert(packet.start_at.unwrap());
            let expected = start + Duration::from_secs_f64((recovered.len() / 2) as f64 / 44100.);
            assert_eq!(packet.start_at, Some(expected));
            recovered.extend_from_slice(&packet.samples);
            rx.recycle(packet);
        }
        assert_eq!(
            recovered,
            input.iter().map(|v| *v as f32).collect::<Vec<_>>()
        );
        assert!(!tx.enqueue(&[1.0; 3], 44100, 2, None, |v| v));
        drop(tx);
        assert!(matches!(
            rx.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn unusual_formats_still_have_a_byte_budget() {
        for (rate, channels) in [(48000, 1), (192000, 8), (384000, 256)] {
            let (tx, _) = channel(rate, channels).unwrap();
            assert!(tx.free.slots() * tx.packet_samples * size_of::<f32>() <= MAX_BUFFER_BYTES);
        }
    }
    #[test]
    fn mono_callbacks_respect_time_budget_in_a_stereo_pool() {
        let (mut tx, mut rx) = channel(48000, 2).unwrap();
        let samples = vec![0.1; 48000];
        assert!(tx.enqueue(&samples, 48000, 1, Some(1_000_000_000), |v| v));
        assert!(tx.enqueue(&samples, 48000, 1, Some(2_000_000_000), |v| v));
        assert!(!tx.enqueue(&[0.1; 1024], 48000, 1, None, |v| v));
        let first = rx.recv_timeout(Duration::ZERO).unwrap();
        assert_eq!(first.callback_end_ns, Some(2_000_000_000));
        rx.recycle(first);
        assert!(tx.enqueue(&[0.1; 1024], 48000, 1, None, |v| v));
    }
}
