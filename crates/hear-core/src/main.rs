use fs2::FileExt;
use hear_protocol::{
    AudioSource, CaptureState, ClientKind, Envelope, LogEvent, LogFilter, LogLevel, ModelPhase,
    ModelState, PROTOCOL_VERSION, ProbeStats, Request, Response, ServiceState, data_dir,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
const MAX_CLIENTS: usize = 32;
mod analysis;
mod archive;
mod capture;
mod cleanup;
mod cloud;
mod events;
mod knowledge;
mod media;
mod microphone;
mod model_pack;
mod moments;
mod playback;
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
            serde_json::to_string_pretty(&probe(AudioSource::Microphone, seconds as u8, true)?)
                .map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    if args.first().is_some_and(|value| value == "probe-system") {
        let seconds = args
            .get(1)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3);
        let parent_pid = if args.len() == 4 && args[2] == "--parent-pid" {
            Some(
                args[3]
                    .parse::<i32>()
                    .map_err(|_| "invalid probe parent pid")?,
            )
        } else if args.len() <= 2 {
            None
        } else {
            return Err("usage: hear-core probe-system [1..20 seconds]".to_owned());
        };
        if !(1..=20).contains(&seconds) || parent_pid.is_some_and(|pid| pid <= 1) {
            return Err("usage: hear-core probe-system [1..20 seconds]".to_owned());
        }
        if let Some(parent_pid) = parent_pid {
            if unsafe { libc::getppid() } != parent_pid {
                std::process::exit(70);
            }
            thread::spawn(move || {
                while unsafe { libc::getppid() } == parent_pid {
                    thread::sleep(Duration::from_millis(200));
                }
                // HAL can remain blocked after the caller exits. The child
                // must not survive its authorization/probe owner.
                std::process::exit(70);
            });
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&probe(AudioSource::System, seconds as u8, true)?)
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
    events::recover_asr_staging(&directory);
    archive::recover(&store.db)?;
    archive::cleanup_spool(&directory)?;
    cleanup::drain_deletions(&store.db, &directory, now_ms()?)?;
    moments::recover(&store.db, &directory)?;
    cleanup::recover_staging(&store.db, &directory)?;
    store.recover(&directory)?;
    events::recover_tasks(&mut store.db, now_ms()?)?;
    knowledge::recover_voice(&store.db)?;
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
    let speaker_present = model_pack::speaker_model_path(&directory).is_file();
    let models = Arc::new(Mutex::new(ModelState {
        phase: if models_present {
            ModelPhase::Checking
        } else {
            ModelPhase::Missing
        },
        progress_percent: 0,
        speaker_phase: if speaker_present {
            ModelPhase::Checking
        } else {
            ModelPhase::Missing
        },
    }));
    logs.emit(
        ClientKind::Core,
        uuid::Uuid::new_v4(),
        LogEvent::simple("core", LogLevel::Info, "started"),
    );
    let shared = Arc::new(CoreState {
        store: Mutex::new(store),
        llm_outbound: Mutex::new(()),
        asr_outbound: Mutex::new(()),
        voice_outbound: Mutex::new(()),
        cleanup_gate: Mutex::new(()),
        logs,
        launch_mode,
        run_id,
        directory: directory.clone(),
        models,
        microphone_busy: AtomicBool::new(false),
        system_busy: AtomicBool::new(false),
        capture: Mutex::new(CaptureRuntime::default()),
        allow_capture: AtomicBool::new(capture_after_launch),
        maintenance_pause: AtomicBool::new(false),
    });
    if models_present || speaker_present {
        let models = Arc::clone(&shared.models);
        let directory = directory.clone();
        thread::spawn(move || {
            let verified = model_pack::ready(&directory);
            let speaker_verified = model_pack::speaker_ready(&directory);
            if let Ok(mut state) = models.lock() {
                if matches!(state.phase, ModelPhase::Checking) {
                    state.phase = if verified {
                        ModelPhase::Ready
                    } else {
                        ModelPhase::Missing
                    };
                    state.progress_percent = if verified { 100 } else { 0 };
                }
                if matches!(state.speaker_phase, ModelPhase::Checking) {
                    state.speaker_phase = if speaker_verified {
                        ModelPhase::Ready
                    } else {
                        ModelPhase::Missing
                    };
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
    for provider in [
        hear_protocol::CloudProvider::Llm,
        hear_protocol::CloudProvider::Asr,
    ] {
        let cloud_shared = Arc::clone(&shared);
        let cloud_stop = Arc::clone(&stop);
        thread::spawn(move || cloud_loop(cloud_shared, cloud_stop, provider));
    }
    {
        let voice_shared = Arc::clone(&shared);
        let voice_stop = Arc::clone(&stop);
        thread::spawn(move || voice_loop(voice_shared, voice_stop));
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
        stream
            .set_nonblocking(false)
            .map_err(|error| error.to_string())?;
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
        let published = result.and_then(|result| {
            shared
                .store
                .lock()
                .map_err(|error| error.to_string())?
                .publish_analysis(&block, result)
        });
        let success = published.is_ok();
        if published.is_ok_and(|deleted| deleted) {
            if let (Ok(store), Ok(now)) = (shared.store.lock(), now_ms()) {
                let _ = cleanup::drain_deletions(&store.db, &shared.directory, now);
            }
        }
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

fn cloud_loop(
    shared: Arc<CoreState>,
    stop: Arc<AtomicBool>,
    provider: hear_protocol::CloudProvider,
) {
    let mut last_refresh = Instant::now() - Duration::from_secs(30);
    while !stop.load(Ordering::Acquire) {
        let now = match now_ms() {
            Ok(now) => now,
            Err(_) => {
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        if provider == hear_protocol::CloudProvider::Llm
            && last_refresh.elapsed() >= Duration::from_secs(15)
        {
            if let Ok(mut store) = shared.store.lock() {
                if let Err(error) = events::refresh_candidates(&mut store.db, now, false) {
                    eprintln!("hear-core event candidate refresh failed: {error}");
                }
            }
            last_refresh = Instant::now();
        }
        let task = shared
            .store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut store| events::take_task(&mut store.db, provider, now));
        let task = match task {
            Ok(Some(task)) => task,
            Ok(None) => {
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            Err(_) => {
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        let task_id = task.id;
        let outcome = process_cloud_task(&shared, &task, now);
        let mut event = LogEvent::simple(
            "cloud",
            if outcome.is_ok() {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            if outcome.is_ok() {
                "task_progress"
            } else {
                "task_failed"
            },
        );
        event.job_id = Some(task_id);
        if let Err(error) = &outcome {
            event.error_code = Some(error.clone());
        }
        shared.logs.emit(ClientKind::Worker, task_id, event);
        if let Err(error) = outcome {
            if let Ok(store) = shared.store.lock() {
                let _ = events::task_failed(&store.db, &task, &error, now_ms().unwrap_or(now));
            }
        }
    }
}

fn run_voice_worker(model_path: &Path, samples: &[f32]) -> Result<Vec<f32>, String> {
    let worker = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .with_file_name("hear-worker");
    run_voice_worker_with_timeout(&worker, model_path, samples, Duration::from_secs(30))
}

fn run_voice_worker_with_timeout(
    worker: &Path,
    model_path: &Path,
    samples: &[f32],
    timeout: Duration,
) -> Result<Vec<f32>, String> {
    let deadline = Instant::now() + timeout;
    let mut child = Command::new(worker)
        .arg("embed")
        .arg(model_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "worker_failed".to_owned())?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| "worker_failed".to_owned())?;
    let mut bytes = Vec::with_capacity(4 + samples.len() * 4);
    bytes.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = input
            .write_all(&bytes)
            .map_err(|_| "worker_failed".to_owned());
        drop(input);
        let _ = sender.send(result);
    });
    let mut write_result = None;
    loop {
        if let Ok(result) = receiver.try_recv() {
            write_result = Some(result);
        }
        if matches!(write_result, Some(Err(_))) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("worker_failed".into());
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("worker_timeout".to_owned());
            }
        }
    }
    if write_result.is_none() {
        write_result = receiver.recv_timeout(Duration::from_millis(500)).ok();
    }
    if !matches!(write_result, Some(Ok(()))) {
        let _ = child.wait();
        return Err("worker_failed".into());
    }
    let output = child
        .wait_with_output()
        .map_err(|_| "worker_failed".to_owned())?;
    if !output.status.success() || output.stdout.len() > 8_192 {
        return Err("worker_failed".to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "model_invalid".to_owned())
}

fn voice_error_code(error: &str) -> &'static str {
    if error.starts_with("audio block unavailable: ") {
        return "audio_unavailable";
    }
    match error {
        "speaker audio unavailable" | "ASR source audio unavailable" => "audio_unavailable",
        "speaker audio range unsuitable" | "speaker audio level unsuitable" => "audio_quality",
        "worker_timeout" => "worker_timeout",
        "model_invalid" => "model_invalid",
        _ => "worker_failed",
    }
}

fn voice_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        // Matching saved vectors is a database operation. An unavailable
        // model must not hide identities after an upgrade or sample change.
        let now = now_ms().unwrap_or(0);
        let matched = shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut store| knowledge::match_one_ready(&mut store.db, now));
        match matched {
            Ok(true) => continue,
            Err(_) => {
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            Ok(false) => {}
        }
        let ready = shared
            .models
            .lock()
            .is_ok_and(|state| matches!(state.speaker_phase, ModelPhase::Ready));
        if !ready {
            thread::sleep(Duration::from_secs(2));
            continue;
        }
        let Ok(_voice_gate) = shared.voice_outbound.lock() else {
            break;
        };
        let work = shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut store| knowledge::take_voice(&mut store.db, now));
        match work {
            Ok(Some(work)) => {
                let manifest = shared
                    .store
                    .lock()
                    .map_err(|error| error.to_string())
                    .and_then(|store| {
                        events::voice_manifest(&store.db, &work.source, work.start, work.end)
                    });
                let result = manifest
                    .and_then(|(task, parts)| {
                        events::voice_pcm_16k(&shared.directory, &task, &parts)
                    })
                    .and_then(|pcm| {
                        run_voice_worker(&model_pack::speaker_model_path(&shared.directory), &pcm)
                    });
                let mut event = LogEvent::simple("identity", LogLevel::Info, "voice_processed");
                event.job_id = Some(work.segment_id);
                match result {
                    Ok(vector) => {
                        if let Ok(mut store) = shared.store.lock() {
                            if knowledge::publish_voice(
                                &mut store.db,
                                &work,
                                &vector,
                                now_ms().unwrap_or(now),
                            )
                            .is_err()
                            {
                                let _ = knowledge::fail_voice(
                                    &store.db,
                                    work.segment_id,
                                    "model_invalid",
                                    now,
                                );
                                event.level = LogLevel::Warn;
                                event.error_code = Some("model_invalid".into());
                            }
                        }
                    }
                    Err(error) => {
                        let code = voice_error_code(&error);
                        if let Ok(store) = shared.store.lock() {
                            let _ = knowledge::fail_voice(&store.db, work.segment_id, code, now);
                        }
                        event.level = LogLevel::Warn;
                        event.error_code = Some(code.to_owned());
                    }
                }
                shared.logs.emit(ClientKind::Worker, work.segment_id, event);
            }
            Ok(None) => thread::sleep(Duration::from_secs(1)),
            Err(_) => thread::sleep(Duration::from_secs(2)),
        }
    }
}

fn process_cloud_task(
    shared: &CoreState,
    task: &events::CloudTask,
    now: i64,
) -> Result<(), String> {
    let (current, model) = shared
        .store
        .lock()
        .map_err(|e| e.to_string())
        .and_then(|store| {
            Ok((
                events::task_current(&store.db, task)?,
                events::model(&store.db, task.provider)?,
            ))
        })?;
    if !current {
        let store = shared.store.lock().map_err(|e| e.to_string())?;
        events::supersede_task(&store.db, task, now)?;
        return Ok(());
    }
    if model.is_none() {
        return Err("provider_not_configured".to_owned());
    }
    match task.provider {
        hear_protocol::CloudProvider::Llm => {
            let input = {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                events::llm_input(&store.db, task)?
            };
            let outbound = shared
                .outbound_gate(task.provider)
                .lock()
                .map_err(|e| e.to_string())?;
            let current = {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                events::task_current(&store.db, task)?
            };
            if !current {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                events::supersede_task(&store.db, task, now)?;
                return Ok(());
            }
            if queue_is_paused(shared, task.provider)? {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                events::defer_paused(&mut store.db, task, now)?;
                return Ok(());
            }
            let key = active_cloud_credential(shared, task.provider)?;
            let output = cloud::chat(&key, &task.model_id, events::FIT_SYSTEM, &input)?;
            drop(outbound);
            let _event_gate = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
            let count = {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                events::publish_fit(&mut store.db, task, output, now_ms()?)?
            };
            let mut event = LogEvent::simple("events", LogLevel::Info, "fit_published");
            event.job_id = Some(task.id);
            event.duration_ms = Some(count as u64);
            shared.logs.emit(ClientKind::Worker, task.id, event);
            Ok(())
        }
        hear_protocol::CloudProvider::Asr => {
            let query = {
                let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let current = {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::task_current(&store.db, task)?
                };
                if !current {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::supersede_task(&store.db, task, now)?;
                    return Ok(());
                }
                if queue_is_paused(shared, task.provider)? {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::defer_paused(&mut store.db, task, now)?;
                    return Ok(());
                }
                let key = active_cloud_credential(shared, task.provider)?;
                cloud::query_audio(&key, task.remote_id)?
            };
            match query {
                cloud::AsrQuery::Complete(utterances) => {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::publish_asr(&mut store.db, task, &utterances, now_ms()?)?;
                    Ok(())
                }
                cloud::AsrQuery::Processing => {
                    if task.submit_attempts > 0 && now - task.updated_ms > 2 * 60 * 60 * 1000 {
                        return Err("provider_processing_timeout".to_owned());
                    }
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::asr_wait(&store.db, task, now_ms()?)
                }
                cloud::AsrQuery::Missing => {
                    if task.submit_attempts > 0 && now - task.updated_ms < 60_000 {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        return events::asr_wait(&store.db, task, now_ms()?);
                    }
                    if task.submit_attempts >= 3 {
                        return Err("provider_missing_submitted_task".to_owned());
                    }
                    if queue_is_paused(shared, task.provider)? {
                        let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::defer_paused(&mut store.db, task, now)?;
                        return Ok(());
                    }
                    let parts = {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::audio_manifest(&store.db, task)?
                    };
                    let ogg = events::assemble_ogg(&shared.directory, task, &parts)?;
                    let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                    // Recheck after audio decoding: a boundary edit or cleanup
                    // may have superseded the task before any upload.
                    let current = {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::task_current(&store.db, task)?
                    };
                    if !current {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::supersede_task(&store.db, task, now)?;
                        return Ok(());
                    }
                    if queue_is_paused(shared, task.provider)? {
                        let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::defer_paused(&mut store.db, task, now)?;
                        return Ok(());
                    }
                    {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::mark_submit_intent(&store.db, task, now_ms()?)?;
                    }
                    let key = active_cloud_credential(shared, task.provider)?;
                    cloud::submit_audio(&key, task.remote_id, &ogg, cloud::AsrAudioFormat::Ogg)?;
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::asr_wait(&store.db, task, now_ms()?)
                }
            }
        }
    }
}

// Call only while holding this provider's outbound gate. Configuration swaps
// and retires Keychain references under the same gate.
fn active_cloud_credential(
    shared: &CoreState,
    provider: hear_protocol::CloudProvider,
) -> Result<String, String> {
    let reference = {
        let store = shared.store.lock().map_err(|e| e.to_string())?;
        events::credential_reference(&store.db, provider)?
    }
    .ok_or_else(|| "credential_missing".to_owned())?;
    cloud::credential(provider, &reference)?.ok_or_else(|| "credential_missing".to_owned())
}

fn queue_is_paused(
    shared: &CoreState,
    provider: hear_protocol::CloudProvider,
) -> Result<bool, String> {
    let store = shared.store.lock().map_err(|e| e.to_string())?;
    Ok(events::queue_states(&store.db)?
        .into_iter()
        .find(|q| q.provider == provider)
        .is_some_and(|q| q.paused))
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
    recorded_frames: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    join: thread::JoinHandle<()>,
}

fn start_source(shared: &Arc<CoreState>, source: AudioSource) -> SourceSlot {
    let session_id = uuid::Uuid::new_v4();
    let recorded_frames = Arc::new(AtomicU64::new(0));
    let source_frames = Arc::clone(&recorded_frames);
    let stop = Arc::new(AtomicBool::new(false));
    let source_stop = Arc::clone(&stop);
    let source_shared = Arc::clone(shared);
    let join = thread::spawn(move || {
        let status = microphone::allow_idle_sleep();
        let mut event = LogEvent::simple(
            "capture",
            if status == 0 {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            if status == 0 {
                "idle_sleep_allowed"
            } else {
                "idle_sleep_policy_failed"
            },
        );
        event.capture_span_id = Some(session_id);
        if status != 0 {
            event.error_code = Some("coreaudio_sleep_policy_failed".to_owned());
        }
        source_shared.logs.emit(ClientKind::Core, session_id, event);
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
                    let finalized_frames = match &event {
                        record::RecordEvent::Block(block) if !block.preparing => {
                            block.frames as u64
                        }
                        _ => 0,
                    };
                    source_shared
                        .store
                        .lock()
                        .map_err(|error| error.to_string())?
                        .insert_record_event(&source_shared.directory, session_id, event)?;
                    source_frames.fetch_add(finalized_frames, Ordering::Relaxed);
                    Ok(())
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
            } else if source == AudioSource::System
                && result
                    .as_ref()
                    .is_err_and(|error| error.contains("permission probe"))
            {
                Some("电脑声音录制授权需要重新验证".to_owned())
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
            event.error_code = Some(
                if source == AudioSource::System
                    && result
                        .as_ref()
                        .is_err_and(|error| error.contains("permission probe"))
                {
                    "system_audio_permission_probe_failed"
                } else {
                    "capture_source_failed"
                }
                .to_owned(),
            );
        }
        source_shared
            .logs
            .emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
    });
    SourceSlot {
        session_id,
        recorded_frames,
        stop,
        join,
    }
}

fn capture_retry_delay(previous: Duration, recorded_frames: u64) -> Duration {
    const BASE: Duration = Duration::from_secs(2);
    const MAX: Duration = Duration::from_secs(30);
    const STABLE_FRAMES: u64 = record::RATE as u64 * 60;
    if recorded_frames >= STABLE_FRAMES || previous < BASE {
        BASE
    } else {
        (previous * 2).min(MAX)
    }
}

fn reset_capture_retry(retry_at: &mut Instant, retry_delay: &mut Duration) {
    *retry_delay = Duration::ZERO;
    *retry_at = Instant::now();
}

fn capture_supervisor(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let mut microphone: Option<SourceSlot> = None;
    let mut system: Option<SourceSlot> = None;
    let mut microphone_retry_at = Instant::now();
    let mut system_retry_at = Instant::now();
    let mut microphone_retry_delay = Duration::ZERO;
    let mut system_retry_delay = Duration::ZERO;
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
            && !shared.maintenance_pause.load(Ordering::Acquire)
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

        for (source, slot, wanted, was_wanted, retry_at, retry_delay, busy) in [
            (
                AudioSource::Microphone,
                &mut microphone,
                want_microphone,
                &mut microphone_was_wanted,
                &mut microphone_retry_at,
                &mut microphone_retry_delay,
                &shared.microphone_busy,
            ),
            (
                AudioSource::System,
                &mut system,
                want_system,
                &mut system_was_wanted,
                &mut system_retry_at,
                &mut system_retry_delay,
                &shared.system_busy,
            ),
        ] {
            if slept {
                reset_capture_retry(retry_at, retry_delay);
            }
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
                    *retry_delay = capture_retry_delay(
                        *retry_delay,
                        current.recorded_frames.load(Ordering::Relaxed),
                    );
                    *retry_at = Instant::now() + *retry_delay;
                    let mut event =
                        LogEvent::simple("capture", LogLevel::Info, "source_retry_scheduled");
                    event.capture_span_id = Some(current.session_id);
                    event.duration_ms = Some(retry_delay.as_millis() as u64);
                    shared
                        .logs
                        .emit(ClientKind::Core, current.session_id, event);
                } else {
                    reset_capture_retry(retry_at, retry_delay);
                }
            }
            // Close after join: a failing source can open a gap while its
            // thread is winding down, even after pause was requested.
            if !wanted && *was_wanted {
                reset_capture_retry(retry_at, retry_delay);
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
    llm_outbound: Mutex<()>,
    asr_outbound: Mutex<()>,
    voice_outbound: Mutex<()>,
    cleanup_gate: Mutex<()>,
    logs: LogWorker,
    launch_mode: LaunchMode,
    run_id: uuid::Uuid,
    directory: std::path::PathBuf,
    models: Arc<Mutex<ModelState>>,
    microphone_busy: AtomicBool,
    system_busy: AtomicBool,
    capture: Mutex<CaptureRuntime>,
    allow_capture: AtomicBool,
    maintenance_pause: AtomicBool,
}

struct CaptureMaintenance<'a>(&'a CoreState);

impl Drop for CaptureMaintenance<'_> {
    fn drop(&mut self) {
        self.0.maintenance_pause.store(false, Ordering::Release);
    }
}

fn pause_capture_for_cleanup(shared: &CoreState) -> Result<CaptureMaintenance<'_>, String> {
    shared.maintenance_pause.store(true, Ordering::Release);
    let guard = CaptureMaintenance(shared);
    let deadline = Instant::now() + Duration::from_secs(30);
    while shared.microphone_busy.load(Ordering::Acquire)
        || shared.system_busy.load(Ordering::Acquire)
    {
        if Instant::now() >= deadline {
            return Err("audio capture did not finish sealing in time; retry cleanup".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(guard)
}

fn open_intent_gap(
    tx: &Transaction<'_>,
    source: &str,
    reason: &str,
    at: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason)
         SELECT ?1,?2,?3,NULL,?4 WHERE EXISTS(
           SELECT 1 FROM service_state WHERE id=1 AND enabled_intent=1 AND
           ((?2='microphone' AND microphone_enabled=1) OR (?2='system' AND system_enabled=1))
         ) AND NOT EXISTS(
           SELECT 1 FROM media_gaps WHERE source=?2 AND reason=?4 AND ended_ms IS NULL
         )",
        params![uuid::Uuid::new_v4().to_string(), source, at, reason],
    )?;
    Ok(())
}

fn close_intent_gaps(tx: &Transaction<'_>, reason: &str, at: i64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE media_gaps SET ended_ms=max(started_ms,?2) WHERE reason=?1 AND ended_ms IS NULL",
        params![reason, at],
    )?;
    Ok(())
}

#[derive(Default)]
struct CaptureRuntime {
    microphone_active: bool,
    system_active: bool,
    microphone_reason: Option<String>,
    system_reason: Option<String>,
}

impl CoreState {
    fn outbound_gate(&self, provider: hear_protocol::CloudProvider) -> &Mutex<()> {
        match provider {
            hear_protocol::CloudProvider::Llm => &self.llm_outbound,
            hear_protocol::CloudProvider::Asr => &self.asr_outbound,
        }
    }
    fn service_state(&self) -> Result<ServiceState, String> {
        let store = self.store.lock().map_err(|error| error.to_string())?;
        let mut state = store.state()?;
        let cloud_ready = events::configured(&store.db)?;
        drop(store);
        state.disk_free_bytes = fs2::available_space(&self.directory).unwrap_or(0);
        state.storage_warning = !state.storage_paused
            && (state.audio_used_bytes >= state.audio_limit_bytes.saturating_mul(85) / 100
                || state.disk_free_bytes <= 2 * 1_073_741_824);
        let models_ready = matches!(
            self.models.lock().map_err(|e| e.to_string())?.phase,
            ModelPhase::Ready
        );
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
        } else if !models_ready || !cloud_ready {
            state.capture_state = CaptureState::NotReady;
            state.reason = Some(if !models_ready {
                "local speech models are not ready".to_owned()
            } else {
                "configure both cloud providers before recording".to_owned()
            });
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
        } else {
            state.capture_state = CaptureState::Stopped;
            state.reason = None;
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

fn probe(source: AudioSource, seconds: u8, direct: bool) -> Result<ProbeStats, String> {
    if !(1..=20).contains(&seconds) {
        return Err("audio source test duration must be 1 to 20 seconds".to_owned());
    }
    match source {
        AudioSource::Microphone => microphone::Microphone::probe(seconds),
        AudioSource::System => {
            if !direct && system_audio::tap_available() {
                if !system_audio::tap_ready() {
                    return Err("system audio Tap requires foreground authorization".to_owned());
                }
                return system_audio::probe_child(seconds, Duration::from_secs(seconds as u64 + 8));
            }
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
            if frames == 0 {
                return Err("system audio probe received no frames".to_owned());
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
    // Store only a digest: ConfigureCloud contains a user-supplied secret.
    let mutation_fingerprint = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&envelope.request).map_err(|error| error.to_string())?)
    );
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
            Request::CloudStatus => {
                let queues = {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::queue_states(&store.db)?
                };
                Response::CloudStatus {
                    id: envelope.id,
                    queues,
                }
            }
            Request::CloudTasks { provider, limit } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                Response::CloudTasks {
                    id: envelope.id,
                    tasks: events::task_views(&store.db, provider, limit)?,
                }
            }
            Request::ConfigureCloud {
                provider,
                api_key,
                model,
            } => {
                let chosen = match provider {
                    hear_protocol::CloudProvider::Llm => {
                        model.unwrap_or_else(|| "deepseek-flash".to_owned())
                    }
                    hear_protocol::CloudProvider::Asr => "bigmodel".to_owned(),
                };
                let allowed = match provider {
                    hear_protocol::CloudProvider::Llm => {
                        matches!(chosen.as_str(), "deepseek-flash" | "deepseek-v4-pro")
                    }
                    hear_protocol::CloudProvider::Asr => chosen == "bigmodel",
                };
                let _outbound = shared
                    .outbound_gate(provider)
                    .lock()
                    .map_err(|e| e.to_string())?;
                let (prior, previous_reference) = {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    (
                        store.prior_mutation_response(envelope.id, &mutation_fingerprint)?,
                        events::credential_reference(&store.db, provider)?,
                    )
                };
                if let Some(response) = prior {
                    response
                } else if !allowed {
                    Response::Error {
                        id: envelope.id,
                        code: "model_unsupported".to_owned(),
                        message: "select a supported cloud model".to_owned(),
                    }
                } else {
                    let verified = match provider {
                        hear_protocol::CloudProvider::Llm => cloud::verify_llm(&api_key, &chosen),
                        hear_protocol::CloudProvider::Asr => cloud::verify_asr(&api_key),
                    };
                    match verified.and_then(|()| cloud::save_credential(provider, &api_key)) {
                        Ok(reference) => {
                            let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                            let outcome =
                                store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                                    events::set_configured_with_credential(
                                        tx, provider, &chosen, &reference,
                                    )?;
                                    Ok(Response::CloudStatus {
                                        id: envelope.id,
                                        queues: events::queue_states(tx)?,
                                    })
                                });
                            drop(store);
                            match outcome {
                                Ok(response @ Response::Error { .. }) => {
                                    let _ = cloud::delete_credential(&reference);
                                    response
                                }
                                Ok(response) => {
                                    if let Some(old) = previous_reference {
                                        let _ = cloud::delete_credential(&old);
                                    }
                                    response
                                }
                                Err(error) => {
                                    let _ = cloud::delete_credential(&reference);
                                    return Err(error);
                                }
                            }
                        }
                        Err(error) => Response::Error {
                            id: envelope.id,
                            code: error,
                            message: "cloud provider verification or macOS Keychain storage failed"
                                .to_owned(),
                        },
                    }
                }
            }
            Request::SetCloudPaused { provider, paused } => {
                let _outbound = shared
                    .outbound_gate(provider)
                    .lock()
                    .map_err(|e| e.to_string())?;
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    events::set_paused(tx, provider, paused)?;
                    Ok(Response::CloudStatus {
                        id: envelope.id,
                        queues: events::queue_states(tx)?,
                    })
                })?
            }
            Request::RetryCloudTask { task_id } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    events::retry_task_in(tx, task_id, now_ms()?)?;
                    Ok(Response::Accepted {
                        id: envelope.id,
                        object_id: task_id,
                    })
                }) {
                    Ok(response) => response,
                    Err(_) => Response::Error {
                        id: envelope.id,
                        code: "task_not_retryable".to_owned(),
                        message: "task is not failed or no longer matches the current event"
                            .to_owned(),
                    },
                }
            }
            Request::ProcessNow => {
                let mut count = 0;
                let response = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        count = events::refresh_candidates_in(tx, now_ms()?, true)?;
                        Ok(Response::CloudStatus {
                            id: envelope.id,
                            queues: events::queue_states(tx)?,
                        })
                    })?
                };
                let mut event =
                    LogEvent::simple("events", LogLevel::Info, "manual_candidate_refresh");
                event.duration_ms = Some(count as u64);
                shared.logs.emit(envelope.client, envelope.id, event);
                response
            }
            Request::Timeline { from_ms, to_ms } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                match events::timeline(&store.db, from_ms, to_ms) {
                    Ok(items) => Response::Timeline {
                        id: envelope.id,
                        items,
                        gaps: events::timeline_gaps(&store.db, from_ms, to_ms, now_ms()?)?,
                    },
                    Err(_) => Response::Error {
                        id: envelope.id,
                        code: "invalid_time_range".to_owned(),
                        message: "timeline range must be within one day".to_owned(),
                    },
                }
            }
            Request::EventDetail {
                event_id,
                offset,
                limit,
            } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                match events::event_detail(&store.db, event_id, offset, limit) {
                    Ok(event) => Response::Event {
                        id: envelope.id,
                        event,
                    },
                    Err(_) => Response::Error {
                        id: envelope.id,
                        code: "event_detail_unavailable".to_owned(),
                        message: "event not found or transcript page invalid".to_owned(),
                    },
                }
            }
            Request::RangeDetail {
                started_ms,
                ended_ms,
                sources,
                offset,
                limit,
            } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                match events::range_detail(&store.db, started_ms, ended_ms, sources, offset, limit)
                {
                    Ok(event) => Response::Event {
                        id: envelope.id,
                        event,
                    },
                    Err(_) => Response::Error {
                        id: envelope.id,
                        code: "range_detail_unavailable".to_owned(),
                        message: "invalid time range or transcript page".to_owned(),
                    },
                }
            }
            Request::CreateEvent {
                started_ms,
                ended_ms,
                title,
                sources,
            } => {
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        let object_id = events::create_event_in(
                            tx,
                            started_ms,
                            ended_ms,
                            &title,
                            sources,
                            now_ms()?,
                        )?;
                        Ok(Response::Accepted {
                            id: envelope.id,
                            object_id,
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "event_invalid".to_owned(),
                        message: error,
                    },
                }
            }
            Request::EditEvent {
                event_id,
                expected_revision,
                started_ms,
                ended_ms,
                title,
                sources,
            } => {
                let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        events::edit_event_in(
                            tx,
                            event_id,
                            expected_revision,
                            started_ms,
                            ended_ms,
                            &title,
                            sources,
                            now_ms()?,
                        )?;
                        Ok(Response::Accepted {
                            id: envelope.id,
                            object_id: event_id,
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "event_invalid".to_owned(),
                        message: error,
                    },
                }
            }
            Request::RetranscribeEvent {
                event_id,
                expected_revision,
            } => {
                let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        events::retranscribe_event_in(tx, event_id, expected_revision, now_ms()?)?;
                        Ok(Response::Accepted {
                            id: envelope.id,
                            object_id: event_id,
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "event_invalid".to_owned(),
                        message: error,
                    },
                }
            }
            Request::DeleteEvent {
                event_id,
                expected_revision,
            } => {
                let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        events::delete_event_in(tx, event_id, expected_revision, now_ms()?)?;
                        Ok(Response::EventsChanged {
                            id: envelope.id,
                            event_ids: Vec::new(),
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "event_invalid".to_owned(),
                        message: error,
                    },
                }
            }
            Request::SplitEvent {
                event_id,
                expected_revision,
                at_ms,
                left_title,
                right_title,
            } => {
                let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        let (left, right) = events::split_event_in(
                            tx,
                            event_id,
                            expected_revision,
                            at_ms,
                            &left_title,
                            &right_title,
                            now_ms()?,
                        )?;
                        Ok(Response::EventsChanged {
                            id: envelope.id,
                            event_ids: vec![left, right],
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "event_invalid".to_owned(),
                        message: error,
                    },
                }
            }
            Request::MergeEvents {
                left_id,
                left_revision,
                right_id,
                right_revision,
                title,
            } => {
                let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        let event = events::merge_events_in(
                            tx,
                            left_id,
                            left_revision,
                            right_id,
                            right_revision,
                            &title,
                            now_ms()?,
                        )?;
                        Ok(Response::EventsChanged {
                            id: envelope.id,
                            event_ids: vec![event],
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "event_invalid".to_owned(),
                        message: error,
                    },
                }
            }
            Request::TranscribeRange {
                started_ms,
                ended_ms,
                sources,
            } => {
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        let (range_id, tasks) = events::transcribe_range_in(
                            tx,
                            started_ms,
                            ended_ms,
                            sources,
                            now_ms()?,
                        )?;
                        Ok(Response::TranscriptionQueued {
                            id: envelope.id,
                            range_id,
                            tasks,
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: if error == "no retained audio in selected range" {
                            "range_no_audio"
                        } else {
                            "range_invalid"
                        }
                        .to_owned(),
                        message: error,
                    },
                }
            }
            Request::CorrectText {
                segment_id,
                text,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(tx, "segment", segment_id, expected_revision)?;
                    knowledge::correct_text(tx, segment_id, &text, now_ms()?)?;
                    Ok(Response::TextChanged {
                        id: envelope.id,
                        segment_id,
                        revision: knowledge::object_revision(tx, "segment", segment_id)?,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "text_correction_invalid"),
                        message: error,
                    },
                }
            }
            Request::RevertText {
                segment_id,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(tx, "segment", segment_id, expected_revision)?;
                    knowledge::revert_text(tx, segment_id, now_ms()?)?;
                    Ok(Response::TextChanged {
                        id: envelope.id,
                        segment_id,
                        revision: knowledge::object_revision(tx, "segment", segment_id)?,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "text_correction_invalid"),
                        message: error,
                    },
                }
            }
            Request::People => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                Response::People {
                    id: envelope.id,
                    people: knowledge::people(&store.db)?,
                }
            }
            Request::ObjectRevision { kind, object_id } => {
                let store = shared.store.lock().map_err(|error| error.to_string())?;
                match knowledge::object_revision(&store.db, &kind, object_id) {
                    Ok(revision) => Response::ObjectRevision {
                        id: envelope.id,
                        revision,
                    },
                    Err(message) => Response::Error {
                        id: envelope.id,
                        code: "revision_invalid".into(),
                        message,
                    },
                }
            }
            Request::VoiceSamples { person_id } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                Response::VoiceSamples {
                    id: envelope.id,
                    samples: knowledge::voice_samples(&store.db, person_id)?,
                }
            }
            Request::ReviewItems => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                Response::ReviewItems {
                    id: envelope.id,
                    items: knowledge::review_items(&store.db)?,
                }
            }
            Request::ResolveReview {
                item_id,
                target_segment_id,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(tx, "review", item_id, expected_revision)?;
                    knowledge::resolve_review(tx, item_id, target_segment_id, now_ms()?)?;
                    Ok(Response::ReviewResolved {
                        id: envelope.id,
                        item_id,
                        target_revision: target_segment_id
                            .map(|target| knowledge::object_revision(tx, "segment", target))
                            .transpose()?,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "review_invalid"),
                        message: error,
                    },
                }
            }
            Request::Search {
                query,
                from_ms,
                to_ms,
                person_id,
                offset,
                limit,
            } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                match knowledge::search(
                    &store.db,
                    query.as_deref(),
                    from_ms,
                    to_ms,
                    person_id,
                    offset,
                    limit,
                ) {
                    Ok(hits) => Response::Search {
                        id: envelope.id,
                        empty_reason: if hits.is_empty() && offset == 0 {
                            Some(knowledge::search_empty_reason(
                                &store.db, from_ms, to_ms, person_id,
                            )?)
                        } else {
                            None
                        },
                        hits,
                    },
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "search_invalid".into(),
                        message: error,
                    },
                }
            }
            Request::Moments {
                from_ms,
                to_ms,
                offset,
                limit,
            } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                match moments::list(&store.db, from_ms, to_ms, offset, limit) {
                    Ok(moments) => Response::Moments {
                        id: envelope.id,
                        moments,
                    },
                    Err(message) => Response::Error {
                        id: envelope.id,
                        code: "moments_invalid".into(),
                        message,
                    },
                }
            }
            Request::AddMoment {
                kind,
                anchor_ms,
                text,
            } => {
                let _gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    let moment = moments::add(tx, kind, anchor_ms, text.as_deref(), now_ms()?)?;
                    Ok(Response::MomentSaved {
                        id: envelope.id,
                        moment,
                    })
                }) {
                    Ok(response) => response,
                    Err(message) => Response::Error {
                        id: envelope.id,
                        code: "moment_invalid".into(),
                        message,
                    },
                }
            }
            Request::CaptureScreenshot => {
                let trigger_ms = now_ms()?;
                let _gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                let prior = {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.prior_mutation_response(envelope.id, &mutation_fingerprint)?
                };
                if let Some(prior) = prior {
                    prior
                } else {
                    match moments::capture(&shared.directory, trigger_ms) {
                        Ok(captured) => {
                            let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                            let saved =
                                store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                                    let moment = moments::save_capture(tx, &captured, now_ms()?)?;
                                    Ok(Response::MomentSaved {
                                        id: envelope.id,
                                        moment,
                                    })
                                });
                            if saved.is_err() {
                                let _ = fs::remove_file(shared.directory.join(&captured.relative));
                            }
                            match saved {
                                Ok(response) => response,
                                Err(message) => Response::Error {
                                    id: envelope.id,
                                    code: "screenshot_invalid".into(),
                                    message,
                                },
                            }
                        }
                        Err(message) => Response::Error {
                            id: envelope.id,
                            code: "screenshot_failed".into(),
                            message,
                        },
                    }
                }
            }
            Request::ScreenshotData { moment_id } => {
                let _gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                let result = Connection::open_with_flags(
                    shared.directory.join("hear.sqlite"),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .map_err(|error| error.to_string())
                .and_then(|db| moments::image(&db, &shared.directory, moment_id));
                match result {
                    Ok(png_base64) => Response::ScreenshotData {
                        id: envelope.id,
                        moment_id,
                        png_base64,
                    },
                    Err(message) => Response::Error {
                        id: envelope.id,
                        code: "screenshot_unavailable".into(),
                        message,
                    },
                }
            }
            Request::PlaybackChunk {
                from_ms,
                to_ms,
                sources,
            } => {
                let _gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                let result = Connection::open_with_flags(
                    shared.directory.join("hear.sqlite"),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .map_err(|error| error.to_string())
                .and_then(|db| playback::chunk(&db, &shared.directory, from_ms, to_ms, sources));
                match result {
                    Ok((wav_base64, has_audio, next_audio_ms)) => Response::PlaybackChunk {
                        id: envelope.id,
                        from_ms,
                        to_ms,
                        wav_base64,
                        has_audio,
                        next_audio_ms,
                    },
                    Err(message) => Response::Error {
                        id: envelope.id,
                        code: "playback_unavailable".into(),
                        message,
                    },
                }
            }
            Request::PreviewCleanup {
                from_ms,
                to_ms,
                sources,
                mode,
            } => {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                match cleanup::preview(&store.db, from_ms, to_ms, sources, mode, now_ms()?) {
                    Ok(preview) => Response::CleanupPreview {
                        id: envelope.id,
                        preview,
                    },
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "cleanup_preview_invalid".into(),
                        message: error,
                    },
                }
            }
            Request::ExecuteCleanup {
                preview_token,
                confirmed_all_data,
            } => {
                let _cleanup_gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                let _llm = shared.llm_outbound.lock().map_err(|e| e.to_string())?;
                let _asr = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let _voice = shared.voice_outbound.lock().map_err(|e| e.to_string())?;
                let near_capture_edge = {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    cleanup::verified_scope(&store.db, preview_token, confirmed_all_data, now_ms()?)
                        .is_ok_and(|scope| scope.end >= now_ms().unwrap_or(0) - 20_000)
                };
                let _maintenance = if near_capture_edge {
                    Some(pause_capture_for_cleanup(shared)?)
                } else {
                    None
                };
                let snapshot = (|| {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    if let Some(previous) =
                        store.prior_mutation_response(envelope.id, &mutation_fingerprint)?
                    {
                        Ok(Err(previous))
                    } else {
                        let scope = cleanup::verified_scope(
                            &store.db,
                            preview_token,
                            confirmed_all_data,
                            now_ms()?,
                        )?;
                        let originals = cleanup::originals(&store.db, &scope)?;
                        Ok(Ok((scope, originals)))
                    }
                })();
                match snapshot {
                    Ok(Err(previous)) => previous,
                    Ok(Ok((scope, originals))) => {
                        match cleanup::prepare_media(&shared.directory, &scope, originals) {
                            Ok(prepared) => {
                                let result = {
                                    let mut store =
                                        shared.store.lock().map_err(|e| e.to_string())?;
                                    store.mutate_response(
                                        envelope.id,
                                        &mutation_fingerprint,
                                        |tx| {
                                            let (cleared_audio_blocks, physical_deletions_pending) =
                                                cleanup::apply(tx, &scope, &prepared, now_ms()?)?;
                                            Ok(Response::CleanupComplete {
                                                id: envelope.id,
                                                cleared_audio_blocks,
                                                physical_deletions_pending,
                                            })
                                        },
                                    )
                                };
                                match result {
                                    Ok(response @ Response::CleanupComplete { .. }) => {
                                        cleanup::clear_staging_markers(
                                            &shared.directory,
                                            &prepared,
                                        );
                                        if let Ok(store) = shared.store.lock() {
                                            let _ = cleanup::drain_deletions(
                                                &store.db,
                                                &shared.directory,
                                                now_ms()?,
                                            );
                                        }
                                        response
                                    }
                                    Ok(response) => {
                                        cleanup::remove_prepared(&shared.directory, &prepared);
                                        response
                                    }
                                    Err(error) => {
                                        cleanup::remove_prepared(&shared.directory, &prepared);
                                        Response::Error {
                                            id: envelope.id,
                                            code: "cleanup_failed".into(),
                                            message: error,
                                        }
                                    }
                                }
                            }
                            Err(error) => Response::Error {
                                id: envelope.id,
                                code: "cleanup_failed".into(),
                                message: error,
                            },
                        }
                    }
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "cleanup_invalid".into(),
                        message: error,
                    },
                }
            }
            Request::ExportRange {
                from_ms,
                to_ms,
                sources,
                include_audio,
                include_screenshots,
                destination,
            } => {
                let db_path = shared.directory.join("hear.sqlite");
                let result = rusqlite::Connection::open(db_path)
                    .map_err(|error| error.to_string())
                    .and_then(|db| {
                        db.busy_timeout(Duration::from_secs(5))
                            .map_err(|error| error.to_string())?;
                        archive::export(
                            &db,
                            &shared.directory,
                            from_ms,
                            to_ms,
                            sources,
                            include_audio,
                            include_screenshots,
                            &destination,
                            &shared.cleanup_gate,
                        )
                    });
                match result {
                    Ok((audio_files, transcript_segments)) => Response::ExportComplete {
                        id: envelope.id,
                        destination,
                        audio_files,
                        transcript_segments,
                    },
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "export_failed".into(),
                        message: error,
                    },
                }
            }
            Request::CreatePerson { display_name } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    let person_id = knowledge::create_person(tx, &display_name, now_ms()?)?;
                    Ok(Response::PersonChanged {
                        id: envelope.id,
                        person_id,
                        revision: Some(knowledge::object_revision(tx, "person", person_id)?),
                        deleted: false,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "person_invalid".into(),
                        message: error,
                    },
                }
            }
            Request::CreateAndBindSpeaker {
                segment_id,
                display_name,
                entire_local_speaker,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(
                        tx,
                        if entire_local_speaker {
                            "speaker_group"
                        } else {
                            "segment"
                        },
                        segment_id,
                        expected_revision,
                    )?;
                    let person_id = knowledge::create_person(tx, &display_name, now_ms()?)?;
                    knowledge::bind_speaker(
                        tx,
                        segment_id,
                        person_id,
                        entire_local_speaker,
                        now_ms()?,
                    )?;
                    Ok(Response::PersonChanged {
                        id: envelope.id,
                        person_id,
                        revision: Some(knowledge::object_revision(tx, "person", person_id)?),
                        deleted: false,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "speaker_invalid"),
                        message: error,
                    },
                }
            }
            Request::RenamePerson {
                person_id,
                display_name,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(tx, "person", person_id, expected_revision)?;
                    knowledge::rename_person(tx, person_id, &display_name, now_ms()?)?;
                    Ok(Response::PersonChanged {
                        id: envelope.id,
                        person_id,
                        revision: Some(knowledge::object_revision(tx, "person", person_id)?),
                        deleted: false,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "person_invalid"),
                        message: error,
                    },
                }
            }
            Request::DeletePerson {
                person_id,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(tx, "person", person_id, expected_revision)?;
                    knowledge::delete_person(tx, person_id)?;
                    Ok(Response::PersonChanged {
                        id: envelope.id,
                        person_id,
                        revision: None,
                        deleted: true,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "person_invalid"),
                        message: error,
                    },
                }
            }
            Request::BindSpeaker {
                segment_id,
                person_id,
                entire_local_speaker,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(
                        tx,
                        if entire_local_speaker {
                            "speaker_group"
                        } else {
                            "segment"
                        },
                        segment_id,
                        expected_revision,
                    )?;
                    knowledge::bind_speaker(
                        tx,
                        segment_id,
                        person_id,
                        entire_local_speaker,
                        now_ms()?,
                    )?;
                    Ok(Response::TextChanged {
                        id: envelope.id,
                        segment_id,
                        revision: knowledge::object_revision(
                            tx,
                            if entire_local_speaker {
                                "speaker_group"
                            } else {
                                "segment"
                            },
                            segment_id,
                        )?,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "speaker_binding_invalid"),
                        message: error,
                    },
                }
            }
            Request::UnbindSpeaker {
                segment_id,
                entire_local_speaker,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(
                        tx,
                        if entire_local_speaker {
                            "speaker_group"
                        } else {
                            "segment"
                        },
                        segment_id,
                        expected_revision,
                    )?;
                    knowledge::unbind_speaker(tx, segment_id, entire_local_speaker, now_ms()?)?;
                    Ok(Response::TextChanged {
                        id: envelope.id,
                        segment_id,
                        revision: knowledge::object_revision(
                            tx,
                            if entire_local_speaker {
                                "speaker_group"
                            } else {
                                "segment"
                            },
                            segment_id,
                        )?,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "speaker_binding_invalid"),
                        message: error,
                    },
                }
            }
            Request::AddVoiceSample {
                segment_id,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(tx, "segment", segment_id, expected_revision)?;
                    knowledge::add_voice_sample(tx, segment_id, now_ms()?)?;
                    Ok(Response::TextChanged {
                        id: envelope.id,
                        segment_id,
                        revision: knowledge::object_revision(tx, "segment", segment_id)?,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "voice_sample_invalid"),
                        message: error,
                    },
                }
            }
            Request::RemoveVoiceSample {
                sample_id,
                expected_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::expect_revision(tx, "sample", sample_id, expected_revision)?;
                    let person_id = knowledge::remove_voice_sample(tx, sample_id, now_ms()?)?;
                    Ok(Response::PersonChanged {
                        id: envelope.id,
                        person_id,
                        revision: Some(knowledge::object_revision(tx, "person", person_id)?),
                        deleted: false,
                    })
                }) {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: mutation_error_code(&error, "voice_sample_invalid"),
                        message: error,
                    },
                }
            }
            Request::Start { .. }
                if !shared
                    .models
                    .lock()
                    .is_ok_and(|models| matches!(models.phase, ModelPhase::Ready)) =>
            {
                Response::Error {
                    id: envelope.id,
                    code: "model_not_ready".to_owned(),
                    message: "install and verify local speech models before recording".to_owned(),
                }
            }
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
                let speech_needed = matches!(state.phase, ModelPhase::Missing | ModelPhase::Failed);
                let speaker_needed = matches!(
                    state.speaker_phase,
                    ModelPhase::Missing | ModelPhase::Failed
                );
                if speech_needed || speaker_needed {
                    if speech_needed {
                        state.phase = ModelPhase::Downloading;
                        state.progress_percent = 0;
                    }
                    if speaker_needed {
                        state.speaker_phase = ModelPhase::Downloading;
                    }
                    let models = Arc::clone(&shared.models);
                    let directory = shared.directory.clone();
                    let logs = shared.logs.clone();
                    let correlation_id = envelope.id;
                    thread::spawn(move || {
                        let speech_success = if speech_needed {
                            let result = model_pack::install(&directory, |progress| {
                                if let Ok(mut state) = models.lock() {
                                    state.progress_percent = progress;
                                }
                            });
                            if let Ok(mut state) = models.lock() {
                                state.phase = if result.is_ok() {
                                    ModelPhase::Ready
                                } else {
                                    ModelPhase::Failed
                                };
                                state.progress_percent = if result.is_ok() { 100 } else { 0 };
                            }
                            result.is_ok()
                        } else {
                            true
                        };
                        let speaker_success = if speaker_needed {
                            let result = model_pack::install_speaker(&directory);
                            if let Ok(mut state) = models.lock() {
                                state.speaker_phase = if result.is_ok() {
                                    ModelPhase::Ready
                                } else {
                                    ModelPhase::Failed
                                };
                            }
                            result.is_ok()
                        } else {
                            true
                        };
                        let success = speech_success && speaker_success;
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
            Request::Probe { source, seconds } => match probe(source, seconds, false) {
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

fn mutation_error_code(error: &str, fallback: &str) -> String {
    if error.starts_with("revision_conflict:") {
        "revision_conflict".into()
    } else {
        fallback.into()
    }
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
        events::init(&db)?;
        moments::init(&db)?;
        cleanup::init(&db)?;
        archive::init(&db)?;
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
             WHERE source = ?1 AND ended_ms IS NULL
             AND reason NOT IN ('manual_pause','storage_limit','source_disabled')",
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
        let at = now_ms()?;
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        open_intent_gap(&tx, "microphone", "storage_limit", at)
            .map_err(|error| error.to_string())?;
        open_intent_gap(&tx, "system", "storage_limit", at).map_err(|error| error.to_string())?;
        tx.execute(
            "UPDATE capture_guard SET pause_reason = 'storage_limit' WHERE id = 1",
            [],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
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
    ) -> Result<bool, String> {
        if result.error.is_some() {
            return Err("local speech worker rejected audio".to_owned());
        }
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        let still_current: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE id=?1 AND
             analysis_state='processing' AND media_path=?2)",
                params![block.id, block.media_path],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !still_current {
            return Ok(false);
        }
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
        let retention = retention::resolve_analysis(
            &tx,
            &block.session_id,
            block.sample_start,
            &block.id,
            block.flush,
            now_ms()?,
        )?;
        for previous_id in retention.newly_speech {
            events::note_analysis(&tx, &previous_id)?;
        }
        events::note_analysis(&tx, &block.id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(retention.deleted_media)
    }

    fn handle(&mut self, id: uuid::Uuid, request: Request) -> Result<Response, String> {
        let fingerprint = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        match request {
            Request::Status | Request::Quit => Ok(Response::Status {
                id,
                state: self.state()?,
            }),
            Request::Start { expected_revision } => {
                if !events::configured(&self.db)? {
                    return Ok(Response::Error {
                        id,
                        code: "setup_incomplete".to_owned(),
                        message: "configure both cloud providers before recording".to_owned(),
                    });
                }
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    close_intent_gaps(tx, "manual_pause", at)?;
                    tx.execute("UPDATE service_state SET enabled_intent=1,paused_intent=0,revision=revision+1 WHERE id=1",[])
                })
            }
            Request::Pause { expected_revision } => {
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    open_intent_gap(tx, "microphone", "manual_pause", at)?;
                    open_intent_gap(tx, "system", "manual_pause", at)?;
                    tx.execute("UPDATE service_state SET paused_intent = 1, revision = revision + 1 WHERE id = 1", [])
                })
            }
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
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    close_intent_gaps(tx, "manual_pause", at)?;
                    close_intent_gaps(tx, "storage_limit", at)?;
                    tx.execute("UPDATE capture_guard SET pause_reason = NULL WHERE id = 1", [])?;
                    tx.execute("UPDATE service_state SET paused_intent = 0, revision = revision + 1 WHERE id = 1", [])
                })
            }
            Request::SetSource {
                source,
                enabled,
                expected_revision,
            } => {
                let column = match source {
                    AudioSource::Microphone => "microphone_enabled",
                    AudioSource::System => "system_enabled",
                };
                let source_name = source_name(source);
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    if enabled {
                        tx.execute("UPDATE media_gaps SET ended_ms=max(started_ms,?2)
                          WHERE source=?1 AND reason='source_disabled' AND ended_ms IS NULL",
                            params![source_name,at])?;
                    } else {
                        open_intent_gap(tx, source_name, "source_disabled", at)?;
                    }
                    tx.execute(
                        &format!("UPDATE service_state SET {column} = ?1, revision = revision + 1 WHERE id = 1"),
                        params![enabled],
                    )
                })
            }
            Request::SetAudioLimit {
                limit_bytes,
                expected_revision,
            } => {
                if !(64 * 1_048_576..=1024 * 1_073_741_824).contains(&limit_bytes) {
                    return Ok(Response::Error {
                        id,
                        code: "audio_limit_invalid".to_owned(),
                        message: "audio limit must be between 64 MiB and 1 TiB".to_owned(),
                    });
                }
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    tx.execute(
                        "UPDATE capture_guard SET audio_limit_bytes=?1,
                         pause_reason=CASE WHEN
                           (SELECT coalesce(sum(bytes),0) FROM audio_blocks) +
                           (SELECT coalesce(sum(bytes),0) FROM orphan_media) >= ?1
                           THEN 'storage_limit' ELSE pause_reason END
                         WHERE id=1",
                        params![limit_bytes as i64],
                    )?;
                    let paused: bool = tx.query_row(
                        "SELECT pause_reason IS NOT NULL FROM capture_guard WHERE id=1",
                        [],
                        |row| row.get(0),
                    )?;
                    if paused {
                        open_intent_gap(tx, "microphone", "storage_limit", at)?;
                        open_intent_gap(tx, "system", "storage_limit", at)?;
                    }
                    tx.execute(
                        "UPDATE service_state SET revision=revision+1 WHERE id=1",
                        [],
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
            Request::CloudStatus
            | Request::CloudTasks { .. }
            | Request::ConfigureCloud { .. }
            | Request::SetCloudPaused { .. }
            | Request::RetryCloudTask { .. }
            | Request::ProcessNow
            | Request::Timeline { .. }
            | Request::EventDetail { .. }
            | Request::RangeDetail { .. }
            | Request::CreateEvent { .. }
            | Request::EditEvent { .. }
            | Request::RetranscribeEvent { .. }
            | Request::DeleteEvent { .. }
            | Request::SplitEvent { .. }
            | Request::MergeEvents { .. }
            | Request::TranscribeRange { .. }
            | Request::CorrectText { .. }
            | Request::RevertText { .. } => {
                unreachable!("handled by IPC dispatcher")
            }
            Request::ObjectRevision { .. }
            | Request::People
            | Request::CreatePerson { .. }
            | Request::CreateAndBindSpeaker { .. }
            | Request::RenamePerson { .. }
            | Request::DeletePerson { .. }
            | Request::BindSpeaker { .. }
            | Request::UnbindSpeaker { .. }
            | Request::AddVoiceSample { .. }
            | Request::RemoveVoiceSample { .. }
            | Request::VoiceSamples { .. }
            | Request::ReviewItems
            | Request::ResolveReview { .. }
            | Request::Search { .. }
            | Request::Moments { .. }
            | Request::AddMoment { .. }
            | Request::CaptureScreenshot
            | Request::ScreenshotData { .. }
            | Request::PlaybackChunk { .. }
            | Request::PreviewCleanup { .. }
            | Request::ExecuteCleanup { .. }
            | Request::ExportRange { .. } => {
                unreachable!("handled by IPC dispatcher")
            }
        }
    }

    fn prior_mutation_response(
        &self,
        id: uuid::Uuid,
        fingerprint: &str,
    ) -> Result<Option<Response>, String> {
        prior_mutation_response(&self.db, id, fingerprint)
    }

    fn mutate_response(
        &mut self,
        id: uuid::Uuid,
        fingerprint: &str,
        change: impl FnOnce(&Transaction<'_>) -> Result<Response, String>,
    ) -> Result<Response, String> {
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        if let Some(response) = prior_mutation_response(&tx, id, fingerprint)? {
            return Ok(response);
        }
        let response = change(&tx)?;
        if matches!(response, Response::Error { .. }) {
            return Ok(response);
        }
        let response_json = serde_json::to_string(&response).map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO mutation_receipts (request_id, fingerprint, state_json) VALUES (?1, ?2, ?3)",
            params![id.to_string(), fingerprint, response_json],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(response)
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

fn prior_mutation_response(
    db: &Connection,
    id: uuid::Uuid,
    fingerprint: &str,
) -> Result<Option<Response>, String> {
    let prior: Option<(String, String)> = db
        .query_row(
            "SELECT fingerprint,state_json FROM mutation_receipts WHERE request_id=?1",
            params![id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    match prior {
        None => Ok(None),
        Some((previous, _)) if previous != fingerprint => Ok(Some(Response::Error {
            id,
            code: "idempotency_conflict".to_owned(),
            message: "request ID was already used for a different mutation".to_owned(),
        })),
        Some((_, response)) => serde_json::from_str(&response)
            .map(Some)
            .map_err(|error| error.to_string()),
    }
}

fn read_state(db: &Connection) -> Result<ServiceState, String> {
    db.query_row(
        "SELECT revision, enabled_intent, paused_intent, microphone_enabled, system_enabled,
                (SELECT pause_reason IS NOT NULL FROM capture_guard WHERE id = 1),
                (SELECT coalesce(sum(bytes),0) FROM audio_blocks) +
                (SELECT coalesce(sum(bytes),0) FROM orphan_media),
                (SELECT audio_limit_bytes FROM capture_guard WHERE id=1)
         FROM service_state WHERE id = 1",
        [],
        |row| {
            Ok(ServiceState {
                revision: row.get(0)?,
                enabled_intent: row.get(1)?,
                paused_intent: row.get(2)?,
                storage_paused: row.get(5)?,
                audio_used_bytes: row.get::<_, i64>(6)?.max(0) as u64,
                audio_limit_bytes: row.get::<_, i64>(7)?.max(0) as u64,
                disk_free_bytes: 0,
                storage_warning: false,
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
    fn speaker_input_quality_is_not_reported_as_missing_audio() {
        assert_eq!(
            voice_error_code("speaker audio range unsuitable"),
            "audio_quality"
        );
        assert_eq!(
            voice_error_code("speaker audio level unsuitable"),
            "audio_quality"
        );
        assert_eq!(
            voice_error_code("speaker audio unavailable"),
            "audio_unavailable"
        );
        assert_eq!(
            voice_error_code("speaker audio decoder failed"),
            "worker_failed"
        );
    }

    #[test]
    fn speaker_input_classification_uses_retained_audio_and_actual_file() {
        let directory = tempfile::tempdir().unwrap();
        let db = Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        assert_eq!(
            voice_error_code(&events::voice_manifest(&db, "microphone", 0, 3_000).unwrap_err()),
            "audio_unavailable"
        );
        assert_eq!(
            voice_error_code(&events::voice_manifest(&db, "microphone", 0, 1_000).unwrap_err()),
            "audio_quality"
        );

        let id = Uuid::new_v4();
        let relative = format!("media/microphone/{id}.opus");
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
             frames,bytes,media_path,retention_decision,analysis_state,speech_active)
             VALUES (?1,'session','microphone',0,3000,0,144000,100,?2,'retain','complete',0)",
            rusqlite::params![id.to_string(), relative],
        )
        .unwrap();
        let (task, parts) = events::voice_manifest(&db, "microphone", 0, 3_000).unwrap();
        let missing = events::voice_pcm_16k(directory.path(), &task, &parts).unwrap_err();
        assert_eq!(voice_error_code(&missing), "audio_unavailable");

        let media_directory = directory.path().join("media/microphone");
        fs::create_dir_all(&media_directory).unwrap();
        let mut block = media::OpusBlock::create(&media_directory, id).unwrap();
        block.push(&vec![0.0; 144_000]).unwrap();
        block.seal().unwrap();
        let quiet = events::voice_pcm_16k(directory.path(), &task, &parts).unwrap_err();
        assert_eq!(voice_error_code(&quiet), "audio_quality");
    }

    #[test]
    fn stalled_voice_worker_cannot_hold_the_core_forever_while_stdin_is_full() {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join("stalled-worker");
        fs::write(&worker, b"#!/bin/sh\nexec sleep 60\n").unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        let samples = vec![0.1; 16_000 * 15];
        let result = run_voice_worker_with_timeout(
            &worker,
            directory.path(),
            &samples,
            Duration::from_millis(200),
        );
        assert_eq!(result.unwrap_err(), "worker_timeout");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

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
    fn capture_retry_is_fast_after_a_stable_run_and_bounded_when_unavailable() {
        let mut delay = Duration::ZERO;
        for expected in [2, 4, 8, 16, 30, 30] {
            delay = capture_retry_delay(delay, record::RATE as u64 * 3);
            assert_eq!(delay, Duration::from_secs(expected));
        }
        assert_eq!(
            capture_retry_delay(delay, record::RATE as u64 * 59),
            Duration::from_secs(30)
        );
        assert_eq!(
            capture_retry_delay(delay, record::RATE as u64 * 60),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn sleep_clears_retry_even_when_no_source_slot_is_running() {
        let mut delay = Duration::from_secs(30);
        let mut retry_at = Instant::now() + delay;
        let before = Instant::now();
        reset_capture_retry(&mut retry_at, &mut delay);
        let after = Instant::now();
        assert_eq!(delay, Duration::ZERO);
        assert!(retry_at >= before && retry_at <= after);
        assert_eq!(
            capture_retry_delay(delay, record::RATE as u64 * 3),
            Duration::from_secs(2)
        );
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
    fn lowering_audio_limit_pauses_and_raising_it_requires_manual_resume() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let mut store = Store::new(Connection::open(&path).unwrap()).unwrap();
        store
            .db
            .execute(
                "INSERT INTO orphan_media(media_path,bytes,detected_ms) VALUES ('media/orphan',?1,1)",
                params![80 * 1_048_576_i64],
            )
            .unwrap();
        store
            .handle(
                Uuid::new_v4(),
                Request::SetAudioLimit {
                    limit_bytes: 64 * 1_048_576,
                    expected_revision: 0,
                },
            )
            .unwrap();
        assert!(store.state().unwrap().storage_paused);
        store
            .handle(
                Uuid::new_v4(),
                Request::SetAudioLimit {
                    limit_bytes: 512 * 1_048_576,
                    expected_revision: 1,
                },
            )
            .unwrap();
        assert!(store.state().unwrap().storage_paused);
        assert_eq!(store.state().unwrap().audio_limit_bytes, 512 * 1_048_576);
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
    fn active_manual_pause_is_visible_as_a_timed_gap_until_resume() {
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store
            .db
            .execute("UPDATE service_state SET enabled_intent=1 WHERE id=1", [])
            .unwrap();
        let before = now_ms().unwrap();
        store
            .handle(
                Uuid::new_v4(),
                Request::Pause {
                    expected_revision: 0,
                },
            )
            .unwrap();
        store.db.execute("UPDATE media_gaps SET started_ms=?1 WHERE reason='manual_pause' AND ended_ms IS NULL", params![before - 5_000]).unwrap();
        let during = events::timeline_gaps(
            &store.db,
            before - 5_000,
            before + 10_000,
            now_ms().unwrap(),
        )
        .unwrap();
        assert_eq!(
            during
                .iter()
                .filter(|gap| gap.reason == "manual_pause")
                .count(),
            2
        );
        store
            .handle(
                Uuid::new_v4(),
                Request::Resume {
                    expected_revision: 1,
                },
            )
            .unwrap();
        let open_count: i64 = store
            .db
            .query_row(
                "SELECT count(*) FROM media_gaps WHERE reason='manual_pause' AND ended_ms IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(open_count, 0);
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
    fn event_mutation_receipt_survives_lost_reply_and_commits_with_event() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events.sqlite");
        let create_request = Uuid::new_v4();
        let split_request = Uuid::new_v4();
        let mut store = Store::new(Connection::open(&path).unwrap()).unwrap();
        let created = store
            .mutate_response(create_request, "sha256:create", |tx| {
                let task_id = events::create_event_in(
                    tx,
                    1_000,
                    6_000,
                    "会议",
                    hear_protocol::EventSources::Microphone,
                    7_000,
                )?;
                Ok(Response::Accepted {
                    id: create_request,
                    object_id: task_id,
                })
            })
            .unwrap();
        let Response::Accepted {
            object_id: original,
            ..
        } = created
        else {
            panic!("expected event")
        };
        drop(store);

        let mut reopened = Store::new(Connection::open(&path).unwrap()).unwrap();
        let replay = reopened
            .mutate_response(create_request, "sha256:create", |_| {
                panic!("a duplicate request must not create another event")
            })
            .unwrap();
        assert!(matches!(replay,Response::Accepted {object_id,..} if object_id==original));
        let split = reopened
            .mutate_response(split_request, "sha256:split", |tx| {
                let (left, right) =
                    events::split_event_in(tx, original, 1, 3_000, "前半", "后半", 7_001)?;
                Ok(Response::EventsChanged {
                    id: split_request,
                    event_ids: vec![left, right],
                })
            })
            .unwrap();
        let Response::EventsChanged {
            event_ids: successors,
            ..
        } = split
        else {
            panic!("expected successors")
        };
        drop(reopened);

        let mut reopened = Store::new(Connection::open(&path).unwrap()).unwrap();
        let replay = reopened
            .mutate_response(split_request, "sha256:split", |_| {
                panic!("a split retry must not consume revisions or change successors")
            })
            .unwrap();
        assert!(matches!(replay,Response::EventsChanged {event_ids,..} if event_ids==successors));
        let conflict = reopened
            .mutate_response(split_request, "sha256:different", |_| {
                panic!("a reused ID with new arguments must be rejected")
            })
            .unwrap();
        assert!(matches!(conflict,Response::Error {code,..} if code=="idempotency_conflict"));
        let failed = Uuid::new_v4();
        assert!(
            reopened
                .mutate_response(failed, "sha256:failed", |tx| {
                    events::create_event_in(
                        tx,
                        6_000,
                        8_000,
                        "未提交",
                        hear_protocol::EventSources::Microphone,
                        7_002,
                    )?;
                    Err("simulate reply creation failure".to_owned())
                })
                .is_err()
        );
        let active: i64 = reopened
            .db
            .query_row(
                "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active, 2);
        let receipts: i64 = reopened
            .db
            .query_row("SELECT count(*) FROM mutation_receipts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(receipts, 2);
    }

    #[test]
    fn conflicting_cloud_configuration_cannot_switch_active_credential() {
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let initial = Uuid::new_v4();
        let original_ref = format!("deepseek:{}", Uuid::new_v4());
        store
            .mutate_response(initial, "sha256:initial", |tx| {
                events::set_configured_with_credential(
                    tx,
                    hear_protocol::CloudProvider::Llm,
                    "deepseek-flash",
                    &original_ref,
                )?;
                Ok(Response::CloudStatus {
                    id: initial,
                    queues: events::queue_states(tx)?,
                })
            })
            .unwrap();
        let contested = Uuid::new_v4();
        assert!(
            store
                .prior_mutation_response(contested, "sha256:cloud")
                .unwrap()
                .is_none()
        );
        store
            .mutate_response(contested, "sha256:event", |tx| {
                let task_id = events::create_event_in(
                    tx,
                    1_000,
                    6_000,
                    "先到的事件",
                    hear_protocol::EventSources::Microphone,
                    7_000,
                )?;
                Ok(Response::Accepted {
                    id: contested,
                    object_id: task_id,
                })
            })
            .unwrap();
        let new_ref = format!("deepseek:{}", Uuid::new_v4());
        let conflict = store
            .mutate_response(contested, "sha256:cloud", |tx| {
                events::set_configured_with_credential(
                    tx,
                    hear_protocol::CloudProvider::Llm,
                    "deepseek-v4-pro",
                    &new_ref,
                )?;
                Ok(Response::CloudStatus {
                    id: contested,
                    queues: events::queue_states(tx)?,
                })
            })
            .unwrap();
        assert!(matches!(conflict,Response::Error {code,..} if code=="idempotency_conflict"));
        assert_eq!(
            events::credential_reference(&store.db, hear_protocol::CloudProvider::Llm).unwrap(),
            Some(original_ref)
        );
        assert_eq!(
            events::model(&store.db, hear_protocol::CloudProvider::Llm)
                .unwrap()
                .as_deref(),
            Some("deepseek-flash")
        );
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
