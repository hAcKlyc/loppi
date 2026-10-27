//! One native capture fault domain per source. Core never waits in HAL/SCK.
use crate::{ingestion, record};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const STOP_DEADLINE: Duration = Duration::from_secs(10);
const START_DEADLINE: Duration = Duration::from_secs(45);
const PROGRESS_DEADLINE: Duration = Duration::from_secs(15);

#[derive(Serialize, Deserialize)]
struct Config {
    session: ingestion::Session,
    seconds: Option<u8>,
}
#[derive(Serialize, Deserialize, Default)]
pub struct Progress {
    ready: bool,
    pub frames: u64,
    blocks: usize,
    dropped_packets: u64,
    finished: bool,
    error: Option<String>,
    #[serde(default)]
    pub capture: Option<crate::capture::MetricsView>,
}

pub fn clock_delta_ms(error: &str) -> Option<i64> {
    [
        "wall clock changed during audio capture: ",
        "audio hardware clock changed during capture: ",
    ]
    .iter()
    .find_map(|prefix| {
        error
            .strip_prefix(prefix)
            .and_then(|delta| delta.parse().ok())
    })
}

/// Keep operational causes while excluding arbitrary OS/provider messages.
pub fn error_code(error: &str) -> String {
    if clock_delta_ms(error).is_some() {
        return if error.starts_with("wall clock") {
            "capture_wall_clock_changed"
        } else {
            "capture_hardware_clock_changed"
        }
        .into();
    }
    if let Some((stage, native)) = error
        .strip_prefix("system audio capture failed at stage ")
        .and_then(|s| s.split_once(" native "))
    {
        if let (Ok(stage), Ok(native)) = (stage.parse::<i32>(), native.parse::<i32>()) {
            return format!(
                "system_audio_start_stage_{stage}_native_{}{}",
                if native < 0 { "neg" } else { "" },
                native.unsigned_abs()
            );
        }
    }
    for (prefix, code) in [
        (
            "system audio capture failed at stage ",
            "system_audio_start_stage",
        ),
        ("system audio native error ", "system_audio_native"),
        ("capture source exited with code ", "capture_source_exit"),
        (
            "capture source terminated by signal ",
            "capture_source_signal",
        ),
    ] {
        if let Some(number) = error
            .strip_prefix(prefix)
            .and_then(|v| v.parse::<i32>().ok())
        {
            return format!(
                "{code}_{}{}",
                if number < 0 { "neg" } else { "" },
                number.unsigned_abs()
            );
        }
    }
    let code = match error {
        "capture source heartbeat timed out" => "capture_heartbeat_timeout",
        "capture source start timed out" => "capture_start_timeout",
        "capture source stop timed out" => "capture_stop_timeout",
        "capture durable progress timed out" => "capture_progress_timeout",
        "capture progress exceeded limit" | "invalid capture progress" => {
            "capture_invalid_progress"
        }
        "audio callbacks stalled; source stopped" => "capture_callbacks_stalled",
        "audio callback queue overflowed; source stopped" => "capture_queue_overflow",
        "audio source format changed; source stopped" => "capture_format_changed",
        "audio callback stream disconnected" => "capture_callbacks_disconnected",
        "audio callbacks arrived out of monotonic order" => "capture_callbacks_out_of_order",
        "audio hardware clock changed during capture" => "capture_hardware_clock_changed",
        "wall clock changed during audio capture" => "capture_wall_clock_changed",
        "audio callback preceded the session clock anchor" => "capture_before_clock_anchor",
        "no microphone input device is available" => "microphone_device_unavailable",
        "microphone_stream_failed" => "microphone_stream_failed",
        "system_audio_tap_upgrade" => "system_audio_tap_upgrade",
        "storage_low" => "storage_low",
        _ if error.starts_with("microphone permission unavailable (status ") => {
            "microphone_permission_unavailable"
        }
        _ if error.contains("permission probe") => "system_audio_permission_probe_failed",
        _ => "capture_source_failed",
    };
    code.into()
}

fn exit_error(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        format!("capture source exited with code {code}")
    } else if let Some(signal) = status.signal() {
        format!("capture source terminated by signal {signal}")
    } else {
        "capture source exited".into()
    }
}

