use crate::capture::{self, AudioPacket, AudioReceiver};
use crate::media::{OpusBlock, SealedAudio};
use crate::microphone::Microphone;
use crate::retention::{self, Retention};
use crate::system_audio::SystemAudio;
use fs2::available_space;
use hear_protocol::AudioSource;
use sherpa_onnx::LinearResampler;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(crate) const RATE: usize = 48_000;
const BLOCK_FRAMES: usize = RATE * 5;

pub struct BlockRecord {
    pub id: Uuid,
    pub source: AudioSource,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub media: Option<SealedAudio>,
    pub frames: usize,
    pub sample_start: u64,
    pub decision: &'static str,
    pub preparing: bool,
}

pub struct RecordGap {
    pub source: AudioSource,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub reason: &'static str,
}

pub enum RecordEvent {
    Block(BlockRecord),
    Gap(RecordGap),
}

pub struct RecordResult {
    pub blocks: usize,
    pub frames: u64,
    pub dropped_packets: u64,
}

fn callback_stalled(last_callback_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_callback_at) >= Duration::from_secs(3)
}

enum Device {
    Microphone(Microphone),
    System(SystemAudio, AudioReceiver),
}

impl Device {
    fn open(source: AudioSource) -> Result<Self, String> {
        match source {
            AudioSource::Microphone => Ok(Self::Microphone(Microphone::start()?)),
            AudioSource::System => {
                let (sender, receiver) = capture::channel(48_000, 2)?;
                Ok(Self::System(SystemAudio::start(sender)?, receiver))
            }
        }
    }

    fn receiver(&mut self) -> &mut AudioReceiver {
        match self {
            Self::Microphone(device) => &mut device.receiver,
            Self::System(_, receiver) => receiver,
        }
    }

    fn failed(&self) -> bool {
        match self {
            Self::Microphone(device) => device.stream_failed.load(Ordering::Relaxed),
            Self::System(device, _) => device.failed(),
        }
    }

    fn dropped(&self) -> u64 {
        match self {
            Self::Microphone(device) => device.dropped_packets.load(Ordering::Relaxed),
            Self::System(device, _) => device.dropped.load(Ordering::Relaxed),
        }
    }
}

struct UnsealedBlock {
    record: BlockRecord,
    samples: Vec<f32>,
}

enum SealInput {
    Block(UnsealedBlock),
    Gap(RecordGap),
}

struct Assembler<F: FnMut(SealInput) -> Result<(), String>> {
    source: AudioSource,
    persist: F,
    started_ms: Option<i64>,
    samples: Vec<f32>,
    blocks: usize,
    frames: u64,
    captured_frames: u64,
    last_packet_end_ms: Option<i64>,
    session_started_ms: i64,
}

impl<F: FnMut(SealInput) -> Result<(), String>> Assembler<F> {
    fn push(&mut self, samples: &[f32], sample_start_ms: i64) -> Result<(), String> {
        if self.last_packet_end_ms.is_none()
            && sample_start_ms.saturating_sub(self.session_started_ms) > 250
        {
            (self.persist)(SealInput::Gap(RecordGap {
                source: self.source,
                started_ms: self.session_started_ms,
                ended_ms: sample_start_ms,
                reason: "source_start_gap",
            }))?;
            self.frames = self.frames.saturating_add(
                sample_start_ms.saturating_sub(self.session_started_ms) as u64 * RATE as u64 / 1000,
            );
        }
        if let Some(previous_end) = self.last_packet_end_ms {
            let gap_ms = sample_start_ms.saturating_sub(previous_end);
            if gap_ms > 250 {
                self.seal()?;
                (self.persist)(SealInput::Gap(RecordGap {
                    source: self.source,
                    started_ms: previous_end,
                    ended_ms: sample_start_ms,
                    reason: "callback_gap",
                }))?;
                // The sample coordinate represents the *real* source time.
                // A period with no callback is not audio, and must not make
                // speech on either side appear adjacent to the VAD or UI.
                self.frames = self
                    .frames
                    .saturating_add(gap_ms as u64 * RATE as u64 / 1000);
            }
        }
        self.last_packet_end_ms =
            Some(sample_start_ms.saturating_add(samples.len() as i64 * 1000 / RATE as i64));
        let mut consumed = 0;
        while consumed < samples.len() {
            if self.started_ms.is_none() {
                self.started_ms = Some(sample_start_ms + consumed as i64 * 1000 / RATE as i64);
            }
            let available = BLOCK_FRAMES - self.samples.len();
            let take = available.min(samples.len() - consumed);
            self.samples
                .extend_from_slice(&samples[consumed..consumed + take]);
            consumed += take;
            if self.samples.len() == BLOCK_FRAMES {
                self.seal()?;
            }
        }
        Ok(())
    }

