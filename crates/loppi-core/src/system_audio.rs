use crate::capture::AudioSender;
use loppi_protocol::ProbeStats;
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const TAP_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const TAP_START_TIMEOUT: Duration = Duration::from_secs(8);

fn tap_ready_marker() -> Result<PathBuf, String> {
    Ok(loppi_protocol::data_dir()?.join("tap-ready-v1"))
}

pub fn tap_available() -> bool {
    unsafe extern "C" {
        fn loppi_system_audio_tap_available() -> i32;
    }
    unsafe { loppi_system_audio_tap_available() != 0 }
}

pub fn tap_ready() -> bool {
    tap_ready_marker().is_ok_and(|path| path.is_file())
}

pub fn probe_child(
    source: loppi_protocol::AudioSource,
    seconds: u8,
    timeout: Duration,
) -> Result<ProbeStats, String> {
    let binary = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut probe = Command::new(binary)
        .args([
            if source == loppi_protocol::AudioSource::Microphone {
                "probe-mic"
            } else {
                "probe-system"
            },
            &seconds.to_string(),
            "--parent-pid",
            &std::process::id().to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("system audio permission probe could not start: {error}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match probe.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut output = String::new();
                probe
                    .stdout
                    .take()
                    .ok_or("system audio probe output missing")?
                    .read_to_string(&mut output)
                    .map_err(|error| error.to_string())?;
                let stats: ProbeStats =
                    serde_json::from_str(&output).map_err(|error| error.to_string())?;
                if stats.frames == 0 {
                    return Err("system audio permission probe received no frames".to_owned());
                }
                return Ok(stats);
            }
            Ok(Some(status)) => {
                return Err(format!("system audio permission probe failed: {status}"));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = probe.kill();
                let _ = probe.wait();
                return Err("system audio permission probe timed out".to_owned());
            }
            Err(error) => {
                let _ = probe.kill();
                let _ = probe.wait();
                return Err(format!("system audio permission probe failed: {error}"));
            }
        }
    }
}

fn tap_preflight() -> Result<(), String> {
    probe_child(loppi_protocol::AudioSource::System, 1, TAP_PROBE_TIMEOUT).map(|_| ())
}

struct CallbackState {
    sender: UnsafeCell<AudioSender>,
    busy: AtomicBool,
    dropped: Arc<AtomicU64>,
}

unsafe extern "C" fn receive_audio(
    context: *mut c_void,
    samples: *const f32,
    frames: u32,
    rate: u32,
    channels: u32,
    sample_start_ns: u64,
) {
    if context.is_null()
        || samples.is_null()
        || frames == 0
        || rate == 0
        || !(1..=2).contains(&channels)
        || frames > rate * 2
    {
        return;
    }
    // SCK dispatches on a serial queue; HAL uses one IOProc for this source.
    // Native stop drains callbacks before freeing this unique mutable owner.
    let state = unsafe { &*(context as *const CallbackState) };
    // Native APIs serialize this source; reject unexpected reentrancy before
    // borrowing the SPSC producer, rather than risking aliased mutable access.
    if state
        .busy
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        state.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let count = frames as usize * channels as usize;
    let data = unsafe { std::slice::from_raw_parts(samples, count) };
    let captured = (sample_start_ns != 0)
        .then(|| i64::try_from(sample_start_ns).ok())
        .flatten();
    if !unsafe { &mut *state.sender.get() }.enqueue(data, rate, channels as usize, captured, |v| v)
    {
        state.dropped.fetch_add(1, Ordering::Relaxed);
    }
    state.busy.store(false, Ordering::Release);
}

pub struct SystemAudio {
    handle: *mut c_void,
    context: *mut CallbackState,
    using_legacy: bool,
    can_upgrade: bool,
    pub dropped: Arc<AtomicU64>,
    metrics: Arc<crate::capture::Metrics>,
}

