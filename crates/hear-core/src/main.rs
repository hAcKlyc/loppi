use fs2::FileExt;
use hear_protocol::{
    AudioSource, CaptureState, ClientKind, Envelope, LogEvent, LogFilter, LogLevel, ModelPhase,
    ModelState, PROTOCOL_VERSION, ProbeStats, Request, Response, ServiceState, data_dir,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
const MAX_CLIENTS: usize = 32;
mod analysis;
mod capture;
mod media;
mod microphone;
mod model_pack;
mod record;
mod retention;
mod system_audio;
mod unilogs;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LaunchMode {
    OnDemand,
    Login,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("hear-core: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|value| value == "probe-mic") {
        let seconds = args
            .get(1)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3);
        if !(1..=20).contains(&seconds) || args.len() > 2 {
            return Err("usage: hear-core probe-mic [1..20 seconds]".to_owned());
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&probe(AudioSource::Microphone, seconds as u8)?)
                .map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    if args.first().is_some_and(|value| value == "probe-system") {
        let seconds = args
            .get(1)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3);
        if !(1..=20).contains(&seconds) || args.len() > 2 {
            return Err("usage: hear-core probe-system [1..20 seconds]".to_owned());
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&probe(AudioSource::System, seconds as u8)?)
                .map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    if !args.is_empty()
        && args.as_slice() != ["serve", "--on-demand"]
        && args.as_slice() != ["serve", "--login"]
    {
        return Err("usage: hear-core [serve --on-demand|serve --login]".to_owned());
    }
    let launch_mode = if args.as_slice() == ["serve", "--on-demand"] {
        LaunchMode::OnDemand
    } else {
        LaunchMode::Login
    };

    let directory = data_dir()?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let Some(lock) = acquire_owner_lock(&directory, launch_mode)? else {
        return Ok(());
    };
    let socket = directory.join("core.sock");
    remove_stale_socket(&socket)?;
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let connection =
        Connection::open(directory.join("hear.sqlite")).map_err(|error| error.to_string())?;
    let mut store = Store::new(connection)?;
    store.recover(&directory)?;
    let login_session = current_login_session()?;
    let capture_after_launch =
        launch_mode == LaunchMode::Login && !store.quit_in_login(login_session)?;
    if launch_mode == LaunchMode::Login && !capture_after_launch {
        store.close_open_gaps(AudioSource::Microphone)?;
        store.close_open_gaps(AudioSource::System)?;
    }
    let run_id = uuid::Uuid::new_v4();
    let logs = LogWorker::start(directory.clone());
    let models_present = model_pack::installed_path(&directory).is_dir();
    let models = Arc::new(Mutex::new(ModelState {
        phase: if models_present {
            ModelPhase::Checking
        } else {
            ModelPhase::Missing
        },
        progress_percent: 0,
    }));
    logs.emit(
        ClientKind::Core,
        uuid::Uuid::new_v4(),
        LogEvent::simple("core", LogLevel::Info, "started"),
    );
    let shared = Arc::new(CoreState {
        store: Mutex::new(store),
        logs,
        launch_mode,
        run_id,
        directory: directory.clone(),
        models,
        microphone_busy: AtomicBool::new(false),
        system_busy: AtomicBool::new(false),
        capture: Mutex::new(CaptureRuntime::default()),
        allow_capture: AtomicBool::new(capture_after_launch),
    });
    if models_present {
        let models = Arc::clone(&shared.models);
        let directory = directory.clone();
        thread::spawn(move || {
            let verified = model_pack::ready(&directory);
            if let Ok(mut state) = models.lock() {
                if matches!(state.phase, ModelPhase::Checking) {
                    state.phase = if verified {
                        ModelPhase::Ready
                    } else {
                        ModelPhase::Missing
                    };
                    state.progress_percent = if verified { 100 } else { 0 };
                }
            }
        });
    }
    let stop = Arc::new(AtomicBool::new(false));
    let capture_thread = {
        let capture_shared = Arc::clone(&shared);
        let capture_stop = Arc::clone(&stop);
        thread::spawn(move || capture_supervisor(capture_shared, capture_stop))
    };
    {
        let analysis_shared = Arc::clone(&shared);
        let analysis_stop = Arc::clone(&stop);
        thread::spawn(move || analysis_loop(analysis_shared, analysis_stop));
    }
    let clients = Arc::new(AtomicUsize::new(0));
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    while !stop.load(Ordering::Acquire) {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        if let Err(error) = verify_peer(&stream) {
            eprintln!("hear-core rejected a local client: {error}");
            continue;
        }
        if clients.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
            clients.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        let clients = Arc::clone(&clients);
        thread::spawn(move || {
            match serve_one(&mut stream, &shared) {
                Ok(true) => stop.store(true, Ordering::Release),
                Ok(false) => {}
                Err(error) => {
                    let mut event = LogEvent::simple("ipc", LogLevel::Warn, "request_failed");
                    event.error_code = Some("invalid_request".to_owned());
                    shared
                        .logs
                        .emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
                    eprintln!("hear-core request failed: {error}");
                }
            }
            clients.fetch_sub(1, Ordering::AcqRel);
        });
    }
    let _ = capture_thread.join();
    drop(listener);
    fs::remove_file(&socket).map_err(|error| error.to_string())?;
    drop(lock);
    Ok(())
}

fn analysis_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let worker_path = match std::env::current_exe()
        .ok()
        .map(|path| path.with_file_name("hear-worker"))
    {
        Some(path) => path,
        None => return,
    };
    let model_dir = model_pack::installed_path(&shared.directory);
    let mut worker: Option<analysis::AnalysisClient> = None;
    while !stop.load(Ordering::Acquire) {
        let models_ready = shared
            .models
            .lock()
            .is_ok_and(|state| matches!(state.phase, ModelPhase::Ready));
        if !models_ready {
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        let pending = shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut store| store.take_pending_block());
        let block = match pending {
            Ok(Some(block)) => block,
            Ok(None) => {
                thread::sleep(Duration::from_millis(500));
                continue;
            }
            Err(_) => {
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let result = (|| -> Result<analysis::WorkResult, String> {
            let pcm = decode_saved_block(&shared.directory, &block)?;
            if worker.is_none() {
                let mut restarted = analysis::AnalysisClient::start(&worker_path, &model_dir)?;
                let context = shared
                    .store
                    .lock()
                    .map_err(|error| error.to_string())?
                    .replay_context(&block)?;
                for prior in context {
                    let prior_pcm = decode_saved_block(&shared.directory, &prior)?;
                    let replay =
                        restarted.analyze(&prior.source, prior.sample_start, &prior_pcm, false)?;
                    if replay.error.is_some() {
                        return Err("worker rejected replay audio".to_owned());
                    }
                }
                worker = Some(restarted);
            }
            worker
                .as_mut()
                .unwrap()
                .analyze(&block.source, block.sample_start, &pcm, block.flush)
        })();
        let success = result
            .and_then(|result| {
                shared
                    .store
                    .lock()
                    .map_err(|error| error.to_string())?
                    .publish_analysis(&block, result)
            })
            .is_ok();
        if !success {
            worker = None;
            if let Ok(mut store) = shared.store.lock() {
                let _ = store.analysis_failed(&block.id);
            }
        }
        let mut event = LogEvent::simple(
            "speech",
            if success {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            if success {
                "coarse_complete"
            } else {
                "coarse_failed"
            },
        );
        event.capture_span_id = uuid::Uuid::parse_str(&block.session_id).ok();
        if !success {
            event.error_code = Some("local_analysis_failed".to_owned());
        }
        shared
            .logs
            .emit(ClientKind::Worker, uuid::Uuid::new_v4(), event);
    }
}

fn decode_saved_block(directory: &Path, block: &PendingBlock) -> Result<Vec<f32>, String> {
    let path = Path::new(&block.media_path);
    let components: Vec<_> = path.components().collect();
    if components.len() != 3
        || components[0].as_os_str() != "media"
        || components[1].as_os_str() != std::ffi::OsStr::new(&block.source)
        || !matches!(block.source.as_str(), "microphone" | "system")
    {
        return Err("invalid media manifest path".to_owned());
    }
    media::decode_block(&directory.join(path), block.frames)
}

struct SourceSlot {
    session_id: uuid::Uuid,
    stop: Arc<AtomicBool>,
    join: thread::JoinHandle<()>,
}

fn start_source(shared: &Arc<CoreState>, source: AudioSource) -> SourceSlot {
    let session_id = uuid::Uuid::new_v4();
    let stop = Arc::new(AtomicBool::new(false));
    let source_stop = Arc::clone(&stop);
    let source_shared = Arc::clone(shared);
    let join = thread::spawn(move || {
        let clock = match record::ClockAnchor::now() {
            Ok(clock) => clock,
            Err(error) => {
                if let Ok(mut capture) = source_shared.capture.lock() {
                    match source {
                        AudioSource::Microphone => capture.microphone_reason = Some(error),
                        AudioSource::System => capture.system_reason = Some(error),
                    }
                }
                match source {
                    AudioSource::Microphone => source_shared
                        .microphone_busy
                        .store(false, Ordering::Release),
                    AudioSource::System => {
                        source_shared.system_busy.store(false, Ordering::Release)
                    }
                }
                return;
            }
        };
        let media_dir = source_shared
            .directory
            .join("media")
            .join(source_name(source));
        let start_result = source_shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut store| {
                store.start_recording_session(session_id, source, source_shared.run_id, clock)
            });
        let result = start_result.and_then(|()| {
            record::record_continuously(
                source,
                &source_stop,
                &media_dir,
                clock,
                |event| {
                    source_shared
                        .store
                        .lock()
                        .map_err(|error| error.to_string())?
                        .insert_record_event(&source_shared.directory, session_id, event)
                },
                || {
                    if let Ok(mut capture) = source_shared.capture.lock() {
                        match source {
                            AudioSource::Microphone => {
                                capture.microphone_active = true;
                                capture.microphone_reason = None;
                            }
                            AudioSource::System => {
                                capture.system_active = true;
                                capture.system_reason = None;
                            }
                        }
                    }
                },
            )
        });
        let completed = result.is_ok();
        if result.as_ref().is_err_and(|error| error == "storage_low") {
            if let Ok(mut store) = source_shared.store.lock() {
                let _ = store.pause_storage();
            }
        }
        let close_result = source_shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut store| store.end_recording_session(session_id, completed));
        if let Ok(mut capture) = source_shared.capture.lock() {
            let reason = if completed && close_result.is_ok() {
                None
            } else {
                Some("audio source stopped; retrying when available".to_owned())
            };
            match source {
                AudioSource::Microphone => {
                    capture.microphone_active = false;
                    capture.microphone_reason = reason;
                }
                AudioSource::System => {
                    capture.system_active = false;
                    capture.system_reason = reason;
                }
            }
        }
        match source {
            AudioSource::Microphone => source_shared
                .microphone_busy
                .store(false, Ordering::Release),
            AudioSource::System => source_shared.system_busy.store(false, Ordering::Release),
        }
        let mut event = LogEvent::simple(
            "capture",
            if completed {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            if completed {
                "source_stopped"
            } else {
                "source_failed"
            },
        );
        event.capture_span_id = Some(session_id);
        if !completed {
            event.error_code = Some("capture_source_failed".to_owned());
        }
        source_shared
            .logs
            .emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
    });
    SourceSlot {
        session_id,
        stop,
        join,
    }
}

fn capture_supervisor(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let mut microphone: Option<SourceSlot> = None;
    let mut system: Option<SourceSlot> = None;
    let mut microphone_retry_at = Instant::now();
    let mut system_retry_at = Instant::now();
    let mut microphone_was_wanted = false;
    let mut system_was_wanted = false;
    let mut previous_wall = SystemTime::now();
    let mut previous_monotonic = Instant::now();
    while !stop.load(Ordering::Acquire) {
        let wall = SystemTime::now();
        let monotonic = Instant::now();
        let slept = wall
            .duration_since(previous_wall)
            .ok()
            .zip(Some(monotonic.duration_since(previous_monotonic)))
            .is_some_and(|(wall_elapsed, monotonic_elapsed)| {
                wall_elapsed > monotonic_elapsed + Duration::from_secs(2)
            });
        previous_wall = wall;
        previous_monotonic = monotonic;

        let state = shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|store| store.state());
        let models_ready = shared
            .models
            .lock()
            .is_ok_and(|models| matches!(models.phase, ModelPhase::Ready));
        let capture_allowed = shared.allow_capture.load(Ordering::Acquire)
            && models_ready
            && state.as_ref().is_ok_and(|state| {
                state.enabled_intent && !state.paused_intent && !state.storage_paused
            });
        let (want_microphone, want_system) = match state {
            Ok(state) => (
                capture_allowed && state.microphone_enabled,
                capture_allowed && state.system_enabled,
            ),
            Err(_) => (false, false),
        };

        for (source, slot, wanted, was_wanted, retry_at, busy) in [
            (
                AudioSource::Microphone,
                &mut microphone,
                want_microphone,
                &mut microphone_was_wanted,
                &mut microphone_retry_at,
                &shared.microphone_busy,
            ),
            (
                AudioSource::System,
                &mut system,
                want_system,
                &mut system_was_wanted,
                &mut system_retry_at,
                &shared.system_busy,
            ),
        ] {
            let should_stop = slept || !wanted;
            if slot
                .as_ref()
                .is_some_and(|current| should_stop || current.join.is_finished())
            {
                let current = slot.take().unwrap();
                current.stop.store(true, Ordering::Release);
                let ended_unexpectedly = !should_stop;
                let _ = current.join.join();
                if slept {
                    if let Ok(mut store) = shared.store.lock() {
                        let _ = store
                            .record_gap_after_session(current.session_id, "sleep_or_clock_gap");
                    }
                }
                if ended_unexpectedly {
                    *retry_at = Instant::now() + Duration::from_secs(30);
                }
            }
            // Close after join: a failing source can open a gap while its
            // thread is winding down, even after pause was requested.
            if !wanted && *was_wanted {
                if let Ok(mut store) = shared.store.lock() {
                    let _ = store.close_open_gaps(source);
                }
            }
            *was_wanted = wanted;
            if wanted
                && slot.is_none()
                && !slept
                && Instant::now() >= *retry_at
                && busy
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                *slot = Some(start_source(&shared, source));
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    for slot in [microphone, system].into_iter().flatten() {
        slot.stop.store(true, Ordering::Release);
        let _ = slot.join.join();
    }
    if let Ok(mut store) = shared.store.lock() {
        let _ = store.close_open_gaps(AudioSource::Microphone);
        let _ = store.close_open_gaps(AudioSource::System);
    }
}

struct CoreState {
    store: Mutex<Store>,
    logs: LogWorker,
    launch_mode: LaunchMode,
    run_id: uuid::Uuid,
    directory: std::path::PathBuf,
    models: Arc<Mutex<ModelState>>,
    microphone_busy: AtomicBool,
    system_busy: AtomicBool,
    capture: Mutex<CaptureRuntime>,
    allow_capture: AtomicBool,
}

#[derive(Default)]
struct CaptureRuntime {
    microphone_active: bool,
    system_active: bool,
    microphone_reason: Option<String>,
    system_reason: Option<String>,
}

impl CoreState {
    fn service_state(&self) -> Result<ServiceState, String> {
        let mut state = self
            .store
            .lock()
            .map_err(|error| error.to_string())?
            .state()?;
        let runtime = self.capture.lock().map_err(|error| error.to_string())?;
        state.microphone_recording = runtime.microphone_active;
        state.system_recording = runtime.system_active;
        state.microphone_reason = runtime.microphone_reason.clone();
        state.system_reason = runtime.system_reason.clone();
        let any_active = runtime.microphone_active || runtime.system_active;
        if !state.microphone_enabled && runtime.microphone_active {
            state.microphone_reason = Some("microphone is stopping".to_owned());
        }
        if !state.system_enabled && runtime.system_active {
            state.system_reason = Some("system audio is stopping".to_owned());
        }
        if any_active && (state.paused_intent || state.storage_paused) {
            state.capture_state = CaptureState::Stopping;
            state.reason = Some("audio sources are stopping".to_owned());
        } else if any_active {
            state.capture_state = CaptureState::Recording;
            state.reason = None;
        } else if state.storage_paused {
            state.capture_state = CaptureState::Paused;
            state.reason =
                Some("recording paused because audio storage reached its safety limit".to_owned());
        } else if state.paused_intent {
            state.capture_state = CaptureState::Paused;
            state.reason = Some("recording was paused by the user".to_owned());
        } else if state.enabled_intent && !self.allow_capture.load(Ordering::Acquire) {
            state.capture_state = CaptureState::Stopped;
            state.reason = Some(if self.launch_mode == LaunchMode::Login {
                "recording was explicitly stopped for this login; start it manually or at the next login"
                    .to_owned()
            } else {
                "core opened for a query; recording resumes at login or with an explicit control"
                    .to_owned()
            });
        } else if state.enabled_intent {
            let busy = self.microphone_busy.load(Ordering::Acquire)
                || self.system_busy.load(Ordering::Acquire);
            state.capture_state = if busy {
                CaptureState::Starting
            } else {
                CaptureState::Stopped
            };
            state.reason = Some(if busy {
                "audio sources are starting".to_owned()
            } else {
                "audio sources are unavailable or retrying".to_owned()
            });
        }
        Ok(state)
    }
}

enum LogCommand {
    Emit(ClientKind, uuid::Uuid, LogEvent),
    Read(
        LogFilter,
        mpsc::Sender<Result<Vec<hear_protocol::LogRecord>, String>>,
    ),
}

#[derive(Clone)]
struct LogWorker {
    sender: SyncSender<LogCommand>,
    dropped: Arc<AtomicU64>,
}

impl LogWorker {
    fn start(directory: std::path::PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1024);
        let dropped = Arc::new(AtomicU64::new(0));
        let pending_drops = Arc::clone(&dropped);
        thread::spawn(move || {
            let mut logs = match unilogs::UniLogs::open(&directory) {
                Ok(logs) => Some(logs),
                Err(error) => {
                    eprintln!("hear-core unilogs unavailable: {error}");
                    None
                }
            };
            for command in receiver {
                let lost = pending_drops.swap(0, Ordering::AcqRel);
                if lost > 0
                    && let Some(logs) = logs.as_mut()
                {
                    let mut event = LogEvent::simple("unilogs", LogLevel::Warn, "dropped_events");
                    event.dropped_count = Some(lost);
                    logs.emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
                }
                match command {
                    LogCommand::Emit(client, id, event) => {
                        if let Some(logs) = logs.as_mut() {
                            logs.emit(client, id, event);
                        }
                    }
                    LogCommand::Read(filter, reply) => {
                        let result = logs
                            .as_ref()
                            .ok_or_else(|| "diagnostic logs are unavailable".to_owned())
                            .and_then(|logs| logs.read(&filter));
                        let _ = reply.send(result);
                    }
                }
            }
        });
        Self { sender, dropped }
    }

    fn emit(&self, client: ClientKind, id: uuid::Uuid, event: LogEvent) {
        if !unilogs::valid_event(&event) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if self
            .sender
            .try_send(LogCommand::Emit(client, id, event))
            .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn read(&self, filter: LogFilter) -> Result<Vec<hear_protocol::LogRecord>, String> {
        let (reply, receiver) = mpsc::channel();
        self.sender
            .try_send(LogCommand::Read(filter, reply))
            .map_err(|_| "diagnostic logs are busy".to_owned())?;
        receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "diagnostic logs did not respond".to_owned())?
    }
}

fn probe(source: AudioSource, seconds: u8) -> Result<ProbeStats, String> {
    if !(1..=20).contains(&seconds) {
        return Err("audio source test duration must be 1 to 20 seconds".to_owned());
    }
    match source {
        AudioSource::Microphone => microphone::Microphone::probe(seconds),
        AudioSource::System => {
            unsafe extern "C" {
                fn hear_system_audio_probe(seconds: u32, peak_millionths_out: *mut u64) -> i64;
            }
            let mut peak_millionths = 0;
            let frames = unsafe { hear_system_audio_probe(seconds as u32, &mut peak_millionths) };
            if frames == -7 {
                return Err("screen and system audio permission unavailable".to_owned());
            }
            if frames < 0 {
                return Err(format!("system audio capture failed at stage {}", -frames));
            }
            Ok(ProbeStats {
                source,
                sample_rate: 48_000,
                channels: 1,
                frames: frames as u64,
                peak: peak_millionths as f32 / 1_000_000.0,
                dropped_packets: 0,
            })
        }
    }
}

fn acquire_lock(directory: &Path) -> Result<Option<File>, String> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("core.lock"))
        .map_err(|error| error.to_string())?;
    match lock.try_lock_exclusive() {
        Ok(()) => Ok(Some(lock)),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn acquire_owner_lock(directory: &Path, mode: LaunchMode) -> Result<Option<File>, String> {
    if mode == LaunchMode::OnDemand {
        return acquire_lock(directory);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(lock) = acquire_lock(directory)? {
            return Ok(Some(lock));
        }
        if let Ok(response) = hear_protocol::send_existing(ClientKind::Core, Request::YieldToLogin)
            && matches!(response, Response::Error { code, .. } if code == "login_owner_present")
        {
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Err("login item could not become the core owner".to_owned());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn remove_stale_socket(socket: &Path) -> Result<(), String> {
    match fs::symlink_metadata(socket) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                return Err("refusing to replace unexpected socket path".to_owned());
            }
            fs::remove_file(socket).map_err(|error| error.to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn verify_peer(stream: &UnixStream) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let mut uid = 0;
    let mut gid = 0;
    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if result != 0 || uid != unsafe { libc::geteuid() } {
        return Err("client user differs from hear-core user".to_owned());
    }
    Ok(())
}

#[repr(C)]
struct AuditInfo {
    user: u32,
    success_mask: u32,
    failure_mask: u32,
    terminal_port: i32,
    terminal_machine: u32,
    session: i32,
}

fn current_login_session() -> Result<i64, String> {
    unsafe extern "C" {
        fn getaudit(info: *mut AuditInfo) -> i32;
    }
    let mut info = AuditInfo {
        user: 0,
        success_mask: 0,
        failure_mask: 0,
        terminal_port: 0,
        terminal_machine: 0,
        session: 0,
    };
    if unsafe { getaudit(&mut info) } != 0 || info.session < 0 {
        return Err("cannot determine the current macOS login session".to_owned());
    }
    Ok(i64::from(info.session))
}

fn serve_one(stream: &mut UnixStream, shared: &CoreState) -> Result<bool, String> {
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| error.to_string())?;
    let input = read_request(stream)?;
    if input.is_empty() {
        return Ok(false);
    }
    if input.len() as u64 > MAX_REQUEST_BYTES {
        return Err("request exceeded protocol limit".to_owned());
    }
    let envelope: Envelope = serde_json::from_slice(&input).map_err(|error| error.to_string())?;
    let explicit_quit = matches!(envelope.request, Request::Quit);
    let quit = explicit_quit
        || matches!(envelope.request, Request::YieldToLogin)
            && shared.launch_mode == LaunchMode::OnDemand;
    let explicit_capture = matches!(
        envelope.request,
        Request::Start { .. } | Request::Resume { .. }
    );
    let request_name = envelope.request.name();
    let response = if envelope.version != PROTOCOL_VERSION {
        Response::Error {
            id: envelope.id,
            code: "protocol_version".to_owned(),
            message: format!("expected protocol version {PROTOCOL_VERSION}"),
        }
    } else {
        match envelope.request {
            Request::Models => Response::Models {
                id: envelope.id,
                state: shared
                    .models
                    .lock()
                    .map_err(|error| error.to_string())?
                    .clone(),
            },
            Request::PrepareModels => {
                let mut state = shared.models.lock().map_err(|error| error.to_string())?;
                if matches!(state.phase, ModelPhase::Missing | ModelPhase::Failed) {
                    state.phase = ModelPhase::Downloading;
                    state.progress_percent = 0;
                    let models = Arc::clone(&shared.models);
                    let directory = shared.directory.clone();
                    let logs = shared.logs.clone();
                    let correlation_id = envelope.id;
                    thread::spawn(move || {
                        let result = model_pack::install(&directory, |progress| {
                            if let Ok(mut state) = models.lock() {
                                state.progress_percent = progress;
                            }
                        });
                        let success = result.is_ok();
                        if let Ok(mut state) = models.lock() {
                            state.phase = if success {
                                ModelPhase::Ready
                            } else {
                                ModelPhase::Failed
                            };
                            state.progress_percent = if success { 100 } else { 0 };
                        }
                        let mut event = LogEvent::simple(
                            "models",
                            if success {
                                LogLevel::Info
                            } else {
                                LogLevel::Error
                            },
                            if success {
                                "installed"
                            } else {
                                "install_failed"
                            },
                        );
                        event.correlation_id = Some(correlation_id);
                        if !success {
                            event.error_code = Some("model_setup_failed".to_owned());
                        }
                        logs.emit(ClientKind::Worker, correlation_id, event);
                    });
                }
                Response::Models {
                    id: envelope.id,
                    state: state.clone(),
                }
            }
            Request::Probe { source, seconds } => match probe(source, seconds) {
                Ok(stats) => Response::ProbeResult {
                    id: envelope.id,
                    stats,
                },
                Err(error) => {
                    eprintln!("hear-core audio source test failed: {error}");
                    Response::Error {
                        id: envelope.id,
                        code: "probe_failed".to_owned(),
                        message: if error.starts_with("microphone permission unavailable")
                            || error == "screen and system audio permission unavailable"
                        {
                            error
                        } else {
                            "audio source test failed; check permissions and device".to_owned()
                        },
                    }
                }
            },
            Request::RecordTest { source, seconds } => {
                let state = shared
                    .store
                    .lock()
                    .map_err(|error| error.to_string())?
                    .state()?;
                let enabled = match source {
                    AudioSource::Microphone => state.microphone_enabled,
                    AudioSource::System => state.system_enabled,
                };
                let flag = match source {
                    AudioSource::Microphone => &shared.microphone_busy,
                    AudioSource::System => &shared.system_busy,
                };
                if state.storage_paused {
                    Response::Error {
                        id: envelope.id,
                        code: "storage_limit".to_owned(),
                        message: "recording is paused by the audio storage limit".to_owned(),
                    }
                } else if !enabled {
                    Response::Error {
                        id: envelope.id,
                        code: "source_disabled".to_owned(),
                        message: "enable this source before testing its recording".to_owned(),
                    }
                } else if flag
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    Response::Error {
                        id: envelope.id,
                        code: "source_busy".to_owned(),
                        message: "this audio source is already being tested".to_owned(),
                    }
                } else {
                    let directory = shared.directory.clone();
                    let source_name = match source {
                        AudioSource::Microphone => "microphone",
                        AudioSource::System => "system",
                    };
                    let media_dir = directory.join("media").join(source_name);
                    let session_id = uuid::Uuid::new_v4();
                    let clock = record::ClockAnchor::now()?;
                    let start_result = shared
                        .store
                        .lock()
                        .map_err(|error| error.to_string())?
                        .start_recording_session(session_id, source, shared.run_id, clock);
                    let capture_result = start_result.and_then(|()| {
                        record::record_short(source, seconds, &media_dir, clock, |event| {
                            shared
                                .store
                                .lock()
                                .map_err(|error| error.to_string())?
                                .insert_record_event(&directory, session_id, event)
                        })
                    });
                    let end_result = shared
                        .store
                        .lock()
                        .map_err(|error| error.to_string())
                        .and_then(|mut store| {
                            store.end_recording_session(session_id, capture_result.is_ok())
                        });
                    flag.store(false, Ordering::Release);
                    match capture_result.and_then(|stats| end_result.map(|()| stats)) {
                        Ok(stats) => Response::RecordTestResult {
                            id: envelope.id,
                            source,
                            blocks: stats.blocks,
                            frames: stats.frames,
                            dropped_packets: stats.dropped_packets,
                        },
                        Err(error) => {
                            eprintln!("hear-core short recording failed: {error}");
                            Response::Error {
                                id: envelope.id,
                                code: "record_test_failed".to_owned(),
                                message: "test recording failed; inspect source permissions and diagnostics".to_owned(),
                            }
                        }
                    }
                }
            }
            Request::Log { event } => {
                shared.logs.emit(envelope.client, envelope.id, event);
                let mut received = LogEvent::simple("ipc", LogLevel::Info, "client_event_received");
                received.correlation_id = Some(envelope.id);
                shared.logs.emit(ClientKind::Core, envelope.id, received);
                Response::Status {
                    id: envelope.id,
                    state: shared.service_state()?,
                }
            }
            Request::Logs { filter } => match shared.logs.read(filter) {
                Ok(entries) => Response::Logs {
                    id: envelope.id,
                    entries,
                },
                Err(error) => {
                    eprintln!("hear-core unilogs unavailable: {error}");
                    Response::Error {
                        id: envelope.id,
                        code: "logs_unavailable".to_owned(),
                        message: "diagnostic logs are unavailable".to_owned(),
                    }
                }
            },
            Request::YieldToLogin => {
                if shared.launch_mode == LaunchMode::Login {
                    Response::Error {
                        id: envelope.id,
                        code: "login_owner_present".to_owned(),
                        message: "login item already owns the core".to_owned(),
                    }
                } else {
                    Response::Status {
                        id: envelope.id,
                        state: shared.service_state()?,
                    }
                }
            }
            request => shared
                .store
                .lock()
                .map_err(|error| error.to_string())?
                .handle(envelope.id, request)?,
        }
    };
    if explicit_capture && matches!(response, Response::Status { .. }) {
        shared
            .store
            .lock()
            .map_err(|error| error.to_string())?
            .clear_quit()?;
        shared.allow_capture.store(true, Ordering::Release);
    }
    if explicit_quit && matches!(response, Response::Status { .. }) {
        shared
            .store
            .lock()
            .map_err(|error| error.to_string())?
            .mark_quit(current_login_session()?)?;
        shared.allow_capture.store(false, Ordering::Release);
    }
    let response = match response {
        Response::Status { id, .. } => Response::Status {
            id,
            state: shared.service_state()?,
        },
        response => response,
    };
    if !matches!(request_name, "status" | "logs" | "log") {
        let mut received = LogEvent::simple("ipc", LogLevel::Info, "request_received");
        received.correlation_id = Some(envelope.id);
        shared.logs.emit(envelope.client, envelope.id, received);
        let mut event = LogEvent::simple("ipc", LogLevel::Info, request_name);
        event.revision = shared
            .store
            .lock()
            .ok()
            .and_then(|store| store.state().ok())
            .map(|state| state.revision);
        event.correlation_id = Some(envelope.id);
        shared.logs.emit(ClientKind::Core, envelope.id, event);
    }
    let body = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
    stream.write_all(&body).map_err(|error| error.to_string())?;
    Ok(quit && !matches!(response, Response::Error { .. }))
}

fn read_request(stream: &mut UnixStream) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut input = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("request read deadline exceeded".to_owned());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(input);
        }
        input.extend_from_slice(&chunk[..count]);
        if input.len() as u64 > MAX_REQUEST_BYTES {
            return Err("request exceeded protocol limit".to_owned());
        }
    }
}

