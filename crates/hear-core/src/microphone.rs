use crate::capture::AudioPacket;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use hear_protocol::{AudioSource, ProbeStats};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant, SystemTime};

pub struct Microphone {
    _stream: Stream,
    pub receiver: mpsc::Receiver<AudioPacket>,
    pub sample_rate: u32,
    pub channels: usize,
    pub dropped_packets: Arc<AtomicU64>,
    pub stream_failed: Arc<AtomicBool>,
}

impl Microphone {
    pub fn start() -> Result<Self, String> {
        unsafe extern "C" {
            fn hear_microphone_permission_status() -> i32;
        }
        let authorization = unsafe { hear_microphone_permission_status() };
        if authorization != 3 {
            return Err(format!(
                "microphone permission unavailable (status {authorization})"
            ));
        }
        let device = cpal::default_host()
            .default_input_device()
            .ok_or_else(|| "no microphone input device is available".to_owned())?;
        let supported = device
            .default_input_config()
            .map_err(|error| error.to_string())?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let config: StreamConfig = supported.into();
        let (sender, receiver) = mpsc::sync_channel(64);
        let dropped_packets = Arc::new(AtomicU64::new(0));
        let stream_failed = Arc::new(AtomicBool::new(false));
        let dropped = Arc::clone(&dropped_packets);
        let failed = Arc::clone(&stream_failed);
        let on_error = move |error| {
            failed.store(true, Ordering::Relaxed);
            eprintln!("hear-core microphone stream failed: {error}");
        };
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _| {
                    enqueue(data, &sender, &dropped, sample_rate, channels, |sample| {
                        sample
                    })
                },
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], _| {
                    enqueue(data, &sender, &dropped, sample_rate, channels, |sample| {
                        sample as f32 / i16::MAX as f32
                    })
                },
                on_error,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                config,
                move |data: &[u16], _| {
                    enqueue(data, &sender, &dropped, sample_rate, channels, |sample| {
                        sample as f32 / (u16::MAX as f32 / 2.0) - 1.0
                    })
                },
                on_error,
                None,
            ),
            other => return Err(format!("unsupported microphone format: {other:?}")),
        }
        .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self {
            _stream: stream,
            receiver,
            sample_rate,
            channels,
            dropped_packets,
            stream_failed,
        })
    }

    pub fn probe(seconds: u8) -> Result<ProbeStats, String> {
        let microphone = Self::start()?;
        let deadline = Instant::now() + Duration::from_secs(seconds as u64);
        let mut frames = 0usize;
        let mut peak = 0.0f32;
        let mut first_packet = None;
        while Instant::now() < deadline {
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(200));
            if let Ok(packet) = microphone.receiver.recv_timeout(wait) {
                first_packet.get_or_insert(packet.at);
                frames += packet.samples.len() / microphone.channels;
                for sample in packet.samples {
                    peak = peak.max(sample.abs());
                }
            }
            if microphone.stream_failed.load(Ordering::Relaxed) {
                return Err("microphone capture failed after starting".to_owned());
            }
        }
        if frames == 0 || first_packet.is_none() {
            return Err("microphone returned no audio frames".to_owned());
        }
        Ok(ProbeStats {
            source: AudioSource::Microphone,
            sample_rate: microphone.sample_rate,
            channels: microphone.channels as u16,
            frames: frames as u64,
            peak,
            dropped_packets: microphone.dropped_packets.load(Ordering::Relaxed),
        })
    }
}

fn enqueue<T: Copy>(
    data: &[T],
    sender: &mpsc::SyncSender<AudioPacket>,
    dropped: &AtomicU64,
    rate: u32,
    channels: usize,
    convert: impl Fn(T) -> f32,
) {
    let packet = AudioPacket {
        at: Instant::now(),
        wall_at: SystemTime::now(),
        samples: data.iter().copied().map(convert).collect(),
        rate,
        channels,
    };
    if sender.try_send(packet).is_err() {
        dropped.fetch_add(1, Ordering::Relaxed);
    }
}