impl SystemAudio {
    pub fn start(sender: AudioSender) -> Result<Self, String> {
        let metrics = sender.metrics();
        unsafe extern "C" {
            fn loppi_system_audio_start(
                context: *mut c_void,
                callback: unsafe extern "C" fn(*mut c_void, *const f32, u32, u32, u32, u64),
                stage: *mut i32,
                prefer_tap: i32,
                native_error: *mut i32,
            ) -> *mut c_void;
        }
        let tap_available = tap_available();
        let marker = tap_ready_marker()?;
        let use_tap = tap_available && marker.is_file();
        if use_tap {
            if let Err(error) = tap_preflight() {
                // A revoked or unusable grant must never leave the login item
                // blocked inside HAL. Next retry uses the legacy source until
                // the user grants the Tap in the foreground again.
                let _ = std::fs::remove_file(&marker);
                return Err(error);
            }
        }
        let dropped = Arc::new(AtomicU64::new(0));
        let context = Box::into_raw(Box::new(CallbackState {
            sender: UnsafeCell::new(sender),
            busy: AtomicBool::new(false),
            dropped: Arc::clone(&dropped),
        }));
        let (handle, stage, native_error) = if use_tap {
            let (result_sender, result_receiver) = mpsc::sync_channel(1);
            let context_address = context as usize;
            let spawned = thread::Builder::new()
                .name("loppi-tap-start".to_owned())
                .spawn(move || {
                    let mut stage = 0;
                    let mut native_error = 0;
                    let handle = unsafe {
                        loppi_system_audio_start(
                            context_address as *mut c_void,
                            receive_audio,
                            &mut stage,
                            1,
                            &mut native_error,
                        )
                    };
                    let _ = result_sender.send((handle as usize, stage, native_error));
                });
            if let Err(error) = spawned {
                unsafe { drop(Box::from_raw(context)) };
                return Err(format!("system audio startup thread failed: {error}"));
            }
            match result_receiver.recv_timeout(TAP_START_TIMEOUT) {
                Ok((handle, stage, native_error)) => (handle as *mut c_void, stage, native_error),
                Err(error) => {
                    let _ = std::fs::remove_file(&marker);
                    // The HAL call has no cancellation API. Do not free its
                    // callback context or join a thread blocked in HAL: exit
                    // this helper so launchd can restart with the legacy
                    // source and a durable gap instead of hanging forever.
                    eprintln!("loppi-core Tap startup exceeded deadline: {error}");
                    // Forced helper termination must not run global native
                    // destructors concurrently with capture/model threads.
                    unsafe { libc::_exit(70) };
                }
            }
        } else {
            let mut stage = 0;
            let mut native_error = 0;
            let handle = unsafe {
                loppi_system_audio_start(
                    context.cast(),
                    receive_audio,
                    &mut stage,
                    0,
                    &mut native_error,
                )
            };
            (handle, stage, native_error)
        };
        if handle.is_null() {
            unsafe { drop(Box::from_raw(context)) };
            if use_tap {
                let _ = std::fs::remove_file(&marker);
            }
            return Err(if native_error == 0 {
                format!("system audio capture failed at stage {stage}")
            } else {
                format!("system audio capture failed at stage {stage} native {native_error}")
            });
        }
        Ok(Self {
            handle,
            context,
            using_legacy: !use_tap,
            can_upgrade: tap_available,
            dropped,
            metrics,
        })
    }

    pub fn error(&self) -> Option<String> {
        unsafe extern "C" {
            fn loppi_system_audio_error(handle: *mut c_void) -> i32;
            fn loppi_system_audio_exclusion_generation(handle: *mut c_void) -> u64;
        }
        // A successful foreground permission probe switches the running
        // legacy source to the Tap at the next capture supervisor retry.
        if self.using_legacy
            && self.can_upgrade
            && tap_ready_marker().is_ok_and(|path| path.is_file())
        {
            return Some("system_audio_tap_upgrade".into());
        }
        let code = unsafe { loppi_system_audio_error(self.handle) };
        self.metrics
            .exclusion_generation(unsafe { loppi_system_audio_exclusion_generation(self.handle) });
        (code != 0).then(|| format!("system audio native error {code}"))
    }
}

pub fn playback_exclusion_generation() -> u64 {
    unsafe extern "C" {
        fn loppi_playback_exclusion_generation() -> u64;
    }
    unsafe { loppi_playback_exclusion_generation() }
}

impl Drop for SystemAudio {
    fn drop(&mut self) {
        unsafe extern "C" {
            fn loppi_system_audio_stop(handle: *mut c_void) -> i32;
        }
        let status = unsafe { loppi_system_audio_stop(self.handle) };
        if status == -2 {
            // A timed-out SCStream can still capture and invoke callbacks.
            // The owning helper must exit rather than report this source as
            // stopped while its native stream remains live.
            eprintln!("loppi-core system audio stop timed out; terminating capture owner");
            // Forced helper termination must not run global native
            // destructors concurrently with capture/model threads.
            unsafe { libc::_exit(70) };
        } else {
            unsafe { drop(Box::from_raw(self.context)) };
        }
    }
}

/// Observation only: main-display power is not machine sleep or a capture gate.
pub fn main_display_asleep() -> Option<bool> {
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayIsAsleep(display: u32) -> u32;
    }
    let display = unsafe { CGMainDisplayID() };
    (display != 0).then(|| unsafe { CGDisplayIsAsleep(display) != 0 })
}