fn now_ms() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?;
    i64::try_from(duration.as_millis()).map_err(|error| error.to_string())
}

fn source_name(source: AudioSource) -> &'static str {
    match source {
        AudioSource::Microphone => "microphone",
        AudioSource::System => "system",
    }
}

struct Store {
    db: Connection,
}

struct PendingBlock {
    id: String,
    session_id: String,
    source: String,
    sample_start: u64,
    frames: usize,
    media_path: String,
    flush: bool,
}

impl Store {
    fn new(db: Connection) -> Result<Self, String> {
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS service_state (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               revision INTEGER NOT NULL,
               enabled_intent INTEGER NOT NULL,
               paused_intent INTEGER NOT NULL,
               microphone_enabled INTEGER NOT NULL,
               system_enabled INTEGER NOT NULL
             );
             INSERT OR IGNORE INTO service_state VALUES (1, 0, 0, 0, 1, 1);
             CREATE TABLE IF NOT EXISTS capture_guard (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               audio_limit_bytes INTEGER NOT NULL,
               pause_reason TEXT
             );
             INSERT OR IGNORE INTO capture_guard VALUES (1, 10737418240, NULL);
             CREATE TABLE IF NOT EXISTS mutation_receipts (
               request_id TEXT PRIMARY KEY,
               fingerprint TEXT NOT NULL,
               state_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS audio_blocks (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL,
               source TEXT NOT NULL CHECK (source IN ('microphone', 'system')),
               started_ms INTEGER NOT NULL,
               ended_ms INTEGER NOT NULL,
               sample_start INTEGER NOT NULL,
               frames INTEGER NOT NULL,
               bytes INTEGER NOT NULL,
               media_path TEXT,
               retention_decision TEXT NOT NULL,
               analysis_state TEXT NOT NULL,
               analysis_attempts INTEGER NOT NULL DEFAULT 0,
               speech_active INTEGER,
               CHECK (ended_ms >= started_ms)
             );
             CREATE INDEX IF NOT EXISTS audio_blocks_source_time ON audio_blocks(source, started_ms);
             CREATE TABLE IF NOT EXISTS coarse_segments (
               id TEXT PRIMARY KEY,
               block_id TEXT NOT NULL,
               session_id TEXT NOT NULL,
               source TEXT NOT NULL,
               started_ms INTEGER NOT NULL,
               ended_ms INTEGER NOT NULL,
               text TEXT NOT NULL,
               model_version TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS coarse_segments_time ON coarse_segments(started_ms);
             CREATE TABLE IF NOT EXISTS recording_sessions (
               id TEXT PRIMARY KEY,
               source TEXT NOT NULL,
               started_ms INTEGER NOT NULL,
               last_confirmed_ms INTEGER NOT NULL,
               ended_ms INTEGER,
               state TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS session_clock (
               session_id TEXT PRIMARY KEY,
               run_id TEXT NOT NULL,
               wall_anchor_ms INTEGER NOT NULL,
               monotonic_anchor_ns INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS media_gaps (
               id TEXT PRIMARY KEY,
               source TEXT NOT NULL,
               started_ms INTEGER NOT NULL,
               ended_ms INTEGER,
               reason TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS orphan_media (
               media_path TEXT PRIMARY KEY,
               bytes INTEGER NOT NULL,
               detected_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS explicit_exit (
               id INTEGER PRIMARY KEY CHECK(id = 1),
               audit_session INTEGER NOT NULL
             );",
        )
        .map_err(|error| error.to_string())?;
        Ok(Self { db })
    }

    fn recover(&mut self, directory: &Path) -> Result<(), String> {
        // The manifest is written before its Opus file. A crash after rename
        // but before the final DB update can therefore be recovered with the
        // original source, session and sample/time coordinates.
        let prepared = {
            let mut statement = self
                .db
                .prepare(
                    "SELECT id, session_id, media_path, frames, ended_ms
                 FROM audio_blocks WHERE analysis_state = 'sealing'",
                )
                .map_err(|error| error.to_string())?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        for (id, session_id, relative, frames, ended_ms) in prepared {
            let path = Path::new(&relative);
            let valid_path = path.components().count() == 3
                && path
                    .components()
                    .next()
                    .is_some_and(|part| part.as_os_str() == "media")
                && path
                    .extension()
                    .is_some_and(|extension| extension == "opus");
            let recovered = if valid_path && (1..=48_000 * 11).contains(&frames) {
                let full = directory.join(path);
                media::decode_block(&full, frames as usize)
                    .ok()
                    .and_then(|_| fs::metadata(full).ok())
                    .map(|metadata| metadata.len() as i64)
            } else {
                None
            };
            if let Some(bytes) = recovered {
                self.db.execute(
                    "UPDATE audio_blocks SET bytes = ?2, analysis_state = 'pending' WHERE id = ?1",
                    params![id, bytes],
                ).map_err(|error| error.to_string())?;
                self.db.execute(
                    "UPDATE recording_sessions SET last_confirmed_ms = max(last_confirmed_ms, ?2)
                     WHERE id = ?1",
                    params![session_id, ended_ms],
                ).map_err(|error| error.to_string())?;
            } else {
                self.db
                    .execute(
                        "UPDATE audio_blocks SET media_path = NULL,
                      retention_decision = 'interrupted_seal_v1', analysis_state = 'failed'
                     WHERE id = ?1",
                        params![id],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        let current_ms = now_ms()?;
        let recovery = self.db.transaction().map_err(|error| error.to_string())?;
        recovery
            .execute(
                "INSERT INTO media_gaps (id, source, started_ms, ended_ms, reason)
             SELECT id, source, last_confirmed_ms,
                    NULL, 'interrupted_tail'
             FROM recording_sessions WHERE state = 'open'",
                [],
            )
            .map_err(|error| error.to_string())?;
        recovery.execute(
            "UPDATE recording_sessions SET ended_ms = ?1, state = 'interrupted' WHERE state = 'open'",
            params![current_ms],
        ).map_err(|error| error.to_string())?;
        recovery.execute(
            "UPDATE audio_blocks SET analysis_state = 'pending' WHERE analysis_state = 'processing'",
            [],
        ).map_err(|error| error.to_string())?;
        recovery.commit().map_err(|error| error.to_string())?;
        self.scan_orphan_media(directory)?;
        if !self.storage_available(directory)? {
            self.pause_storage()?;
        }
        let intent = self.state()?;
        if !intent.enabled_intent || intent.paused_intent || intent.storage_paused {
            self.close_open_gaps(AudioSource::Microphone)?;
            self.close_open_gaps(AudioSource::System)?;
        } else {
            if !intent.microphone_enabled {
                self.close_open_gaps(AudioSource::Microphone)?;
            }
            if !intent.system_enabled {
                self.close_open_gaps(AudioSource::System)?;
            }
        }
        Ok(())
    }

    fn scan_orphan_media(&mut self, directory: &Path) -> Result<(), String> {
        self.db
            .execute("DELETE FROM orphan_media", [])
            .map_err(|error| error.to_string())?;
        for source in ["microphone", "system"] {
            let media_dir = directory.join("media").join(source);
            let entries = match fs::read_dir(&media_dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.to_string()),
            };
            for entry in entries {
                let entry = entry.map_err(|error| error.to_string())?;
                if !entry
                    .file_type()
                    .map_err(|error| error.to_string())?
                    .is_file()
                {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some((id, extension)) = name.rsplit_once('.') else {
                    continue;
                };
                if !matches!(extension, "opus" | "pending") || uuid::Uuid::parse_str(id).is_err() {
                    continue;
                }
                let relative = format!("media/{source}/{name}");
                let known: bool = self
                    .db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path = ?1)",
                        params![relative],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                if !known {
                    let bytes = entry.metadata().map_err(|error| error.to_string())?.len();
                    self.db
                        .execute(
                            "INSERT OR REPLACE INTO orphan_media (media_path, bytes, detected_ms)
                         VALUES (?1, ?2, ?3)",
                            params![relative, bytes as i64, now_ms()?],
                        )
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(())
    }

    fn state(&self) -> Result<ServiceState, String> {
        read_state(&self.db)
    }

    fn quit_in_login(&self, session: i64) -> Result<bool, String> {
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM explicit_exit WHERE id = 1 AND audit_session = ?1)",
                params![session],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
    }

    fn mark_quit(&mut self, session: i64) -> Result<(), String> {
        self.db
            .execute(
                "INSERT INTO explicit_exit (id, audit_session) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET audit_session = excluded.audit_session",
                params![session],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn clear_quit(&mut self) -> Result<(), String> {
        self.db
            .execute("DELETE FROM explicit_exit WHERE id = 1", [])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn start_recording_session(
        &mut self,
        id: uuid::Uuid,
        source: AudioSource,
        run_id: uuid::Uuid,
        clock: record::ClockAnchor,
    ) -> Result<(), String> {
        let source = source_name(source);
        let at = now_ms()?;
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "UPDATE media_gaps SET ended_ms = max(started_ms, ?2)
             WHERE source = ?1 AND ended_ms IS NULL",
            params![source, at],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO recording_sessions (id, source, started_ms, last_confirmed_ms, state)
             VALUES (?1, ?2, ?3, ?3, 'open')",
            params![id.to_string(), source, at],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO session_clock (session_id, run_id, wall_anchor_ms, monotonic_anchor_ns)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id.to_string(),
                run_id.to_string(),
                clock.wall_ms,
                clock.monotonic_ns
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn end_recording_session(&mut self, id: uuid::Uuid, completed: bool) -> Result<(), String> {
        let at = now_ms()?;
        if !completed {
            self.db
                .execute(
                    "INSERT INTO media_gaps (id, source, started_ms, ended_ms, reason)
                 SELECT id, source, last_confirmed_ms, NULL, 'source_failure'
                 FROM recording_sessions WHERE id = ?1 AND state = 'open'",
                    params![id.to_string()],
                )
                .map_err(|error| error.to_string())?;
        }
        self.db.execute(
            "UPDATE recording_sessions SET ended_ms = ?2, state = ?3 WHERE id = ?1 AND state = 'open'",
            params![id.to_string(), at, if completed { "closed" } else { "failed" }],
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn record_gap_after_session(
        &mut self,
        session_id: uuid::Uuid,
        reason: &str,
    ) -> Result<(), String> {
        let at = now_ms()?;
        self.db
            .execute(
                "INSERT INTO media_gaps (id, source, started_ms, ended_ms, reason)
             SELECT ?1, source, last_confirmed_ms, NULL, ?3
             FROM recording_sessions WHERE id = ?2 AND last_confirmed_ms < ?4",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    session_id.to_string(),
                    reason,
                    at
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn close_open_gaps(&mut self, source: AudioSource) -> Result<(), String> {
        let at = now_ms()?;
        self.db
            .execute(
                "UPDATE media_gaps SET ended_ms = max(started_ms, ?2)
             WHERE source = ?1 AND ended_ms IS NULL",
                params![source_name(source), at],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn insert_record_event(
        &mut self,
        directory: &Path,
        session_id: uuid::Uuid,
        event: record::RecordEvent,
    ) -> Result<(), String> {
        match event {
            record::RecordEvent::Block(block) => self.insert_block(directory, session_id, block),
            record::RecordEvent::Gap(gap) => {
                self.db
                    .execute(
                        "INSERT INTO media_gaps (id, source, started_ms, ended_ms, reason)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            uuid::Uuid::new_v4().to_string(),
                            source_name(gap.source),
                            gap.started_ms,
                            gap.ended_ms,
                            gap.reason
                        ],
                    )
                    .map_err(|error| error.to_string())?;
                Ok(())
            }
        }
    }

    fn insert_block(
        &mut self,
        directory: &Path,
        session_id: uuid::Uuid,
        block: record::BlockRecord,
    ) -> Result<(), String> {
        if block
            .media
            .as_ref()
            .is_some_and(|media| media.samples != block.frames as u64)
        {
            return Err("sealed media sample count differs from timeline metadata".to_owned());
        }
        if block.preparing {
            let relative = format!("media/{}/{}.opus", source_name(block.source), block.id);
            self.db
                .execute(
                    "INSERT INTO audio_blocks
                 (id, session_id, source, started_ms, ended_ms, sample_start, frames, bytes,
                  media_path, retention_decision, analysis_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, 'sealing')",
                    params![
                        block.id.to_string(),
                        session_id.to_string(),
                        source_name(block.source),
                        block.started_ms,
                        block.ended_ms,
                        block.sample_start as i64,
                        block.frames as i64,
                        relative,
                        block.decision,
                    ],
                )
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        let (media_path, bytes) = match block.media {
            Some(media) => {
                let relative = media
                    .path
                    .strip_prefix(directory)
                    .map_err(|_| "sealed media was outside the application directory".to_owned())?;
                (Some(relative.to_string_lossy().into_owned()), media.bytes)
            }
            None => (None, 0),
        };
        let source = source_name(block.source);
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        if block.decision == "discard_digital_silence_v1" {
            tx.execute(
                "INSERT INTO audio_blocks
                 (id, session_id, source, started_ms, ended_ms, sample_start, frames, bytes,
                  media_path, retention_decision, analysis_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, NULL, ?8, 'not_needed')",
                params![
                    block.id.to_string(),
                    session_id.to_string(),
                    source,
                    block.started_ms,
                    block.ended_ms,
                    block.sample_start as i64,
                    block.frames as i64,
                    block.decision,
                ],
            )
            .map_err(|error| error.to_string())?;
        } else {
            let changed = tx.execute(
                "UPDATE audio_blocks SET bytes = ?2, analysis_state = 'pending'
                 WHERE id = ?1 AND session_id = ?3 AND media_path = ?4 AND analysis_state = 'sealing'",
                params![block.id.to_string(), bytes as i64, session_id.to_string(), media_path],
            ).map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("sealed media lacked its prepared manifest".to_owned());
            }
        }
        tx.execute(
            "UPDATE recording_sessions SET last_confirmed_ms = max(last_confirmed_ms, ?2)
             WHERE id = ?1 AND state = 'open'",
            params![session_id.to_string(), block.ended_ms],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        if !matches!(self.storage_available(directory), Ok(true)) {
            self.pause_storage()?;
        }
        Ok(())
    }

    fn storage_available(&self, directory: &Path) -> Result<bool, String> {
        let (used, limit): (i64, i64) = self
            .db
            .query_row(
                "SELECT (SELECT coalesce(sum(bytes), 0) FROM audio_blocks) +
                    (SELECT coalesce(sum(bytes), 0) FROM orphan_media), audio_limit_bytes
             FROM capture_guard WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        let free = fs2::available_space(directory).map_err(|error| error.to_string())?;
        Ok(used < limit && free > 1_073_741_824)
    }

    fn pause_storage(&mut self) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE capture_guard SET pause_reason = 'storage_limit' WHERE id = 1",
                [],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn take_pending_block(&mut self) -> Result<Option<PendingBlock>, String> {
        let idle_before_ms = now_ms()?.saturating_sub(8_000);
        let block = self
            .db
            .query_row(
                "SELECT b.id, b.session_id, b.source, b.sample_start, b.frames, b.media_path,
                    (s.state != 'open' OR b.ended_ms <= ?1 OR EXISTS (
                      SELECT 1 FROM audio_blocks later
                      WHERE later.session_id = b.session_id
                        AND later.sample_start > b.sample_start
                        AND later.sample_start != b.sample_start + b.frames
                    )) AND NOT EXISTS (
                      SELECT 1 FROM audio_blocks adjacent
                      WHERE adjacent.session_id = b.session_id
                        AND adjacent.sample_start = b.sample_start + b.frames
                        AND adjacent.media_path IS NOT NULL
                    )
             FROM audio_blocks b JOIN recording_sessions s ON s.id = b.session_id
             WHERE b.analysis_state = 'pending' AND b.analysis_attempts < 3
                   AND b.media_path IS NOT NULL
                   AND (s.state != 'open' OR b.ended_ms <= ?1 OR EXISTS (
                     SELECT 1 FROM audio_blocks later
                     WHERE later.session_id = b.session_id AND later.sample_start > b.sample_start
                   ))
             ORDER BY b.started_ms, b.id LIMIT 1",
                params![idle_before_ms],
                |row| {
                    Ok(PendingBlock {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        source: row.get(2)?,
                        sample_start: row.get::<_, i64>(3)? as u64,
                        frames: row.get::<_, i64>(4)? as usize,
                        media_path: row.get(5)?,
                        flush: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(block) = &block {
            self.db
                .execute(
                    "UPDATE audio_blocks SET analysis_state = 'processing',
                 analysis_attempts = analysis_attempts + 1 WHERE id = ?1",
                    params![block.id],
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(block)
    }

    fn replay_context(&self, current: &PendingBlock) -> Result<Vec<PendingBlock>, String> {
        let mut statement = self
            .db
            .prepare(
                "SELECT id, session_id, source, sample_start, frames, media_path,
                    coalesce(speech_active, 0)
             FROM audio_blocks
             WHERE session_id = ?1 AND sample_start < ?2
               AND analysis_state = 'complete' AND media_path IS NOT NULL
             ORDER BY sample_start DESC LIMIT 6",
            )
            .map_err(|error| error.to_string())?;
        let candidates = statement
            .query_map(
                params![current.session_id, current.sample_start as i64],
                |row| {
                    Ok((
                        PendingBlock {
                            id: row.get(0)?,
                            session_id: row.get(1)?,
                            source: row.get(2)?,
                            sample_start: row.get::<_, i64>(3)? as u64,
                            frames: row.get::<_, i64>(4)? as usize,
                            media_path: row.get(5)?,
                            flush: false,
                        },
                        row.get::<_, bool>(6)?,
                    ))
                },
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut cursor = current.sample_start;
        let mut replay = Vec::new();
        for (prior, speech_active) in candidates {
            if !speech_active || prior.sample_start + prior.frames as u64 != cursor {
                break;
            }
            cursor = prior.sample_start;
            replay.push(prior);
        }
        replay.reverse();
        Ok(replay)
    }

    fn analysis_failed(&mut self, block_id: &str) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE audio_blocks SET analysis_state = CASE
               WHEN analysis_attempts >= 3 THEN 'failed' ELSE 'pending' END
             WHERE id = ?1 AND analysis_state = 'processing'",
                params![block_id],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn publish_analysis(
        &mut self,
        block: &PendingBlock,
        result: analysis::WorkResult,
    ) -> Result<(), String> {
        if result.error.is_some() {
            return Err("local speech worker rejected audio".to_owned());
        }
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        for segment in result.segments {
            if segment.end_sample <= segment.start_sample {
                return Err("local speech worker returned an invalid time range".to_owned());
            }
            let started_ms = map_sample(&tx, &block.session_id, segment.start_sample)?;
            let ended_ms = map_sample(&tx, &block.session_id, segment.end_sample)?;
            tx.execute(
                "INSERT INTO coarse_segments
                 (id, block_id, session_id, source, started_ms, ended_ms, text, model_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'sensevoice-int8-2024-07-17')",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    block.id,
                    block.session_id,
                    block.source,
                    started_ms,
                    ended_ms,
                    segment.text
                ],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.execute(
            "UPDATE audio_blocks SET analysis_state = 'complete', speech_active = ?2
             WHERE id = ?1 AND analysis_state = 'processing'",
            params![block.id, result.speech_active],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
    }

    fn handle(&mut self, id: uuid::Uuid, request: Request) -> Result<Response, String> {
        let fingerprint = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        match request {
            Request::Status | Request::Quit => Ok(Response::Status {
                id,
                state: self.state()?,
            }),
            Request::Start { .. } => Ok(Response::Error {
                id,
                code: "setup_incomplete".to_owned(),
                message: "recording cannot start until local speech models and both cloud providers are ready".to_owned(),
            }),
            Request::Pause { expected_revision } => self.mutate(
                id,
                expected_revision,
                fingerprint,
                |tx| tx.execute("UPDATE service_state SET paused_intent = 1, revision = revision + 1 WHERE id = 1", []),
            ),
            Request::Resume { expected_revision } => {
                if self.state()?.storage_paused
                    && !self.storage_available(&hear_protocol::data_dir()?)?
                {
                    return Ok(Response::Error {
                        id,
                        code: "storage_limit".to_owned(),
                        message: "free audio space before resuming recording".to_owned(),
                    });
                }
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    tx.execute("UPDATE capture_guard SET pause_reason = NULL WHERE id = 1", [])?;
                    tx.execute("UPDATE service_state SET paused_intent = 0, revision = revision + 1 WHERE id = 1", [])
                })
            }
            Request::SetSource { source, enabled, expected_revision } => {
                let column = match source {
                    AudioSource::Microphone => "microphone_enabled",
                    AudioSource::System => "system_enabled",
                };
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    tx.execute(
                        &format!("UPDATE service_state SET {column} = ?1, revision = revision + 1 WHERE id = 1"),
                        params![enabled],
                    )
                })
            }
            Request::Models
            | Request::PrepareModels
            | Request::Probe { .. }
            | Request::RecordTest { .. }
            | Request::Log { .. }
            | Request::Logs { .. }
            | Request::YieldToLogin => {
                unreachable!("handled by IPC dispatcher")
            }
        }
    }

    fn mutate(
        &mut self,
        id: uuid::Uuid,
        expected_revision: i64,
        fingerprint: String,
        change: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<usize>,
    ) -> Result<Response, String> {
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        let id_text = id.to_string();
        let receipt: Option<(String, String)> = tx
            .query_row(
                "SELECT fingerprint, state_json FROM mutation_receipts WHERE request_id = ?1",
                params![id_text],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((prior_fingerprint, state_json)) = receipt {
            if prior_fingerprint != fingerprint {
                return Ok(Response::Error {
                    id,
                    code: "idempotency_conflict".to_owned(),
                    message: "request ID was already used for a different mutation".to_owned(),
                });
            }
            let state = serde_json::from_str(&state_json).map_err(|error| error.to_string())?;
            return Ok(Response::Status { id, state });
        }
        let current = read_state(&tx)?;
        if current.revision != expected_revision {
            return Ok(Response::Error {
                id,
                code: "revision_conflict".to_owned(),
                message: format!(
                    "expected revision {expected_revision}, current revision {}",
                    current.revision
                ),
            });
        }
        change(&tx).map_err(|error| error.to_string())?;
        let state = read_state(&tx)?;
        let state_json = serde_json::to_string(&state).map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO mutation_receipts (request_id, fingerprint, state_json) VALUES (?1, ?2, ?3)",
            params![id_text, fingerprint, state_json],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(Response::Status { id, state })
    }
}

fn read_state(db: &Connection) -> Result<ServiceState, String> {
    db.query_row(
        "SELECT revision, enabled_intent, paused_intent, microphone_enabled, system_enabled,
                (SELECT pause_reason IS NOT NULL FROM capture_guard WHERE id = 1)
         FROM service_state WHERE id = 1",
        [],
        |row| {
            Ok(ServiceState {
                revision: row.get(0)?,
                enabled_intent: row.get(1)?,
                paused_intent: row.get(2)?,
                storage_paused: row.get(5)?,
                microphone_enabled: row.get(3)?,
                system_enabled: row.get(4)?,
                capture_state: CaptureState::NotReady,
                reason: Some("local speech models and both cloud providers must be configured before recording".to_owned()),
                microphone_recording: false,
                system_recording: false,
                microphone_reason: None,
                system_reason: None,
            })
        },
    )
    .map_err(|error| error.to_string())
}

fn map_sample(db: &Connection, session_id: &str, sample_16k: u64) -> Result<i64, String> {
    let sample_48k =
        i64::try_from(sample_16k.saturating_mul(3)).map_err(|error| error.to_string())?;
    let mapped: Option<(i64, i64)> = db
        .query_row(
            "SELECT started_ms, sample_start FROM audio_blocks
         WHERE session_id = ?1 AND sample_start <= ?2
               AND ?2 <= sample_start + frames
         ORDER BY sample_start DESC LIMIT 1",
            params![session_id, sample_48k],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    mapped
        .map(|(wall_ms, sample_start)| wall_ms + (sample_48k - sample_start) * 1000 / 48_000)
        .ok_or_else(|| "speech segment could not map to saved audio".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn query_does_not_enable_recording() {
        let db = Connection::open_in_memory().unwrap();
        let mut store = Store::new(db).unwrap();
        let id = Uuid::new_v4();
        let response = store.handle(id, Request::Status).unwrap();
        let Response::Status { state, .. } = response else {
            panic!("expected status")
        };
        assert!(!state.enabled_intent);
        assert!(matches!(state.capture_state, CaptureState::NotReady));
        assert_eq!(state.revision, 0);
    }

    #[test]
    fn recovery_publishes_a_sealed_block_left_between_file_and_manifest_commit() {
        let directory = tempfile::tempdir().unwrap();
        let db_path = directory.path().join("hear.sqlite");
        let source = AudioSource::Microphone;
        let session = Uuid::new_v4();
        let block_id = Uuid::new_v4();
        let media_dir = directory.path().join("media/microphone");
        let mut store = Store::new(Connection::open(&db_path).unwrap()).unwrap();
        store
            .start_recording_session(
                session,
                source,
                Uuid::new_v4(),
                record::ClockAnchor::now().unwrap(),
            )
            .unwrap();
        store
            .insert_block(
                directory.path(),
                session,
                record::BlockRecord {
                    id: block_id,
                    source,
                    started_ms: 1_000,
                    ended_ms: 2_000,
                    media: None,
                    frames: 48_000,
                    sample_start: 0,
                    decision: "pending_conservative_v1",
                    preparing: true,
                },
            )
            .unwrap();
        let mut file = media::OpusBlock::create(&media_dir, block_id).unwrap();
        file.push(&vec![0.1; 48_000]).unwrap();
        file.seal().unwrap();
        drop(store);

        let mut reopened = Store::new(Connection::open(&db_path).unwrap()).unwrap();
        reopened.recover(directory.path()).unwrap();
        let (analysis_state, bytes, confirmed): (String, i64, i64) = reopened
            .db
            .query_row(
                "SELECT b.analysis_state, b.bytes, s.last_confirmed_ms
             FROM audio_blocks b JOIN recording_sessions s ON s.id = b.session_id WHERE b.id = ?1",
                params![block_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(analysis_state, "pending");
        assert!(bytes > 0);
        assert!(confirmed >= 2_000);
    }

    #[test]
    fn worker_restart_replays_only_the_contiguous_active_speech_tail() {
        let store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        for (id, start, active) in [("a", 0_i64, 0_i64), ("b", 48_000, 1)] {
            store
                .db
                .execute(
                    "INSERT INTO audio_blocks
                 (id, session_id, source, started_ms, ended_ms, sample_start, frames,
                  bytes, media_path, retention_decision, analysis_state, speech_active)
                 VALUES (?1, 'session', 'system', ?2, ?3, ?4, 48000,
                         100, ?5, 'pending_conservative_v1', 'complete', ?6)",
                    params![
                        id,
                        start / 48,
                        start / 48 + 1_000,
                        start,
                        format!("media/system/{id}.opus"),
                        active
                    ],
                )
                .unwrap();
        }
        let current = PendingBlock {
            id: "c".to_owned(),
            session_id: "session".to_owned(),
            source: "system".to_owned(),
            sample_start: 96_000,
            frames: 48_000,
            media_path: "media/system/c.opus".to_owned(),
            flush: true,
        };
        let replay = store.replay_context(&current).unwrap();
        assert_eq!(
            replay
                .iter()
                .map(|block| block.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
        let after_gap = PendingBlock {
            sample_start: 120_000,
            ..current
        };
        assert!(store.replay_context(&after_gap).unwrap().is_empty());
    }

    #[test]
    fn interrupted_media_is_preserved_and_counts_toward_the_audio_limit() {
        let directory = tempfile::tempdir().unwrap();
        let media_dir = directory.path().join("media/system");
        fs::create_dir_all(&media_dir).unwrap();
        let orphan = format!("{}.pending", Uuid::new_v4());
        fs::write(media_dir.join(&orphan), vec![7; 200]).unwrap();
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store.recover(directory.path()).unwrap();
        let counted: i64 = store
            .db
            .query_row("SELECT sum(bytes) FROM orphan_media", [], |row| row.get(0))
            .unwrap();
        assert_eq!(counted, 200);
        store
            .db
            .execute(
                "UPDATE capture_guard SET audio_limit_bytes = 100 WHERE id = 1",
                [],
            )
            .unwrap();
        assert!(!store.storage_available(directory.path()).unwrap());
        assert!(media_dir.join(orphan).is_file());
    }

    #[test]
    fn failed_source_gap_stays_open_through_retry_and_closes_at_restart() {
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let first = Uuid::new_v4();
        store
            .start_recording_session(
                first,
                AudioSource::Microphone,
                Uuid::new_v4(),
                record::ClockAnchor::now().unwrap(),
            )
            .unwrap();
        store.end_recording_session(first, false).unwrap();
        let open: Option<i64> = store
            .db
            .query_row(
                "SELECT ended_ms FROM media_gaps WHERE id = ?1",
                params![first.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(open, None);
        let second = Uuid::new_v4();
        store
            .start_recording_session(
                second,
                AudioSource::Microphone,
                Uuid::new_v4(),
                record::ClockAnchor::now().unwrap(),
            )
            .unwrap();
        let closed: i64 = store
            .db
            .query_row(
                "SELECT ended_ms FROM media_gaps WHERE id = ?1",
                params![first.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let restarted: i64 = store
            .db
            .query_row(
                "SELECT started_ms FROM recording_sessions WHERE id = ?1",
                params![second.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(closed, restarted);
    }

    #[test]
    fn explicit_quit_applies_only_to_the_current_login_session() {
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let login = current_login_session().unwrap();
        store.mark_quit(login).unwrap();
        assert!(store.quit_in_login(login).unwrap());
        assert!(!store.quit_in_login(login + 1).unwrap());
        store.clear_quit().unwrap();
        assert!(!store.quit_in_login(login).unwrap());
    }

    #[test]
    fn orphan_over_limit_pauses_at_recovery_before_capture_starts() {
        let directory = tempfile::tempdir().unwrap();
        let media_dir = directory.path().join("media/microphone");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(
            media_dir.join(format!("{}.opus", Uuid::new_v4())),
            vec![1; 200],
        )
        .unwrap();
        let db_path = directory.path().join("hear.sqlite");
        let store = Store::new(Connection::open(&db_path).unwrap()).unwrap();
        store
            .db
            .execute(
                "UPDATE capture_guard SET audio_limit_bytes = 100 WHERE id = 1",
                [],
            )
            .unwrap();
        drop(store);
        let mut store = Store::new(Connection::open(&db_path).unwrap()).unwrap();
        store.recover(directory.path()).unwrap();
        assert!(store.state().unwrap().storage_paused);
    }

    #[test]
    fn reaching_audio_cap_pauses_further_capture_after_sealing_the_current_block() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store
            .db
            .execute(
                "UPDATE capture_guard SET audio_limit_bytes = 1 WHERE id = 1",
                [],
            )
            .unwrap();
        let session = Uuid::new_v4();
        let block_id = Uuid::new_v4();
        store
            .start_recording_session(
                session,
                AudioSource::Microphone,
                Uuid::new_v4(),
                record::ClockAnchor::now().unwrap(),
            )
            .unwrap();
        let record = |media, preparing| record::BlockRecord {
            id: block_id,
            source: AudioSource::Microphone,
            started_ms: 1_000,
            ended_ms: 2_000,
            sample_start: 0,
            frames: 48_000,
            media,
            decision: "pending_conservative_v1",
            preparing,
        };
        store
            .insert_block(directory.path(), session, record(None, true))
            .unwrap();
        let mut file =
            media::OpusBlock::create(&directory.path().join("media/microphone"), block_id).unwrap();
        file.push(&vec![0.1; 48_000]).unwrap();
        store
            .insert_block(
                directory.path(),
                session,
                record(Some(file.seal().unwrap()), false),
            )
            .unwrap();
        assert!(store.state().unwrap().storage_paused);
        let state: String = store
            .db
            .query_row(
                "SELECT analysis_state FROM audio_blocks WHERE id = ?1",
                params![block_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "pending");
    }

    #[test]
    fn start_rejects_incomplete_setup_without_changing_intent() {
        let db = Connection::open_in_memory().unwrap();
        let mut store = Store::new(db).unwrap();
        let response = store
            .handle(
                Uuid::new_v4(),
                Request::Start {
                    expected_revision: 0,
                },
            )
            .unwrap();
        assert!(matches!(response, Response::Error { code, .. } if code == "setup_incomplete"));
        assert!(!store.state().unwrap().enabled_intent);
    }

    #[test]
    fn source_preference_persists() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let mut store = Store::new(Connection::open(&path).unwrap()).unwrap();
        store
            .handle(
                Uuid::new_v4(),
                Request::SetSource {
                    source: AudioSource::Microphone,
                    enabled: false,
                    expected_revision: 0,
                },
            )
            .unwrap();
        drop(store);
        let reopened = Store::new(Connection::open(&path).unwrap()).unwrap();
        assert!(!reopened.state().unwrap().microphone_enabled);
    }

    #[test]
    fn pause_can_be_cleared_before_setup_without_starting_recording() {
        let db = Connection::open_in_memory().unwrap();
        let mut store = Store::new(db).unwrap();
        store
            .handle(
                Uuid::new_v4(),
                Request::Pause {
                    expected_revision: 0,
                },
            )
            .unwrap();
        let response = store
            .handle(
                Uuid::new_v4(),
                Request::Resume {
                    expected_revision: 1,
                },
            )
            .unwrap();
        let Response::Status { state, .. } = response else {
            panic!("expected status")
        };
        assert!(!state.paused_intent);
        assert!(!state.enabled_intent);
        assert!(matches!(state.capture_state, CaptureState::NotReady));
    }

    #[test]
    fn mutation_revision_and_duplicate_id_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let id = Uuid::new_v4();
        let mut store = Store::new(Connection::open(&path).unwrap()).unwrap();
        store
            .handle(
                id,
                Request::Pause {
                    expected_revision: 0,
                },
            )
            .unwrap();
        let stale = store
            .handle(
                Uuid::new_v4(),
                Request::Resume {
                    expected_revision: 0,
                },
            )
            .unwrap();
        assert!(matches!(stale, Response::Error { code, .. } if code == "revision_conflict"));
        drop(store);

        let mut reopened = Store::new(Connection::open(&path).unwrap()).unwrap();
        let replay = reopened
            .handle(
                id,
                Request::Pause {
                    expected_revision: 0,
                },
            )
            .unwrap();
        assert!(matches!(replay, Response::Status { state, .. } if state.revision == 1));
        assert_eq!(reopened.state().unwrap().revision, 1);
        let reused = reopened
            .handle(
                id,
                Request::Resume {
                    expected_revision: 1,
                },
            )
            .unwrap();
        assert!(matches!(reused, Response::Error { code, .. } if code == "idempotency_conflict"));
        assert!(reopened.state().unwrap().paused_intent);
    }

    #[test]
    fn logging_queue_drops_oversized_and_full_events_without_blocking() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let logs = LogWorker {
            sender,
            dropped: Arc::new(AtomicU64::new(0)),
        };
        logs.emit(
            ClientKind::Core,
            Uuid::new_v4(),
            LogEvent::simple("ipc", LogLevel::Info, "first"),
        );
        logs.emit(
            ClientKind::Core,
            Uuid::new_v4(),
            LogEvent::simple("ipc", LogLevel::Info, "second"),
        );
        logs.emit(
            ClientKind::Core,
            Uuid::new_v4(),
            LogEvent::simple("ipc", LogLevel::Info, &"x".repeat(1024 * 1024)),
        );
        assert_eq!(logs.dropped.load(Ordering::Relaxed), 2);
    }
}