    fn seal(&mut self) -> Result<(), String> {
        if self.samples.is_empty() {
            return Ok(());
        }
        let started_ms = self.started_ms.take().unwrap();
        let frames = self.samples.len();
        let sample_start = self.frames;
        let ended_ms = started_ms + frames as i64 * 1000 / RATE as i64;
        let id = Uuid::new_v4();
        let decision = retention::classify(&self.samples).code();
        // The normalizer transfers ownership, then immediately consumes more
        // callbacks. Only the separate bounded writer encodes/fsyncs/publishes.
        let samples = std::mem::replace(&mut self.samples, Vec::with_capacity(BLOCK_FRAMES));
        (self.persist)(SealInput::Block(UnsealedBlock {
            record: BlockRecord {
                id,
                source: self.source,
                started_ms,
                ended_ms,
                media: None,
                frames,
                sample_start,
                decision,
                preparing: false,
            },
            samples,
        }))?;
        self.blocks += 1;
        self.frames += frames as u64;
        self.captured_frames += frames as u64;
        self.samples.clear();
        Ok(())
    }
}

fn persist_input(
    directory: &Path,
    persist: &mut impl FnMut(RecordEvent) -> Result<(), String>,
    input: SealInput,
) -> Result<(), String> {
    let SealInput::Block(UnsealedBlock {
        mut record,
        samples,
    }) = input
    else {
        if let SealInput::Gap(gap) = input {
            return persist(RecordEvent::Gap(gap));
        }
        unreachable!();
    };
    let digital_silence = record.decision == Retention::DiscardDigitalSilence.code();
    if !digital_silence {
        persist(RecordEvent::Block(BlockRecord {
            id: record.id,
            source: record.source,
            started_ms: record.started_ms,
            ended_ms: record.ended_ms,
            media: None,
            frames: record.frames,
            sample_start: record.sample_start,
            decision: record.decision,
            preparing: true,
        }))?;
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        if available_space(directory).map_err(|e| e.to_string())? < 1_073_741_824 {
            return Err("storage_low".into());
        }
        let mut block = OpusBlock::create(directory, record.id)?;
        block.push(&samples)?;
        record.media = Some(block.seal()?);
    }
    persist(RecordEvent::Block(record))
}

fn wall_ms(time: SystemTime) -> Result<i64, String> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?;
    i64::try_from(duration.as_millis()).map_err(|error| error.to_string())
}

#[derive(Clone, Copy)]
pub struct ClockAnchor {
    at: Instant,
    pub wall_ms: i64,
    pub monotonic_ns: i64,
}

impl ClockAnchor {
    pub fn from_coordinates(wall_ms: i64, monotonic_ns: i64) -> Result<Self, String> {
        let current = Self::now()?;
        let elapsed = current
            .monotonic_ns
            .checked_sub(monotonic_ns)
            .filter(|elapsed| *elapsed >= 0)
            .ok_or("invalid capture clock anchor")?;
        let at = current
            .at
            .checked_sub(Duration::from_nanos(elapsed as u64))
            .ok_or("capture clock anchor too old")?;
        Ok(Self {
            at,
            wall_ms,
            monotonic_ns,
        })
    }

    pub fn now() -> Result<Self, String> {
        let at = Instant::now();
        let wall_ms = wall_ms(SystemTime::now())?;
        let mut uptime = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut uptime) } != 0 {
            return Err("monotonic clock unavailable".to_owned());
        }
        Ok(Self {
            at,
            wall_ms,
            monotonic_ns: uptime
                .tv_sec
                .saturating_mul(1_000_000_000)
                .saturating_add(uptime.tv_nsec),
        })
    }

    fn packet_start_ms(&self, packet: &AudioPacket, duration: Duration) -> Result<i64, String> {
        let elapsed_ms = packet
            .at
            .checked_duration_since(self.at)
            .ok_or("audio callback preceded the session clock anchor")?
            .as_millis() as i64;
        let mapped_end = self.wall_ms.saturating_add(elapsed_ms);
        let observed_end = wall_ms(packet.wall_at)?;
        if mapped_end.abs_diff(observed_end) > 2_000 {
            return Err("wall clock changed during audio capture".to_owned());
        }
        if let Some(start_ns) = packet.sample_start_ns {
            let start_ms = self
                .wall_ms
                .saturating_add(start_ns.saturating_sub(self.monotonic_ns) / 1_000_000);
            let hardware_end = packet.callback_end_ns.unwrap_or(start_ns);
            let mapped_hardware_end = self
                .wall_ms
                .saturating_add(hardware_end.saturating_sub(self.monotonic_ns) / 1_000_000);
            if mapped_hardware_end.abs_diff(mapped_end) > 2_000 {
                return Err("audio hardware clock changed during capture".into());
            }
            return Ok(start_ms);
        }
        if let Some(start) = packet.start_at {
            return Ok(if start >= self.at {
                self.wall_ms
                    .saturating_add(start.duration_since(self.at).as_millis() as i64)
            } else {
                self.wall_ms
                    .saturating_sub(self.at.duration_since(start).as_millis() as i64)
            });
        }
        Ok(mapped_end.saturating_sub(duration.as_millis() as i64))
    }
}

