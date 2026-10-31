use crate::capture::{self, AudioReceiver, AudioSender};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use loppi_protocol::{AudioSource, ProbeStats};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub struct Microphone {
    _stream: Stream,
    pub receiver: AudioReceiver,
    pub sample_rate: u32,
    pub channels: usize,
    pub dropped_packets: Arc<AtomicU64>,
    pub stream_failed: Arc<AtomicBool>,
}

pub fn allow_idle_sleep() -> i32 {
    unsafe extern "C" {
        fn loppi_allow_audio_idle_sleep() -> i32;
    }
    unsafe { loppi_allow_audio_idle_sleep() }
}

impl Microphone {
    pub fn start() -> Result<Self, String> {
        unsafe extern "C" {
            fn loppi_microphone_permission_status() -> i32;
        }
        let authorization = unsafe { loppi_microphone_permission_status() };
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
        let (mut sender, receiver) = capture::channel(sample_rate, channels)?;
        let dropped_packets = Arc::new(AtomicU64::new(0));
        let stream_failed = Arc::new(AtomicBool::new(false));
        let dropped = Arc::clone(&dropped_packets);
        let failed = Arc::clone(&stream_failed);
        let metrics = receiver.metrics();
        let on_error = move |error: cpal::Error| {
            let (kind, fatal) = stream_error(error.kind());
            metrics.native_error(kind, fatal);
            if fatal {
                failed.store(true, Ordering::Release);
            }
        };
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], info| {
                    enqueue(
                        data,
                        info,
                        &mut sender,
                        &dropped,
                        sample_rate,
                        channels,
                        |sample| sample,
                    )
                },
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], info| {
                    enqueue(
                        data,
                        info,
                        &mut sender,
                        &dropped,
                        sample_rate,
                        channels,
                        |sample| sample as f32 / i16::MAX as f32,
                    )
                },
                on_error,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                config,
                move |data: &[u16], info| {
                    enqueue(
                        data,
                        info,
                        &mut sender,
                        &dropped,
                        sample_rate,
                        channels,
                        |sample| sample as f32 / (u16::MAX as f32 / 2.0) - 1.0,
                    )
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
        let mut microphone = Self::start()?;
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
                for sample in &packet.samples {
                    peak = peak.max(sample.abs());
                }
                microphone.receiver.recycle(packet);
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

// CPAL explicitly reports route changes and denied realtime promotion as
// nonfatal notices. A lost device, xrun or invalidated stream still ends this
// source so its missing samples become a real gap rather than invented audio.
fn stream_error(kind: cpal::ErrorKind) -> (u64, bool) {
    use cpal::ErrorKind::*;
    let code = match kind {
        DeviceChanged => 1,
        RealtimeDenied => 2,
        DeviceBusy => 3,
        DeviceNotAvailable => 4,
        HostUnavailable => 5,
        InvalidInput => 6,
        PermissionDenied => 7,
        ResourceExhausted => 8,
        StreamInvalidated => 9,
        UnsupportedConfig => 10,
        UnsupportedOperation => 11,
        Xrun => 12,
        BackendError => 13,
        _ => 14,
    };
    (code, !matches!(kind, DeviceChanged | RealtimeDenied))
}

#[cfg(test)]
mod tests {
    #[test]
    fn route_and_promotion_notices_do_not_destroy_a_running_stream() {
        assert!(!super::stream_error(cpal::ErrorKind::DeviceChanged).1);
        assert!(!super::stream_error(cpal::ErrorKind::RealtimeDenied).1);
        for kind in [
            cpal::ErrorKind::Xrun,
            cpal::ErrorKind::DeviceNotAvailable,
            cpal::ErrorKind::StreamInvalidated,
            cpal::ErrorKind::BackendError,
        ] {
            assert!(super::stream_error(kind).1);
        }
    }
}

fn enqueue<T: Copy>(
    data: &[T],
    info: &cpal::InputCallbackInfo,
    sender: &mut AudioSender,
    dropped: &AtomicU64,
    rate: u32,
    channels: usize,
    convert: impl Fn(T) -> f32,
) {
    // CoreAudio's CPAL timestamps use mach_absolute_time converted to ns,
    // the same uptime domain as the recording session clock.
    let captured_ns = i64::try_from(info.timestamp().capture.as_nanos()).ok();
    if !sender.enqueue(data, rate, channels, captured_ns, convert) {
        dropped.fetch_add(1, Ordering::Relaxed);
    }
}