fn nonblocking(fd: i32) -> Result<(), String> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err("capture pipe could not be made nonblocking".into());
    }
    Ok(())
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn supervise(
    directory: &Path,
    session: ingestion::Session,
    seconds: Option<u8>,
    stop: &AtomicBool,
    durable_frames: &AtomicU64,
    started: impl FnMut(),
    progress: impl FnMut(u32, &Progress),
) -> Result<record::RecordResult, String> {
    if seconds.is_some_and(|v| !(1..=60).contains(&v)) {
        return Err("invalid recording duration".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let child = Command::new(exe)
        .args(["capture-source", &std::process::id().to_string()])
        .env("HEAR_DATA_DIR", directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    monitor(
        child,
        Config { session, seconds },
        stop,
        durable_frames,
        started,
        progress,
        Deadlines::default(),
    )
}

struct Deadlines {
    stop: Duration,
    start: Duration,
    progress: Duration,
    heartbeat: Duration,
}
impl Default for Deadlines {
    fn default() -> Self {
        Self {
            stop: STOP_DEADLINE,
            start: START_DEADLINE,
            progress: PROGRESS_DEADLINE,
            heartbeat: Duration::from_secs(3),
        }
    }
}
fn monitor(
    child: Child,
    config: Config,
    stop: &AtomicBool,
    durable_frames: &AtomicU64,
    mut started: impl FnMut(),
    mut progress: impl FnMut(u32, &Progress),
    deadlines: Deadlines,
) -> Result<record::RecordResult, String> {
    let mut child = OwnedChild(child);
    let mut input = Some(child.0.stdin.take().ok_or("capture input missing")?);
    let mut output = child.0.stdout.take().ok_or("capture output missing")?;
    let config = serde_json::to_vec(&config).map_err(|e| e.to_string())?;
    // The complete config is smaller than a pipe's atomic write; child reads
    // it before opening any device. No user or audio payload crosses this pipe.
    if config.len() > 4096 {
        return Err("capture config too large".into());
    }
    nonblocking(input.as_ref().unwrap().as_raw_fd())?;
    nonblocking(output.as_raw_fd())?;
    input
        .as_mut()
        .unwrap()
        .write_all(&(config.len() as u32).to_le_bytes())
        .and_then(|()| input.as_mut().unwrap().write_all(&config))
        .map_err(|e| e.to_string())?;
    let began = Instant::now();
    let mut heard = began;
    let mut sealed = began;
    let mut stopping = None;
    let mut ready = false;
    let mut last_frames = 0;
    let mut buffer = Vec::with_capacity(8192);
    let mut final_result = None;
    let mut exited = None;
    loop {
        if stop.load(Ordering::Acquire) && stopping.is_none() {
            stopping = Some(Instant::now());
            // Closing the writer is a durable lifecycle signal even if a
            // stop byte could not fit; the helper observes EOF.
            drop(input.take());
        }
        let mut chunk = [0; 4096];
        loop {
            match output.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    if buffer.len() > 8192 {
                        return Err("capture progress exceeded limit".into());
                    }
                    while let Some(end) = buffer.iter().position(|v| *v == b'\n') {
                        let message: Progress = serde_json::from_slice(&buffer[..end])
                            .map_err(|_| "invalid capture progress")?;
                        buffer.drain(..=end);
                        heard = Instant::now();
                        if message.ready && !ready {
                            ready = true;
                            sealed = Instant::now();
                            started();
                        }
                        if message.frames > last_frames {
                            last_frames = message.frames;
                            sealed = Instant::now();
                            durable_frames.store(last_frames, Ordering::Release);
                        }
                        progress(child.0.id(), &message);
                        if message.finished {
                            final_result = Some(match message.error {
                                Some(e) => Err(e),
                                None => Ok(record::RecordResult {
                                    blocks: message.blocks,
                                    frames: message.frames,
                                    dropped_packets: message.dropped_packets,
                                }),
                            });
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        if let Some(status) = exited {
            if !std::process::ExitStatus::success(&status) {
                return Err(exit_error(status));
            }
            return final_result.unwrap_or_else(|| Err(exit_error(status)));
        }
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            exited = Some(status);
            continue; // Drain the final pipe frame after observing exit.
        }
        if stopping.is_some_and(|at: Instant| at.elapsed() >= deadlines.stop) {
            return Err("capture source stop timed out".into());
        }
        if heard.elapsed() > deadlines.heartbeat {
            return Err("capture source heartbeat timed out".into());
        }
        if !ready && began.elapsed() > deadlines.start {
            return Err("capture source start timed out".into());
        }
        if ready && sealed.elapsed() > deadlines.progress {
            return Err("capture durable progress timed out".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn emit(progress: &Progress) {
    if let Ok(mut bytes) = serde_json::to_vec(progress) {
        bytes.push(b'\n');
        if bytes.len() <= 4096 {
            // One atomic nonblocking write. Dropping telemetry cannot lose
            // metadata: the journal, not this pipe, owns reliable delivery.
            unsafe {
                libc::write(libc::STDOUT_FILENO, bytes.as_ptr().cast(), bytes.len());
            }
        }
    }
}

pub fn run_child(parent: i32) -> Result<(), String> {
    if parent <= 1 || unsafe { libc::getppid() } != parent {
        return Err("capture parent unavailable".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let watch_stop = Arc::clone(&stop);
    std::thread::spawn(move || {
        let mut stopping = None;
        loop {
            if unsafe { libc::getppid() } != parent {
                // Forced helper termination must not run global native
                // destructors concurrently with capture/model threads.
                unsafe { libc::_exit(70) };
            }
            if watch_stop.load(Ordering::Acquire) {
                let began = stopping.get_or_insert_with(Instant::now);
                if began.elapsed() > STOP_DEADLINE {
                    // Forced helper termination must not run global native
                    // destructors concurrently with capture/model threads.
                    unsafe { libc::_exit(70) };
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let mut input = std::io::stdin();
    let mut size = [0; 4];
    input.read_exact(&mut size).map_err(|e| e.to_string())?;
    let size = u32::from_le_bytes(size) as usize;
    if !(1..=4096).contains(&size) {
        return Err("capture config exceeded limit".into());
    }
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    let config: Config = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if config.seconds.is_some_and(|v| !(1..=60).contains(&v)) {
        return Err("invalid recording duration".into());
    }
    let control_stop = Arc::clone(&stop);
    std::thread::spawn(move || {
        let mut byte = [0];
        let _ = input.read(&mut byte);
        control_stop.store(true, Ordering::Release);
    });
    nonblocking(libc::STDOUT_FILENO)?;
    let ready = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(AtomicU64::new(0));
    let finished = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(std::sync::OnceLock::<Arc<crate::capture::Metrics>>::new());
    let heartbeat = {
        let metrics = Arc::clone(&metrics);
        let ready = Arc::clone(&ready);
        let frames = Arc::clone(&frames);
        let finished = Arc::clone(&finished);
        std::thread::spawn(move || {
            while !finished.load(Ordering::Acquire) {
                emit(&Progress {
                    ready: ready.load(Ordering::Acquire),
                    frames: frames.load(Ordering::Acquire),
                    capture: metrics.get().map(|m| m.view()),
                    ..Default::default()
                });
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    };
    let _ = crate::microphone::allow_idle_sleep();
    let directory = hear_protocol::data_dir()?;
    let clock = record::ClockAnchor::from_coordinates(
        config.session.wall_anchor_ms,
        config.session.monotonic_anchor_ns,
    )?;
    let media = directory
        .join("media")
        .join(crate::source_name(config.session.source));
    let mut journal = ingestion::Journal::new(&directory, config.session.clone())?;
    let persist = |event| {
        let added = match &event {
            record::RecordEvent::Block(b) if !b.preparing => b.frames as u64,
            _ => 0,
        };
        journal.persist(event)?;
        frames.fetch_add(added, Ordering::Release);
        Ok(())
    };
    let result = record::record_session(
        config.session.source,
        config.seconds,
        &stop,
        &media,
        clock,
        persist,
        |capture_metrics| {
            let _ = metrics.set(capture_metrics);
            ready.store(true, Ordering::Release);
        },
    );
    finished.store(true, Ordering::Release);
    let _ = heartbeat.join();
    emit(&finished_progress(
        result,
        ready.load(Ordering::Acquire),
        frames.load(Ordering::Acquire),
        metrics.get().map(|m| m.view()),
    ));
    Ok(())
}

fn finished_progress(
    result: Result<record::RecordResult, String>,
    ready: bool,
    frames: u64,
    capture: Option<crate::capture::MetricsView>,
) -> Progress {
    let mut progress = Progress {
        ready,
        frames,
        finished: true,
        capture,
        ..Default::default()
    };
    match result {
        Ok(stats) => {
            progress.ready = true;
            progress.frames = stats.frames;
            progress.blocks = stats.blocks;
            progress.dropped_packets = stats.dropped_packets;
        }
        Err(error) => progress.error = Some(error),
    }
    progress
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_diagnostics_keep_native_codes_but_never_freeform_payloads() {
        for (error, expected) in [
            (
                "system audio native error -3808",
                "system_audio_native_neg3808",
            ),
            (
                "system audio capture failed at stage 3",
                "system_audio_start_stage_3",
            ),
            (
                "capture source exited with code 70",
                "capture_source_exit_70",
            ),
            (
                "capture source terminated by signal 9",
                "capture_source_signal_9",
            ),
            (
                "capture durable progress timed out",
                "capture_progress_timeout",
            ),
            (
                "audio callbacks stalled; source stopped",
                "capture_callbacks_stalled",
            ),
            (
                "system audio native error -3808 secret",
                "capture_source_failed",
            ),
            ("/Users/private/file: secret", "capture_source_failed"),
            (
                "system audio capture failed at stage 2 native -3815",
                "system_audio_start_stage_2_native_neg3815",
            ),
            (
                "system audio capture failed at stage 2 native -3815 secret",
                "capture_source_failed",
            ),
            (
                "audio hardware clock changed during capture: -2500",
                "capture_hardware_clock_changed",
            ),
            (
                "wall clock changed during audio capture: 3000",
                "capture_wall_clock_changed",
            ),
            (
                "wall clock changed during audio capture: secret",
                "capture_source_failed",
            ),
        ] {
            let code = error_code(error);
            assert_eq!(code, expected);
            let mut event = hear_protocol::LogEvent::simple(
                "capture",
                hear_protocol::LogLevel::Warn,
                "source_failed",
            );
            event.error_code = Some(code);
            assert!(crate::unilogs::valid_event(&event));
        }
    }
    #[test]
    fn error_exit_keeps_counts_after_the_last_heartbeat() {
        let (mut tx, rx) = crate::capture::channel(48_000, 1).unwrap();
        let metrics = rx.metrics();
        let last_heartbeat = metrics.view();
        let packet = [0.1_f32; 1024];
        while tx.enqueue(&packet, 48_000, 1, None, |v| v) {}
        assert_eq!(last_heartbeat.dropped_frames, 0);
        let final_message = finished_progress(
            Err("callback overflow".into()),
            true,
            240000,
            Some(metrics.view()),
        );
        let wire = serde_json::to_vec(&final_message).unwrap();
        let decoded: Progress = serde_json::from_slice(&wire).unwrap();
        assert!(decoded.finished && decoded.ready);
        assert_eq!(decoded.frames, 240000);
        assert_eq!(decoded.error.as_deref(), Some("callback overflow"));
        assert_eq!(decoded.capture.unwrap().dropped_frames, 1024);
    }
    fn fixture(code: &str, stop: bool) -> (Result<record::RecordResult, String>, Duration, u32) {
        let child = Command::new("/usr/bin/python3")
            .args(["-c", code])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let config = Config {
            session: ingestion::Session {
                id: uuid::Uuid::new_v4(),
                run_id: uuid::Uuid::new_v4(),
                source: hear_protocol::AudioSource::Microphone,
                wall_anchor_ms: 0,
                monotonic_anchor_ns: 0,
                generation: 0,
            },
            seconds: None,
        };
        let started = Instant::now();
        let result = monitor(
            child,
            config,
            &AtomicBool::new(stop),
            &AtomicU64::new(0),
            || {},
            |_, _| {},
            Deadlines {
                stop: Duration::from_millis(200),
                start: Duration::from_secs(1),
                progress: Duration::from_millis(250),
                heartbeat: Duration::from_secs(1),
            },
        );
        (result, started.elapsed(), pid)
    }
    #[test]
    fn unresponsive_native_stop_is_killed_and_reaped_by_deadline() {
        let (result, elapsed, pid) = fixture("import time; time.sleep(60)", true);
        assert!(result.err().unwrap().contains("stop timed out"));
        assert!(elapsed < Duration::from_secs(2));
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }
    #[test]
    fn heartbeats_without_durable_audio_do_not_hide_stalled_sealing() {
        let (result, elapsed, _) = fixture(
            "import time,json; message={'ready':True,'frames':0,'blocks':0,'dropped_packets':0,'finished':False,'error':None}\nwhile True: print(json.dumps(message),flush=True); time.sleep(0.04)",
            false,
        );
        assert!(result.err().unwrap().contains("durable progress timed out"));
        assert!(elapsed < Duration::from_secs(2));
    }
    #[test]
    fn a_false_success_frame_cannot_mask_a_failed_child_exit() {
        let (result, _, _) = fixture(
            "import json,sys; print(json.dumps({'ready':True,'frames':48000,'blocks':1,'dropped_packets':0,'finished':True,'error':None}),flush=True); sys.exit(70)",
            false,
        );
        assert!(result.err().unwrap().contains("capture source exited"));
    }
    #[test]
    fn successful_child_drains_final_progress_before_reaping() {
        let (result, _, _) = fixture(
            "import json; print(json.dumps({'ready':True,'frames':48000,'blocks':1,'dropped_packets':0,'finished':True,'error':None}),flush=True)",
            false,
        );
        assert_eq!(result.unwrap().frames, 48000);
    }
}