pub fn record_session(
    source: AudioSource,
    seconds: Option<u8>,
    stop: &AtomicBool,
    directory: &Path,
    clock: ClockAnchor,
    persist: impl FnMut(RecordEvent) -> Result<(), String> + Send,
    started: impl FnOnce(),
) -> Result<RecordResult, String> {
    if seconds.is_some_and(|v| !(1..=60).contains(&v)) {
        return Err("invalid recording duration".into());
    }
    run_source(
        source,
        seconds.map(|s| Duration::from_secs(s as u64)),
        stop,
        directory,
        clock,
        persist,
        started,
    )
}

fn run_source(
    source: AudioSource,
    duration: Option<Duration>,
    stop: &AtomicBool,
    directory: &Path,
    clock: ClockAnchor,
    mut persist: impl FnMut(RecordEvent) -> Result<(), String> + Send,
    started: impl FnOnce(),
) -> Result<RecordResult, String> {
    let device = Device::open(source)?;
    let deadline = duration.map(|duration| Instant::now() + duration);
    started();
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(2);
        let writer = scope.spawn(move || {
            for input in receiver {
                persist_input(directory, &mut persist, input)?;
            }
            Ok::<_, String>(())
        });
        let captured = consume_source(device, source, deadline, stop, clock, |input| {
            sender.try_send(input).map_err(|e| match e {
                mpsc::TrySendError::Full(_) => {
                    "audio sealing queue overflowed; source stopped".to_owned()
                }
                mpsc::TrySendError::Disconnected(_) => "audio sealing worker stopped".to_owned(),
            })
        });
        drop(sender);
        let sealed = writer
            .join()
            .map_err(|_| "audio sealing worker panicked".to_owned())?;
        sealed.and(captured)
    })
}

fn consume_source(
    mut device: Device,
    source: AudioSource,
    deadline: Option<Instant>,
    stop: &AtomicBool,
    clock: ClockAnchor,
    persist: impl FnMut(SealInput) -> Result<(), String>,
) -> Result<RecordResult, String> {
    let mut assembler = Assembler {
        source,
        persist,
        started_ms: None,
        samples: Vec::with_capacity(BLOCK_FRAMES),
        blocks: 0,
        frames: 0,
        captured_frames: 0,
        last_packet_end_ms: None,
        session_started_ms: clock.wall_ms,
    };
    let mut resampler: Option<LinearResampler> = None;
    let mut input_format = None;
    let mut mono_buffer = Vec::with_capacity(1024);
    // A wall-clock adjustment must not move later samples before earlier ones.
    // The first callback anchors this source session; subsequent coordinates
    // are derived only from callback monotonic time.
    let mut previous_packet_at: Option<Instant> = None;
    let mut last_callback_at = Instant::now();
    while !stop.load(Ordering::Acquire) && deadline.is_none_or(|at| Instant::now() < at) {
        let wait = deadline
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(250))
            .min(Duration::from_millis(250));
        match device.receiver().recv_timeout(wait) {
            Ok(packet) => {
                last_callback_at = Instant::now();
                if device.dropped() != 0 {
                    assembler.seal()?;
                    return Err("audio callback queue overflowed; source stopped".to_owned());
                }
                if input_format.is_some_and(|format| format != (packet.rate, packet.channels)) {
                    assembler.seal()?;
                    return Err("audio source format changed; source stopped".to_owned());
                }
                if packet.channels == 0 || packet.samples.len() % packet.channels != 0 {
                    assembler.seal()?;
                    return Err("invalid audio source channel layout".to_owned());
                }
                input_format = Some((packet.rate, packet.channels));
                let frames = packet.samples.len() / packet.channels;
                let duration = Duration::from_secs_f64(frames as f64 / packet.rate as f64);
                if previous_packet_at.is_some_and(|at| packet.at < at) {
                    assembler.seal()?;
                    return Err("audio callbacks arrived out of monotonic order".to_owned());
                }
                previous_packet_at = Some(packet.at);
                let start_ms = match clock.packet_start_ms(&packet, duration) {
                    Ok(start) => start,
                    Err(error) => {
                        assembler.seal()?;
                        return Err(error);
                    }
                };
                let mono = if packet.channels == 1 {
                    packet.samples.as_slice()
                } else {
                    mono_buffer.clear();
                    mono_buffer.extend(
                        packet
                            .samples
                            .chunks_exact(packet.channels)
                            .map(|frame| frame.iter().sum::<f32>() / packet.channels as f32),
                    );
                    mono_buffer.as_slice()
                };
                if packet.rate as usize != RATE && resampler.is_none() {
                    resampler = LinearResampler::create(packet.rate as i32, RATE as i32);
                    if resampler.is_none() {
                        return Err("audio resampler unavailable".to_owned());
                    }
                }
                if let Some(resampler) = resampler.as_ref() {
                    assembler.push(&resampler.resample(mono, false), start_ms)?;
                } else {
                    assembler.push(mono, start_ms)?;
                }
                device.receiver().recycle(packet);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                assembler.seal()?;
                return Err("audio callback stream disconnected".to_owned());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if callback_stalled(last_callback_at, Instant::now()) {
                    assembler.seal()?;
                    return Err("audio callbacks stalled; source stopped".to_owned());
                }
            }
        }
        if device.failed() {
            assembler.seal()?;
            return Err("audio source stopped unexpectedly".to_owned());
        }
    }
    if let Some(resampler) = resampler.as_ref() {
        let final_start = assembler
            .last_packet_end_ms
            .unwrap_or(wall_ms(SystemTime::now())?);
        assembler.push(&resampler.resample(&[], true), final_start)?;
    }
    assembler.seal()?;
    Ok(RecordResult {
        blocks: assembler.blocks,
        frames: assembler.captured_frames,
        dropped_packets: device.dropped(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_callbacks_leave_a_time_gap_without_inventing_recorded_audio() {
        let temporary = tempfile::tempdir().unwrap();
        let mut events = Vec::new();
        let mut assembler = Assembler {
            source: AudioSource::System,
            persist: |input| {
                persist_input(
                    temporary.path(),
                    &mut |event| {
                        events.push(event);
                        Ok(())
                    },
                    input,
                )
            },
            started_ms: None,
            samples: Vec::new(),
            blocks: 0,
            frames: 0,
            captured_frames: 0,
            last_packet_end_ms: None,
            session_started_ms: 1_000,
        };
        assembler.push(&vec![0.0; RATE], 1_000).unwrap();
        assembler.push(&vec![0.0; RATE], 3_000).unwrap();
        assembler.seal().unwrap();
        drop(assembler);
        assert_eq!(events.len(), 3);
        let blocks: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                RecordEvent::Block(block) => Some(block),
                RecordEvent::Gap(_) => None,
            })
            .collect();
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&events[1], RecordEvent::Gap(gap)
            if gap.started_ms == 2_000 && gap.ended_ms == 3_000));
        assert_eq!(
            (
                blocks[0].started_ms,
                blocks[0].ended_ms,
                blocks[0].sample_start
            ),
            (1_000, 2_000, 0)
        );
        assert_eq!(
            (
                blocks[1].started_ms,
                blocks[1].ended_ms,
                blocks[1].sample_start
            ),
            (3_000, 4_000, 96_000)
        );
        assert!(blocks.iter().all(|block| block.media.is_none()));
    }

    #[test]
    fn callback_stall_is_distinct_from_normal_silent_samples() {
        let at = Instant::now();
        assert!(!callback_stalled(at, at + Duration::from_millis(2_999)));
        assert!(callback_stalled(at, at + Duration::from_secs(3)));
    }

    #[test]
    fn wall_clock_change_cannot_reorder_a_monotonic_session() {
        let at = Instant::now();
        let clock = ClockAnchor {
            at,
            wall_ms: 10_000,
            monotonic_ns: 4_000_000_000,
        };
        let packet = |at, wall_at| AudioPacket {
            at,
            wall_at,
            start_at: None,
            sample_start_ns: None,
            callback_end_ns: None,
            samples: vec![0.1; 4_800],
            rate: 48_000,
            channels: 1,
        };
        let first = packet(
            at + Duration::from_secs(1),
            UNIX_EPOCH + Duration::from_millis(11_000),
        );
        assert_eq!(
            clock
                .packet_start_ms(&first, Duration::from_millis(100))
                .unwrap(),
            10_900
        );
        let changed = packet(
            at + Duration::from_secs(2),
            UNIX_EPOCH + Duration::from_millis(7_000),
        );
        assert!(
            clock
                .packet_start_ms(&changed, Duration::from_millis(100))
                .is_err()
        );
    }
}
