#![cfg_attr(windows, windows_subsystem = "windows")]
use loppi_protocol::ipc::{Listener as UnixListener, Stream as UnixStream};
use loppi_protocol::platform::OpenOptions;
use loppi_protocol::{
    AudioSource, CaptureState, ClientKind, Envelope, LifecycleAction, LogEvent, LogFilter,
    LogLevel, ModelPhase, ModelState, PROTOCOL_VERSION, ProbeStats, Request, Response,
    ServiceState, data_dir,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
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
mod archive;
mod capture;
mod cleanup;
mod cloud;
mod coarse;
mod context;
mod credentials;
mod diagnostic_report;
#[cfg(test)]
mod diagnostic_tests;
mod diagnostics;
mod events;
mod ingestion;
mod inventory;
mod knowledge;
mod media;
mod microphone;
mod model_pack;
mod moments;
#[cfg(windows)]
mod ocr;
#[cfg(windows)]
mod pipe_io;
mod playback;
mod reads;
mod record;
mod recovery;
mod resources;
mod retention;
mod schedule;
mod source_process;
mod storage;
mod system_audio;
mod unilogs;
mod usage;
mod vocabulary;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LaunchMode {
    OnDemand,
    Login,
}

fn main() {
    #[cfg(windows)]
    if let Err(error) = loppi_protocol::platform::initialize_process() {
        eprintln!("loppi: {error}");
        std::process::exit(1);
    }
    if let Err(error) = run() {
        eprintln!("loppi-core: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    #[cfg(windows)]
    loppi_protocol::platform::require_supported().map_err(|e| e.to_string())?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if std::env::args().nth(1).as_deref() == Some("--build-info") {
        println!(
            "{}",
            serde_json::to_string(&loppi_protocol::build_info("core")).unwrap()
        );
        return Ok(());
    }
    if args.as_slice() == ["--check-credentials"] {
        let result = cloud::check_credentials(&data_dir()?.join("loppi.sqlite"))?;
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        return if result["ok"] == true {
            Ok(())
        } else {
            Err("configured cloud credentials are not readable; see check results".into())
        };
    }
    if args.first().is_some_and(|s| s == "capture-source") {
        if args.len() != 2 {
            return Err("invalid capture helper arguments".into());
        }
        return source_process::run_child(args[1].parse().map_err(|_| "invalid capture parent")?);
    }
    if args
        .first()
        .is_some_and(|value| value == "probe-system" || value == "probe-mic")
    {
        let source = if args[0] == "probe-mic" {
            AudioSource::Microphone
        } else {
            AudioSource::System
        };
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
            return Err("usage: loppi-core probe-system [1..20 seconds]".to_owned());
        };
        if !(1..=20).contains(&seconds) || parent_pid.is_some_and(|pid| pid <= 1) {
            return Err("usage: loppi-core probe-system [1..20 seconds]".to_owned());
        }
        #[cfg(windows)]
        if let Some(parent_pid) = parent_pid {
            loppi_protocol::platform::watch_process_exit(parent_pid as u32)
                .map_err(|e| e.to_string())?;
        }
        if let Some(parent_pid) = parent_pid {
            if loppi_protocol::platform::parent_pid() != parent_pid {
                // Forced helper termination must not run global native
                // destructors concurrently with capture/model threads.
                loppi_protocol::platform::hard_exit(70);
            }
            thread::spawn(move || {
                while loppi_protocol::platform::parent_pid() == parent_pid {
                    thread::sleep(Duration::from_millis(200));
                }
                // HAL can remain blocked after the caller exits. The child
                // must not survive its authorization/probe owner.
                // Forced helper termination must not run global native
                // destructors concurrently with capture/model threads.
                loppi_protocol::platform::hard_exit(70);
            });
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&probe(source, seconds as u8, true)?)
                .map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    let handoff = if args.len() == 3 && args[0] == "serve" && args[1] == "--handoff" {
        match args[2].as_str() {
            "capture" => Some((true, true)),
            "normal" => Some((false, true)),
            "query" => Some((false, false)),
            _ => return Err("invalid handoff mode".into()),
        }
    } else {
        None
    };
    if handoff.is_none()
        && !args.is_empty()
        && args.as_slice() != ["serve", "--on-demand"]
        && args.as_slice() != ["serve", "--login"]
    {
        return Err("usage: loppi-core [serve --on-demand|serve --login]".to_owned());
    }
    let launch_mode = if handoff.is_some() || args.as_slice() == ["serve", "--on-demand"] {
        LaunchMode::OnDemand
    } else {
        LaunchMode::Login
    };
    watch_test_owner()?;

    let directory = data_dir()?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    loppi_protocol::platform::set_mode(&directory, 0o700).map_err(|error| error.to_string())?;
    let Some(lock) = acquire_owner_lock(&directory, launch_mode)? else {
        return Ok(());
    };
    let socket = directory.join("core.sock");
    #[cfg(unix)]
    remove_stale_socket(&socket)?;
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    loppi_protocol::platform::set_mode(&socket, 0o600).map_err(|error| error.to_string())?;
    let connection =
        Connection::open(directory.join("loppi.sqlite")).map_err(|error| error.to_string())?;
    let store = Store::new(connection)?;
    archive::prepare_recovery(&store.db)?;
    let mut store = store;
    store.recover(&directory)?;
    events::recover_tasks(&mut store.db, now_ms()?)?;
    knowledge::recover_voice(&store.db)?;
    let login_session = current_login_session()?;
    let capture_after_launch = handoff
        .map(|v| v.0)
        .unwrap_or(launch_mode == LaunchMode::Login && !store.quit_in_login(login_session)?);
    if handoff.is_some_and(|v| v.1) {
        store.clear_quit()?;
    }
    if launch_mode == LaunchMode::Login && !capture_after_launch {
        store.close_open_gaps(AudioSource::Microphone)?;
        store.close_open_gaps(AudioSource::System)?;
    }
    let run_id = uuid::Uuid::new_v4();
    let logs = LogWorker::start(directory.clone(), run_id);
    let model_assets = model_pack::Assets::discover()?;
    let models = Arc::new(Mutex::new(ModelState {
        phase: ModelPhase::Checking,
        progress_percent: 0,
        error: None,
        speaker_error: None,
        speaker_phase: ModelPhase::Checking,
    }));
    let shared = Arc::new(CoreState {
        readers: reads::Pool::open(&directory.join("loppi.sqlite"))?,
        store: Mutex::new(store),
        llm_outbound: Mutex::new(()),
        workflow_output: events::workflow::LiveOutput::default(),
        asr_outbound: Mutex::new(()),
        voice_outbound: Mutex::new(()),
        cleanup_gate: Mutex::new(()),
        logs,
        launch_mode,
        run_id,
        directory: directory.clone(),
        model_assets,
        models,
        microphone_busy: AtomicBool::new(false),
        system_busy: AtomicBool::new(false),
        capture: Mutex::new(CaptureRuntime::default()),
        allow_capture: AtomicBool::new(capture_after_launch),
        normal_run: AtomicBool::new(handoff.map(|v| v.1).unwrap_or(capture_after_launch)),
        resources: resources::Governor::new(),
    });
    check_models(&shared, true, true, uuid::Uuid::new_v4());
    let stop = Arc::new(AtomicBool::new(false));
    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || recovery::run(shared, stop));
    }
    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || maintenance_loop(shared, stop));
    }
    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || ingestion_loop(shared, stop));
    }
    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut next = Instant::now();
            let mut scan = None;
            while !stop.load(Ordering::Acquire) {
                if scan.is_none() && Instant::now() >= next {
                    scan = inventory::Scan::new(&shared.store).ok();
                    next = Instant::now() + Duration::from_secs(1800);
                }
                if let Some(current) = scan.as_mut() {
                    match current.step(&shared.store, &shared.cleanup_gate, &shared.directory) {
                        Ok(false) => {}
                        _ => scan = None,
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        });
    }
    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || resource_loop(shared, stop));
    }
    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || coarse_loop(shared, stop));
    }
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
        loppi_protocol::CloudProvider::Llm,
        loppi_protocol::CloudProvider::Asr,
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
            eprintln!("loppi-core rejected a local client: {error}");
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
                Err(_) => {} // serve_one records the phase and known request ID.
            }
            clients.fetch_sub(1, Ordering::AcqRel);
        });
    }
    let _ = capture_thread.join();
    drop(listener);
    #[cfg(unix)]
    fs::remove_file(&socket).map_err(|error| error.to_string())?;
    drop(lock);
    Ok(())
}

// Only the core commits facts. The cleanup gate covers planning through durable
// acknowledgement; Store is held only for SQL, never Opus, hashing or fsync.
fn ingest_session(shared: &CoreState, session: uuid::Uuid, closed: bool) -> Result<(), String> {
    let _gate = shared
        .cleanup_gate
        .lock()
        .map_err(|_| "ingestion_cleanup_lock")?;
    ingest_session_locked(&shared.store, &shared.directory, session, closed)
}

fn ingest_session_locked(
    store: &Mutex<Store>,
    directory: &Path,
    session: uuid::Uuid,
    closed: bool,
) -> Result<(), String> {
    let paths = match ingestion::pending(directory, session, closed) {
        Ok(paths) => paths,
        // The publisher owns the journal namespace until its fsync finishes.
        // The existing ingestion poll will retry without treating contention
        // as damaged media or losing the session's durable pending marker.
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
        Err(error) => {
            return Err(match error.kind() {
                std::io::ErrorKind::PermissionDenied => "ingestion_list_permission",
                std::io::ErrorKind::NotADirectory => "ingestion_list_not_directory",
                _ => "ingestion_list_io",
            }
            .into());
        }
    };
    let mut entries = Vec::new();
    for path in paths {
        // Ready supersedes prepared during the atomic publication window.
        if path.extension().is_some_and(|e| e == "prepared")
            && path.with_extension("ready").exists()
        {
            continue;
        }
        match ingestion::load(&path) {
            Ok(entry) => entries.push((path, entry)),
            Err(_) => {
                ingestion::quarantine(directory, &path).map_err(|_| "ingestion_quarantine")?
            }
        }
    }
    entries.sort_by_key(|(_, e)| (e.started_ms, e.ended_ms, e.id));
    for (path, entry) in entries {
        let (acknowledged, ranges) = {
            let began = Instant::now();
            let store = store.lock().map_err(|_| "ingestion_store_lock")?;
            store.ingestion_wait_ms.observe(began.elapsed());
            let ack =
                ingestion::acknowledged(&store.db, &entry).map_err(|_| "ingestion_receipt_read")?;
            (
                ack,
                if ack {
                    vec![]
                } else {
                    ingestion::surviving_samples(&store.db, &entry)
                        .map_err(|_| "ingestion_ranges")?
                },
            )
        };
        if !acknowledged {
            match ingestion::prepare(directory, &entry, &ranges) {
                Ok(prepared) => {
                    let result = {
                        let began = Instant::now();
                        let mut store = store.lock().map_err(|_| "ingestion_store_lock")?;
                        store.ingestion_wait_ms.observe(began.elapsed());
                        let began = Instant::now();
                        let result = ingestion::commit(&mut store.db, &entry, &prepared);
                        store.ingestion_commit_ms.observe(began.elapsed());
                        result
                    };
                    if result.is_err() {
                        ingestion::discard_prepared(directory, &prepared);
                        return Err("ingestion_commit".into());
                    }
                    ingestion::finish_markers(directory, &prepared);
                }
                Err(ingestion::PrepareError::Retry(code)) => return Err(code.into()),
                Err(ingestion::PrepareError::Invalid(_reason)) => {
                    // Missing/corrupt sealed media cannot be called a confirmed
                    // recording. Preserve the manifest and expose an explicit gap.
                    ingestion::reject(
                        &mut store.lock().map_err(|_| "ingestion_store_lock")?.db,
                        &entry,
                    )
                    .map_err(|_| "ingestion_reject")?;
                    ingestion::quarantine(directory, &path).map_err(|_| "ingestion_quarantine")?;
                }
            }
        }
        ingestion::acknowledge(directory, &entry).map_err(|_| "ingestion_acknowledge")?;
        ingestion::forget_receipt(
            &store.lock().map_err(|_| "ingestion_store_lock")?.db,
            &entry,
        )
        .map_err(|_| "ingestion_receipt_delete")?;
    }
    let free = fs2::available_space(directory).map_err(|_| "ingestion_disk_space")?;
    {
        let mut store = store.lock().map_err(|_| "ingestion_store_lock")?;
        let state = store.state().map_err(|_| "ingestion_storage_state")?;
        if !state.storage_paused
            && (state.audio_used_bytes >= state.audio_limit_bytes || free <= 1_073_741_824)
        {
            store
                .pause_storage()
                .map_err(|_| "ingestion_pause_storage")?;
        }
    }
    if closed
        && ingestion::finish_journal(directory, session).map_err(|_| "ingestion_finish_journal")?
    {
        ingestion::forget_session(
            &store.lock().map_err(|_| "ingestion_store_lock")?.db,
            session,
        )
        .map_err(|_| "ingestion_session_delete")?;
    }

    Ok(())
}

fn ingestion_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let mut after = String::new();
    while !stop.load(Ordering::Acquire) {
        let sessions = shared
            .store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|s| ingestion::pending_sessions(&s.db, &after));
        if let Ok(sessions) = sessions {
            after = sessions.last().map(ToString::to_string).unwrap_or_default();
            for session in sessions {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let closed = shared
                    .store
                    .lock()
                    .ok()
                    .and_then(|s| {
                        s.db.query_row(
                            "SELECT state!='open' FROM recording_sessions WHERE id=?1",
                            [session.to_string()],
                            |r| r.get::<_, bool>(0),
                        )
                        .ok()
                    })
                    .unwrap_or(false);
                if let Err(code) = ingest_session(&shared, session, closed) {
                    let mut event = LogEvent::simple("capture", LogLevel::Warn, "ingestion_retry");
                    event.capture_span_id = Some(session);
                    event.error_code = Some(code);
                    shared.logs.emit(ClientKind::Core, session, event);
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
}

// The database owns deletion intent; filesystem latency never holds Store.
// A cursor lets failed unlinks yield to later work, then retry next sweep.
fn maintenance_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let mut after = String::new();
    let mut last_optimize = Instant::now();
    while !stop.load(Ordering::Acquire) {
        if let Ok(mut store) = shared.store.lock() {
            if last_optimize.elapsed() >= Duration::from_secs(3600) {
                let _ = store.db.execute_batch("PRAGMA optimize;");
                last_optimize = Instant::now();
            }
            if let Ok(tx) = store.db.transaction() {
                if retention::settle_unreachable(&tx).is_ok() {
                    let _ = tx.commit();
                }
            }
        }
        let batch = shared
            .store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|store| cleanup::deletion_batch(&store.db, &after));
        match batch {
            Ok(jobs) if !jobs.is_empty() => {
                for path in jobs {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    after = path.clone();
                    // Share the existing media lifetime guard with playback,
                    // export and cleanup; never acquire Store before this guard.
                    let Ok(_media_guard) = shared.cleanup_gate.lock() else {
                        return;
                    };
                    match cleanup::delete_file(&shared.directory, &path) {
                        Ok(deleted) => {
                            if let Ok(mut store) = shared.store.lock() {
                                if let Ok(tx) = store.db.transaction() {
                                    if cleanup::finish_deletion(
                                        &tx,
                                        &path,
                                        deleted,
                                        now_ms().unwrap_or(0),
                                    )
                                    .is_ok()
                                    {
                                        let _ = tx.commit();
                                    }
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
                thread::sleep(Duration::from_millis(25));
            }
            _ => {
                after.clear();
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn resource_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let mut blocked_since = None;
    let mut storage_metrics = serde_json::Value::Null;
    let mut last_mode = shared.resources.mode();
    let mut last_power = resources::power();
    while !stop.load(Ordering::Acquire) {
        let mut backlog = resources::Backlog {
            disk_free: fs2::available_space(&shared.directory).unwrap_or(0),
            ..Default::default()
        };
        let now = now_ms().unwrap_or(0);
        if let Ok(store) = shared.store.try_lock() {
            blocked_since = None;
            if let Ok(value)=store.db.query_row("SELECT audio_bytes,orphan_bytes FROM storage_usage WHERE id=1",[],|r|Ok(serde_json::json!({"logical_audio_bytes":r.get::<_,i64>(0)?,"orphan_bytes":r.get::<_,i64>(1)?}))) {storage_metrics=value;}
            storage_metrics["observed_ms"] = serde_json::json!(now);
            storage_metrics["control_transaction_histogram_ms"] =
                serde_json::json!(store.transaction_ms.counts());
            storage_metrics["ingestion_lock_wait_histogram_ms"] =
                serde_json::json!(store.ingestion_wait_ms.counts());
            storage_metrics["ingestion_commit_histogram_ms"] =
                serde_json::json!(store.ingestion_commit_ms.counts());
            if let Ok(encoded)=store.db.query_row("SELECT json_group_array(json_object('category',category,'bytes',bytes,'observed_ms',observed_ms)) FROM physical_storage",[],|r|r.get::<_,String>(0)) {
                storage_metrics["physical_scan"]=serde_json::from_str(&encoded).unwrap_or(serde_json::Value::Null);
            }
            if let Ok((seconds,oldest))=store.db.query_row("SELECT coalesce(sum(frames),0)/48000,coalesce(min(started_ms),?1) FROM
              (SELECT frames,started_ms FROM audio_blocks WHERE analysis_state='pending' AND analysis_attempts<3 AND media_path IS NOT NULL ORDER BY started_ms,id LIMIT 1024)",[now],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))) {
                backlog.vad_seconds=seconds.max(0) as u64;backlog.oldest_vad_ms=now.saturating_sub(oldest).max(0) as u64;
            }
            if let Ok((seconds,oldest))=store.db.query_row("SELECT coalesce(sum(ended_ms-started_ms),0)/1000,coalesce(min(started_ms),?1) FROM
              (SELECT started_ms,ended_ms FROM coarse_jobs WHERE state='pending' ORDER BY started_ms,id LIMIT 1024)",[now],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))) {
                backlog.coarse_seconds=seconds.max(0) as u64;backlog.oldest_coarse_ms=now.saturating_sub(oldest).max(0) as u64;
            }
            if let Ok((count, oldest)) = store.db.query_row("SELECT count(*),coalesce(min(started_ms),?1) FROM (
                SELECT started_ms FROM (SELECT started_ms FROM voice_observations WHERE state='pending' ORDER BY updated_ms LIMIT 512)
                UNION ALL SELECT started_ms FROM (SELECT started_ms FROM voice_enrollments WHERE state='pending' ORDER BY available_ms LIMIT 512))", [now],
                |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?))) {
                backlog.voice_pending = count.max(0) as u64;
                backlog.oldest_voice_ms = now.saturating_sub(oldest).max(0) as u64;
            }
            backlog.voice_last_success_ms = store.db.query_row("SELECT coalesce(max(updated_ms),0) FROM voice_observations WHERE state='ready'", [], |r| r.get::<_,i64>(0)).unwrap_or(0).max(0) as u64;
        } else {
            backlog.store_wait_ms = blocked_since
                .get_or_insert_with(Instant::now)
                .elapsed()
                .as_millis() as u64;
        }
        shared.resources.sample(backlog);
        let power = resources::power();
        if power.generation != last_power.generation {
            let mut event = LogEvent::simple("capture", LogLevel::Info, "power_changed");
            event.context = Some(loppi_protocol::LogContext {
                cause: Some(
                    if power.sleeping != 0 {
                        "machine_sleep"
                    } else {
                        "full_wake"
                    }
                    .into(),
                ),
                start_ms: (power.sleep_ms > 0).then_some(power.sleep_ms),
                ..Default::default()
            });
            shared
                .logs
                .emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
            last_power = power;
        }
        let mode = shared.resources.mode();
        if mode != last_mode {
            let mut event = LogEvent::simple("resources", LogLevel::Info, "mode_changed");
            event.context = Some(loppi_protocol::LogContext {
                cause: Some(format!("{mode:?}").to_ascii_lowercase()),
                footprint_bytes: shared.resources.diagnostics()["footprint_bytes"].as_u64(),
                count: Some(backlog.voice_pending),
                ..Default::default()
            });
            shared
                .logs
                .emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
            last_mode = mode;
        }
        if shared.resources.mode() != resources::Mode::Normal {
            shared.readers.shrink();
        }
        let diagnostics = serde_json::json!({"schema":2,"build":loppi_protocol::build_info("core"),"run_id":shared.run_id,"pid":std::process::id(),"observed_ms":now,
            "resources":shared.resources.diagnostics(),"storage":storage_metrics,"power":power,
            "wal_bytes":fs::metadata(shared.directory.join("loppi.sqlite-wal")).map(|m|m.len()).ok()});
        // A bounded, replace-only diagnostic snapshot; it is not durable
        // business state and contains no audio, text, people or credentials.
        let path = shared.directory.join("diagnostics.json.pending");
        if let Ok(mut file) = loppi_protocol::platform::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
        {
            if serde_json::to_writer(&mut file, &diagnostics).is_ok() {
                let _ = loppi_protocol::platform::rename(
                    &path,
                    shared.directory.join("diagnostics.json"),
                );
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn coarse_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let model = shared.model_assets.speech_directory();
    let mut worker: Option<analysis::AnalysisClient> = None;
    let mut turn = 0usize;
    while !stop.load(Ordering::Acquire) {
        if worker.as_ref().is_some_and(|w| {
            w.idle_for() > Duration::from_secs(30)
                || shared.resources.unload(resources::Work::Coarse)
        }) {
            worker = None;
        }
        if !shared
            .models
            .lock()
            .is_ok_and(|s| matches!(s.phase, ModelPhase::Ready))
        {
            shared.resources.no_work(resources::Work::Coarse);
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        let available=shared.store.lock().ok().and_then(|s|s.db.query_row("SELECT EXISTS(SELECT 1 FROM coarse_jobs WHERE state='pending' AND available_ms<=?1)",[now_ms().unwrap_or(0)],|r|r.get::<_,bool>(0)).ok()).unwrap_or(false);
        if !available {
            shared.resources.no_work(resources::Work::Coarse);
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        let Some(_permit) = shared.resources.try_admit(resources::Work::Coarse) else {
            thread::sleep(Duration::from_millis(250));
            continue;
        };
        let now = now_ms().unwrap_or(0);
        let first = if turn % 2 == 0 {
            "microphone"
        } else {
            "system"
        };
        let second = if first == "microphone" {
            "system"
        } else {
            "microphone"
        };
        // Keep live feedback, but spend most turns closing the oldest local
        // holes when a mature fitting window is waiting for rough text.
        let closing_window = shared
            .store
            .lock()
            .ok()
            .and_then(|s| {
                s.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM candidate_windows c WHERE c.state='pending'
               AND EXISTS(SELECT 1 FROM coarse_jobs j WHERE j.state='pending'
                 AND j.started_ms<c.ended_ms AND j.ended_ms>c.started_ms))",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .ok()
            })
            .unwrap_or(false);
        let newest = turn % 8 < if closing_window { 2 } else { 6 };
        turn = turn.wrapping_add(1);
        let task = shared
            .store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut s| match coarse::take(&mut s.db, first, newest, now)? {
                Some(job) => Ok(Some(job)),
                None => coarse::take(&mut s.db, second, newest, now),
            });
        let job = match task {
            Ok(Some(job)) => job,
            _ => {
                drop(_permit);
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        let mut stage = "coarse_manifest";
        let result = (|| -> Result<String, String> {
            let parts =
                coarse::manifest(&shared.store.lock().map_err(|e| e.to_string())?.db, &job)?;
            stage = "coarse_decode";
            let pcm = coarse::pcm(&shared.directory, &job, parts)?;
            if worker.is_none() {
                stage = "coarse_worker_start";
                worker = Some(
                    analysis::AnalysisClient::start(
                        &exe.with_file_name(loppi_protocol::platform::executable("loppi-worker")),
                        "recognize",
                        &model,
                    )?
                    .managed(&shared.resources, resources::Work::Coarse),
                );
            }
            stage = "coarse_recognize";
            let v = vocabulary::list(&shared.store.lock().map_err(|e| e.to_string())?.db)?;
            worker
                .as_mut()
                .unwrap()
                .set_hotwords(if v["settings"]["local_asr"] == true {
                    serde_json::from_value::<Vec<loppi_protocol::vocabulary::Entry>>(
                        v["entries"].clone(),
                    )
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter(|e| e.enabled)
                    .map(|e| e.name)
                    .collect()
                } else {
                    Vec::new()
                });
            worker.as_mut().unwrap().request(
                &job.source,
                &job.session_id,
                job.start,
                &pcm,
                true,
                Duration::from_secs(60),
                &|| {
                    stop.load(Ordering::Acquire) || shared.resources.cancel(resources::Work::Coarse)
                },
            )
        })();
        let published = match result {
            Ok(text) => shared
                .store
                .lock()
                .map_err(|_| "store_unavailable".to_owned())
                .and_then(|mut store| {
                    coarse::publish(&mut store.db, &job, &text)
                        .map_err(|_| "publish_failed".to_owned())
                }),
            Err(error) => {
                worker = None;
                Err(error)
            }
        };
        if let Err(error) = &published {
            if let Ok(store) = shared.store.lock() {
                let _ = coarse::failed(&store.db, &job, error, now_ms().unwrap_or(now));
            }
        }
        shared.logs.emit(
            ClientKind::Core,
            uuid::Uuid::new_v4(),
            coarse_log_event(&job, &published, stage),
        );
    }
}

fn coarse_log_event(job: &coarse::Job, published: &Result<bool, String>, stage: &str) -> LogEvent {
    let (level, message) = match published {
        Ok(true) => (LogLevel::Info, "coarse_complete"),
        Ok(false) => (LogLevel::Info, "coarse_not_published"),
        Err(error) if error == "resource_deferred" => (LogLevel::Info, "coarse_deferred"),
        Err(_) => (LogLevel::Warn, "coarse_failed"),
    };
    let mut event = LogEvent::simple("speech", level, message);
    event.job_id = uuid::Uuid::parse_str(&job.id).ok();
    event.capture_span_id = uuid::Uuid::parse_str(&job.session_id).ok();
    event.error_code = published.as_ref().err().map(|error| {
        if error == "publish_failed" || error == "resource_deferred" {
            error.clone()
        } else {
            analysis::failure_code(stage, error)
        }
    });
    event.context = Some(loppi_protocol::LogContext {
        start_ms: Some(job.started_ms),
        end_ms: Some(job.ended_ms),
        count: Some(job.end.saturating_sub(job.start)),
        source: Some(if job.source == "microphone" {
            AudioSource::Microphone
        } else {
            AudioSource::System
        }),
        ..Default::default()
    });
    event
}

fn begin_model_check(state: &mut ModelState) -> (bool, bool) {
    let recheck_all = matches!(state.phase, ModelPhase::Ready)
        && matches!(state.speaker_phase, ModelPhase::Ready);
    let check = |phase| recheck_all || matches!(phase, ModelPhase::Missing | ModelPhase::Failed);
    let speech = check(state.phase);
    let speaker = check(state.speaker_phase);
    if speech {
        state.phase = ModelPhase::Checking;
        state.progress_percent = 0;
        state.error = None;
    }
    if speaker {
        state.speaker_phase = ModelPhase::Checking;
        state.speaker_error = None;
    }
    (speech, speaker)
}

fn check_models(shared: &Arc<CoreState>, speech: bool, speaker: bool, correlation_id: uuid::Uuid) {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        let mut success = true;
        if speech {
            let result = shared.model_assets.verify(false).and_then(|_| {
                check_speech_load(&shared.models, &shared.resources, &shared.model_assets)
            });
            success &= result.is_ok();
            finish_model(&shared.models, false, result);
        }
        if speaker {
            let result = shared.model_assets.verify(true);
            success &= result.is_ok();
            finish_model(&shared.models, true, result);
        }
        let mut event = LogEvent::simple(
            "models",
            if success {
                LogLevel::Info
            } else {
                LogLevel::Error
            },
            if success { "checked" } else { "check_failed" },
        );
        event.correlation_id = Some(correlation_id);
        if !success {
            event.error_code = Some("model_setup_failed".into());
        }
        shared.logs.emit(ClientKind::Core, correlation_id, event);
    });
}

fn finish_model(models: &Mutex<ModelState>, speaker: bool, result: Result<(), String>) {
    if let Ok(mut state) = models.lock() {
        let phase = if result.is_ok() {
            ModelPhase::Ready
        } else {
            ModelPhase::Failed
        };
        if speaker {
            state.speaker_phase = phase;
            state.speaker_error = result.err();
        } else {
            state.phase = phase;
            state.error = result.err();
            state.progress_percent = if matches!(phase, ModelPhase::Ready) {
                100
            } else {
                0
            };
        }
    }
}

/// Synthetic in-memory input exercises actual loading, never a capture session,
/// database write or cloud request. Each child is killed and reaped on drop.
fn check_speech_load(
    models: &Mutex<ModelState>,
    resources: &Arc<resources::Governor>,
    assets: &model_pack::Assets,
) -> Result<(), String> {
    if let Ok(mut state) = models.lock() {
        state.phase = ModelPhase::Loading;
    }
    let permit = resources.wait_admit(resources::Work::Coarse, Duration::from_secs(30));
    let _permit = match permit {
        Ok(p) => p,
        Err(_) => {
            resources.no_work(resources::Work::Coarse);
            return Err("model_resources_busy".into());
        }
    };
    let worker = std::env::current_exe()
        .map_err(|_| "model_worker_missing")?
        .with_file_name(loppi_protocol::platform::executable("loppi-worker"));
    let directory = assets.speech_directory();
    let samples = vec![0.0f32; 16_000];
    let cancelled = || resources.cancel(resources::Work::Coarse);
    {
        let mut vad = analysis::AnalysisClient::start(&worker, "vad", &directory)
            .map_err(|_| "model_worker_missing")?;
        let result: analysis::VadResult = vad
            .request(
                "microphone",
                "model-load-check",
                0,
                &samples,
                true,
                Duration::from_secs(30),
                &cancelled,
            )
            .map_err(|e| model_pack::load_error_code(&e))?;
        if result.error.is_some() {
            return Err("model_load_failed".into());
        }
    }
    let mut recognizer = analysis::AnalysisClient::start(&worker, "recognize", &directory)
        .map_err(|_| "model_worker_missing")?
        .managed(&resources, resources::Work::Coarse);
    let _: String = recognizer
        .request(
            "microphone",
            "model-load-check",
            0,
            &samples,
            true,
            Duration::from_secs(30),
            &cancelled,
        )
        .map_err(|e| model_pack::load_error_code(&e))?;
    Ok(())
}

fn analysis_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let worker_path = match std::env::current_exe()
        .ok()
        .map(|path| path.with_file_name(loppi_protocol::platform::executable("loppi-worker")))
    {
        Some(path) => path,
        None => return,
    };
    let model_dir = shared.model_assets.speech_directory();
    let mut worker: Option<analysis::AnalysisClient> = None;
    while !stop.load(Ordering::Acquire) {
        if worker
            .as_ref()
            .is_some_and(|w| w.idle_for() > Duration::from_secs(60))
        {
            worker = None;
        }
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
        let mut stage = "local_analysis_decode";
        let result = (|| -> Result<analysis::VadResult, String> {
            let pcm = decode_saved_block(&shared.directory, &block)?;
            if worker.is_none() {
                stage = "local_analysis_worker_start";
                worker = Some(analysis::AnalysisClient::start(
                    &worker_path,
                    "vad",
                    &model_dir,
                )?);
            }
            if worker.as_ref().unwrap().needs_replay(
                &block.source,
                &block.session_id,
                block.sample_start,
            ) {
                stage = "local_analysis_replay_context";
                let context = shared
                    .store
                    .lock()
                    .map_err(|error| error.to_string())?
                    .replay_context(&block)?;
                for prior in context {
                    stage = "local_analysis_replay_decode";
                    let prior_pcm = decode_saved_block(&shared.directory, &prior)?;
                    stage = "local_analysis_replay_worker";
                    let replay = worker.as_mut().unwrap().analyze(
                        &prior.source,
                        &prior.session_id,
                        prior.sample_start,
                        &prior_pcm,
                        false,
                    )?;
                    if replay.error.is_some() {
                        return Err("worker rejected replay audio".to_owned());
                    }
                    // Replay can close speech that had not yet been published
                    // when the worker died. Keep that suffix through the same
                    // durable dedupe and cleanup boundary as current results.
                    stage = "local_analysis_replay_publish";
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    let tx = store.db.transaction().map_err(|e| e.to_string())?;
                    coarse::enqueue(&tx, &prior, replay.segments)?;
                    tx.commit().map_err(|e| e.to_string())?;
                }
            }
            stage = "local_analysis_worker";
            worker.as_mut().unwrap().analyze(
                &block.source,
                &block.session_id,
                block.sample_start,
                &pcm,
                block.flush,
            )
        })();
        let published = result.and_then(|result| {
            stage = "local_analysis_publish";
            shared
                .store
                .lock()
                .map_err(|error| error.to_string())?
                .publish_analysis(&block, result)
        });
        let success = published.is_ok();
        if !success {
            worker = None;
            if let Ok(mut store) = shared.store.lock() {
                let _ = store.analysis_failed(&block.id);
            }
        }
        let event = vad_log_event(&block, &published, stage);
        shared
            .logs
            .emit(ClientKind::Worker, uuid::Uuid::new_v4(), event);
    }
}

fn vad_log_event(block: &PendingBlock, published: &Result<bool, String>, stage: &str) -> LogEvent {
    let success = published.is_ok();
    let mut event = LogEvent::simple(
        "speech",
        if success {
            LogLevel::Info
        } else {
            LogLevel::Warn
        },
        if success {
            "vad_complete"
        } else {
            "vad_failed"
        },
    );
    event.context = Some(loppi_protocol::LogContext {
        start_ms: Some(block.started_ms),
        end_ms: Some(block.ended_ms),
        source: Some(if block.source == "microphone" {
            AudioSource::Microphone
        } else {
            AudioSource::System
        }),
        ..Default::default()
    });
    event.capture_span_id = uuid::Uuid::parse_str(&block.session_id).ok();
    event.job_id = uuid::Uuid::parse_str(&block.id).ok();
    if let Err(error) = published {
        event.error_code = Some(analysis::failure_code(stage, error));
    }
    event
}

fn cloud_loop(
    shared: Arc<CoreState>,
    stop: Arc<AtomicBool>,
    provider: loppi_protocol::CloudProvider,
) {
    let mut turn = 0u64;
    let mut last_refresh = Instant::now() - Duration::from_secs(30);
    while !stop.load(Ordering::Acquire) {
        let now = match now_ms() {
            Ok(now) => now,
            Err(_) => {
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        if provider == loppi_protocol::CloudProvider::Llm
            && last_refresh.elapsed() >= Duration::from_secs(15)
        {
            if let Ok(mut store) = shared.store.lock() {
                if events::refresh_candidates(&mut store.db, now, false).is_err() {
                    let mut event =
                        LogEvent::simple("events", LogLevel::Warn, "candidate_refresh_failed");
                    event.error_code = Some("candidate_refresh_failed".into());
                    shared
                        .logs
                        .emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
                }
            }
            last_refresh = Instant::now();
        }
        let work = if provider == loppi_protocol::CloudProvider::Llm {
            resources::Work::Llm
        } else {
            resources::Work::Asr
        };
        let Some(_permit) = shared.resources.try_admit(work) else {
            thread::sleep(Duration::from_millis(500));
            continue;
        };
        let task = shared
            .store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut store| events::take_task_fair(&mut store.db, provider, now, turn));
        let task = match task {
            Ok(Some(task)) => task,
            Ok(None) => {
                drop(_permit);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            Err(_) => {
                drop(_permit);
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        turn = turn.wrapping_add(1);
        let task_id = task.id;
        let started = Instant::now();
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
        event.duration_ms = Some(started.elapsed().as_millis() as u64);
        if task.target_kind == "event" {
            event.event_id = uuid::Uuid::parse_str(&task.target_id).ok();
        }
        let mut context = outcome
            .as_ref()
            .err()
            .map(|e| e.context.clone())
            .unwrap_or_default();
        context.start_ms = Some(task.start);
        context.end_ms = Some(task.end);
        context.previous_failures = Some(task.attempts);
        context.provider = Some(task.provider);
        if let Err(error) = &outcome {
            let candidate = LogEvent {
                error_code: Some(error.code.clone()),
                ..event.clone()
            };
            event.error_code = Some(if unilogs::valid_event(&candidate) {
                error.code.clone()
            } else {
                "processing_failed".into()
            });
            context.retry_after_ms = error.retry_after_ms;
        }
        event.context = Some(context);
        shared.logs.emit(ClientKind::Worker, task_id, event);
        if let Err(error) = outcome {
            // Account failures were committed while their credential gate was
            // held. Reapplying one here could race an explicit ASR retry, whose
            // new state is already submitted, or a replacement configuration.
            if error.code == "provider_http_402" {
                continue;
            }
            if let Ok(store) = shared.store.lock() {
                let _ = events::task_failed_with_retry(
                    &store.db,
                    &task,
                    &error.code,
                    error.retry_after_ms,
                    now_ms().unwrap_or(now),
                );
            }
        }
    }
}

#[cfg(test)]
fn run_voice_worker_with_timeout(
    worker: &Path,
    model_path: &Path,
    samples: &[f32],
    timeout: Duration,
) -> Result<Vec<f32>, String> {
    let mut worker = analysis::AnalysisClient::start(worker, "embed", model_path)?;
    worker.request("microphone", "test", 0, samples, true, timeout, &|| false)
}

fn voice_error_code(error: &str) -> &'static str {
    if error.starts_with("audio block unavailable: ") {
        return "audio_unavailable";
    }
    match error {
        "speaker audio unavailable" | "ASR source audio unavailable" => "audio_unavailable",
        "speaker audio range unsuitable" | "speaker audio level unsuitable" => "audio_quality",
        "worker_timeout" => "worker_timeout",
        "resource_deferred" => "resource_deferred",
        "model_invalid" => "model_invalid",
        _ => "worker_failed",
    }
}

fn voice_loop(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let mut worker: Option<analysis::AnalysisClient> = None;
    let mut matcher = knowledge::VoiceMatcher::default();
    while !stop.load(Ordering::Acquire) {
        if worker.as_ref().is_some_and(|w| {
            w.idle_for() > Duration::from_secs(30)
                || shared.resources.unload(resources::Work::Voice)
        }) {
            worker = None;
        }
        if shared.resources.mode() == resources::Mode::Critical {
            shared.resources.no_work(resources::Work::Voice);
            matcher.clear();
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        // Matching saved vectors is a database operation. An unavailable
        // model must not hide identities after an upgrade or sample change.
        let now = now_ms().unwrap_or(0);
        if shared.resources.unload(resources::Work::Voice) {
            matcher.clear();
        }
        let enrolled = (|| -> Result<bool, String> {
            let input =
                knowledge::prepare_enrollment(&shared.store.lock().map_err(|e| e.to_string())?.db)?;
            let Some(input) = input else {
                return Ok(false);
            };
            let allowed = matcher.allows_enrollment(&input.matching, &input.person, |after| {
                knowledge::match_sample_page(
                    &shared.store.lock().map_err(|e| e.to_string())?.db,
                    &input.matching,
                    after,
                )
            })?;
            knowledge::publish_enrollment(
                &mut shared.store.lock().map_err(|e| e.to_string())?.db,
                &input,
                allowed,
                now,
            )
        })();
        match enrolled {
            Ok(true) => {
                shared.resources.no_work(resources::Work::Voice);
                continue;
            }
            Err(_) => {
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            Ok(false) => {}
        }
        let matched = (|| -> Result<bool, String> {
            let input =
                knowledge::prepare_match(&shared.store.lock().map_err(|e| e.to_string())?.db)?;
            let Some(input) = input else {
                return Ok(false);
            };
            let selected = matcher.select(&input, |after| {
                knowledge::match_sample_page(
                    &shared.store.lock().map_err(|e| e.to_string())?.db,
                    &input,
                    after,
                )
            })?;
            knowledge::publish_match(
                &mut shared.store.lock().map_err(|e| e.to_string())?.db,
                &input,
                selected,
                now,
            )
        })();
        match matched {
            Ok(true) => {
                shared.resources.no_work(resources::Work::Voice);
                continue;
            }
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
            shared.resources.no_work(resources::Work::Voice);
            thread::sleep(Duration::from_secs(2));
            continue;
        }
        let available=shared.store.lock().ok().and_then(|s|s.db.query_row("SELECT EXISTS(SELECT 1 FROM voice_observations WHERE state='pending' AND available_ms<=?1 UNION ALL SELECT 1 FROM voice_enrollments WHERE state='pending' AND vector IS NULL AND available_ms<=?1)",[now],|r|r.get::<_,bool>(0)).ok()).unwrap_or(false);
        if !available {
            shared.resources.no_work(resources::Work::Voice);
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        let Some(_permit) = shared.resources.try_admit(resources::Work::Voice) else {
            thread::sleep(Duration::from_millis(500));
            continue;
        };
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
                        work.audio_ranges()
                            .into_iter()
                            .map(|(start, end)| {
                                events::voice_manifest(&store.db, &work.source, start, end)
                            })
                            .collect::<Result<Vec<_>, String>>()
                    });
                let mut voice_context = loppi_protocol::LogContext {
                    source: Some(if work.source == "microphone" {
                        AudioSource::Microphone
                    } else {
                        AudioSource::System
                    }),
                    start_ms: Some(work.start),
                    end_ms: Some(work.end),
                    ..Default::default()
                };
                let result: Result<(Vec<f32>, Option<knowledge::VoiceQuality>), String> = manifest
                    .and_then(|manifests| {
                        events::voice_input_16k(
                            &shared.directory,
                            &manifests,
                            work.enrollment.is_some(),
                            &mut voice_context,
                        )
                    })
                    .map_err(|error| {
                        // Preparation never invoked the model. A decode or
                        // manifest failure means this input is unavailable,
                        // regardless of a decoder's free-form wording.
                        if voice_error_code(&error) == "audio_quality" {
                            "speaker audio level unsuitable".to_owned()
                        } else {
                            "speaker audio unavailable".to_owned()
                        }
                    })
                    .and_then(|pcm| {
                        let quality = knowledge::enrollment_quality(&pcm);
                        if work.enrollment.is_some() && quality.is_none() {
                            return Err("speaker audio level unsuitable".into());
                        }
                        if worker.is_none() {
                            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                            worker = Some(
                                analysis::AnalysisClient::start(
                                    &exe.with_file_name(loppi_protocol::platform::executable(
                                        "loppi-worker",
                                    )),
                                    "embed",
                                    &shared.model_assets.speaker_file(),
                                )?
                                .managed(&shared.resources, resources::Work::Voice),
                            );
                        }
                        let response = worker.as_mut().unwrap().request(
                            "microphone",
                            "voice",
                            0,
                            &pcm,
                            true,
                            Duration::from_secs(30),
                            &|| {
                                stop.load(Ordering::Acquire)
                                    || shared.resources.cancel(resources::Work::Voice)
                            },
                        );
                        // Only a failed model/protocol operation retires a
                        // healthy resident. Input failures bypass this branch.
                        if response.is_err() {
                            worker = None;
                        }
                        response.map(|vector| (vector, quality))
                    });
                let mut event = LogEvent::simple("identity", LogLevel::Info, "voice_processed");
                event.context = Some(voice_context);
                event.job_id = Some(work.segment_id);
                match result {
                    Ok((vector, quality)) => {
                        if let Ok(mut store) = shared.store.lock() {
                            if knowledge::publish_voice(
                                &mut store.db,
                                &work,
                                &vector,
                                quality,
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
                        event.level = if code == "resource_deferred" {
                            LogLevel::Info
                        } else {
                            LogLevel::Warn
                        };
                        event.error_code = Some(code.to_owned());
                    }
                }
                shared.logs.emit(ClientKind::Worker, work.segment_id, event);
            }
            Ok(None) => {
                drop(_permit);
                thread::sleep(Duration::from_secs(1));
            }
            Err(_) => {
                drop(_permit);
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn process_cloud_task(
    shared: &CoreState,
    task: &events::CloudTask,
    now: i64,
) -> Result<(), cloud::Failure> {
    let deadline = Instant::now()
        + Duration::from_secs(if task.provider == loppi_protocol::CloudProvider::Llm {
            15 * 60
        } else {
            60
        });
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
        return Err("provider_not_configured".to_owned().into());
    }
    match task.provider {
        loppi_protocol::CloudProvider::Llm => {
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
            if task.target_kind == "screenshot" {
                // Cleanup shares this outbound gate, so the image remains owned
                // until upload finishes. Keep file IO outside the Store lock.
                let png = {
                    let db = shared.readers.snapshot()?;
                    moments::image(
                        &db,
                        &shared.directory,
                        uuid::Uuid::parse_str(&task.target_id)
                            .map_err(|_| "invalid_screenshot_task".to_owned())?,
                    )?
                };
                let key = active_cloud_credential(shared, task.provider)?;
                let meter = usage::Recorder::task(shared, task.provider, task.id)?;
                let output = cloud::describe_image(&key, &task.model_id, &png, deadline, &meter)
                    .map_err(|error| record_account_failure(shared, task, error))?;
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                moments::publish_image(&mut store.db, task, output, now_ms()?)?;
                return Ok(());
            }
            if task.target_kind == "workflow" {
                let key = active_cloud_credential(shared, task.provider)?;
                let meter = usage::Recorder::task(shared, task.provider, task.id)?;
                let ((system, input), attempt) = {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    if !events::task_current(&store.db, task)? {
                        events::supersede_task(&store.db, task, now)?;
                        return Ok(());
                    }
                    let prompt = events::workflow::task_input(&store.db, task)?;
                    let attempt = store.db.query_row(
                        "UPDATE cloud_tasks SET submit_attempts=submit_attempts+1 WHERE id=?1 AND state='processing' RETURNING submit_attempts",
                        [task.id.to_string()],
                        |row| row.get(0),
                    ).map_err(|error| error.to_string())?;
                    (prompt, attempt)
                };
                let preview = shared.workflow_output.start(task.id, attempt)?;
                let output = cloud::chat_markdown(
                    &key, &task.model_id, &system, &input, deadline, &meter,
                    &mut |body| preview.update(body),
                ).map_err(|error| record_account_failure(shared, task, error))?;
                drop(outbound);
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                events::workflow::publish(&mut store.db, task, &output, now_ms()?)?;
                return Ok(());
            }
            // Capture prior-event summaries under the same gate as cleanup,
            // including context outside this candidate's own time range.
            let input = {
                let store = shared.store.lock().map_err(|e| e.to_string())?;
                events::llm_input(&store.db, task)?
            };
            let key = active_cloud_credential(shared, task.provider)?;
            let meter = usage::Recorder::task(shared, task.provider, task.id)?;
            let base_system = if task.target_kind == "event" {
                events::rebuild::SYSTEM
            } else {
                events::FIT_SYSTEM
            };
            let mut request: serde_json::Value =
                serde_json::from_str(&input).map_err(|_| "invalid_fit_input".to_owned())?;
            let system = events::workflow::classification_system(base_system, &request);
            if let Some(object) = request.as_object_mut() {
                object.remove("classification");
            }
            let provider_input =
                serde_json::to_string(&request).map_err(|error| error.to_string())?;
            let output = cloud::chat(
                &key,
                &task.model_id,
                &system,
                &provider_input,
                deadline,
                &meter,
            )
            .map_err(|error| record_account_failure(shared, task, error))?;
            drop(outbound);
            let _event_gate = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
            let count = {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                events::publish_fit_with_input(&mut store.db, task, &input, output, now_ms()?)?
            };
            let mut event = LogEvent::simple("events", LogLevel::Info, "fit_published");
            event.job_id = Some(task.id);
            event.context = Some(loppi_protocol::LogContext {
                count: Some(count as u64),
                ..Default::default()
            });
            shared.logs.emit(ClientKind::Worker, task.id, event);
            Ok(())
        }
        loppi_protocol::CloudProvider::Asr => {
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
                let meter = usage::Recorder::task(shared, task.provider, task.id)?;
                cloud::query_audio(&key, task.remote_id, deadline, &meter)
                    .map_err(|error| record_account_failure(shared, task, error))?
            };
            match query {
                cloud::AsrQuery::Complete(utterances) => {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::publish_asr(&mut store.db, task, &utterances, now_ms()?)?;
                    Ok(())
                }
                cloud::AsrQuery::Processing => {
                    if task.submit_attempts > 0 && now - task.updated_ms > 2 * 60 * 60 * 1000 {
                        return Err("provider_processing_timeout".to_owned().into());
                    }
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::asr_wait(&store.db, task, now_ms()?).map_err(Into::into)
                }
                cloud::AsrQuery::Missing => {
                    if task.submit_attempts > 0 && now - task.updated_ms < 60_000 {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        return events::asr_wait(&store.db, task, now_ms()?).map_err(Into::into);
                    }
                    if task.submit_attempts >= 3 {
                        return Err("provider_missing_submitted_task".to_owned().into());
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
                    let input = {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::asr::input(&store.db, task.id)?
                    };
                    let (ogg, audio_duration) = if let Some(input) = input {
                        (
                            events::asr::assemble(
                                &shared.directory,
                                &events::asr::resolve_media(&input, &parts)?,
                            )?,
                            input.duration_ms,
                        )
                    } else {
                        (
                            events::assemble_ogg(&shared.directory, task, &parts)?,
                            task.end - task.start,
                        )
                    };
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
                    let key = active_cloud_credential(shared, task.provider)?;
                    cloud::remaining(deadline)?;
                    {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        events::mark_submit_intent(&store.db, task, now_ms()?)?;
                    }
                    let vocabulary = {
                        let store = shared.store.lock().map_err(|e| e.to_string())?;
                        vocabulary::submission_context(&store.db, task.id)?
                    };
                    let meter = usage::Recorder::task(shared, task.provider, task.id)?;
                    cloud::submit_audio(
                        &key,
                        task.remote_id,
                        &ogg,
                        cloud::AsrAudioFormat::Ogg,
                        &vocabulary,
                        audio_duration,
                        deadline,
                        &meter,
                    )
                    .map_err(|error| record_account_failure(shared, task, error))?;
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    events::asr_wait(&store.db, task, now_ms()?).map_err(Into::into)
                }
            }
        }
    }
}

// Call only while holding this provider's outbound gate. Configuration swaps
// and retires Keychain references under the same gate.
fn record_account_failure(
    shared: &CoreState,
    task: &events::CloudTask,
    mut error: cloud::Failure,
) -> cloud::Failure {
    if error.code == "provider_http_402" {
        // Commit the account fact before releasing the credential's gate. A
        // newly verified configuration can then clear it without a late old
        // response blocking the replacement account. The outer task handler
        // logs this outcome without applying a second account transition.
        let saved = shared.store.lock().map_err(|_| ()).and_then(|store| {
            events::task_failed_with_retry(
                &store.db,
                task,
                &error.code,
                error.retry_after_ms,
                now_ms().unwrap_or(task.updated_ms),
            )
            .map_err(|_| ())
        });
        if saved.is_err() {
            // A storage failure must not later apply this old account fact
            // outside the gate. The common handler still records the failure.
            error.code = "cloud_failure_persist_failed".into();
        }
    }
    error
}

// Call only while holding this provider's outbound gate. Configuration swaps
// and retires Keychain references under the same gate.
fn active_cloud_credential(
    shared: &CoreState,
    provider: loppi_protocol::CloudProvider,
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
    provider: loppi_protocol::CloudProvider,
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
    slept: bool,
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
        let start_result = source_shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut store| {
                store.start_recording_session(session_id, source, source_shared.run_id, clock)
            });
        let result = start_result.and_then(|()| {
            let session = ingestion::session(
                &source_shared.store.lock().map_err(|e| e.to_string())?.db,
                session_id,
            )?;
            source_process::supervise(
                &source_shared.directory,
                session,
                None,
                &source_stop,
                &source_frames,
                || {
                    source_shared
                        .resources
                        .capture_progress(source_name(source));
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
                |pid, progress| {
                    source_shared.resources.capture_observed(
                        source_name(source),
                        session_id,
                        pid,
                        progress,
                    );
                },
            )
        });
        source_shared.resources.capture_stopped(source_name(source));
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
        let capture_metrics = source_shared
            .resources
            .capture_metrics(source_name(source), session_id);
        let capacity = source_shared.store.lock().ok().and_then(|s| s.state().ok());
        event.context = Some(loppi_protocol::LogContext {
            source: Some(source),
            main_display_asleep: system_audio::main_display_asleep(),
            captured_frames: Some(source_frames.load(Ordering::Acquire)),
            clock_delta_ms: result
                .as_ref()
                .err()
                .and_then(|error| source_process::clock_delta_ms(error)),
            dropped_frames: capture_metrics.as_ref().map(|m| m.dropped_frames),
            queued_ms: capture_metrics.as_ref().map(|m| m.queued_ms),
            used_bytes: capacity.as_ref().map(|s| s.audio_used_bytes),
            limit_bytes: capacity.as_ref().map(|s| s.audio_limit_bytes),
            free_bytes: fs2::available_space(&source_shared.directory).ok(),
            ..Default::default()
        });
        if let Err(error) = &result {
            event.error_code = Some(source_process::error_code(error));
        }
        source_shared
            .logs
            .emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
    });
    SourceSlot {
        session_id,
        slept: false,
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
    let mut previous_power = resources::power();
    while !stop.load(Ordering::Acquire) {
        let wall = SystemTime::now();
        let monotonic = Instant::now();
        let clock_gap = wall
            .duration_since(previous_wall)
            .ok()
            .zip(Some(monotonic.duration_since(previous_monotonic)))
            .is_some_and(|(wall_elapsed, monotonic_elapsed)| {
                wall_elapsed > monotonic_elapsed + Duration::from_secs(2)
            });
        let power = resources::power();
        let suspended = power.available != 0 && power.sleeping != 0;
        let slept = if power.available != 0 {
            power.generation != previous_power.generation
        } else {
            clock_gap
        };
        previous_power = power;
        previous_wall = wall;
        previous_monotonic = monotonic;

        let state = shared
            .store
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut store| {
                if shared.normal_run.load(Ordering::Acquire)
                    && store.settle_schedule().unwrap_or(false)
                {
                    shared.allow_capture.store(true, Ordering::Release);
                }
                if slept && power.available != 0 && power.sleep_ms > 0 {
                    // A helper may already have failed and left its slot
                    // before sleep. Its durable tail gap still owns this
                    // boundary, even while that source is only in backoff.
                    store.record_open_sleep_gaps(power.sleep_ms)?;
                }
                Ok((store.state()?, events::configured(&store.db)?))
            });
        let models_ready = shared
            .models
            .lock()
            .is_ok_and(|models| matches!(models.phase, ModelPhase::Ready));
        let capture_allowed = shared.allow_capture.load(Ordering::Acquire)
            && models_ready
            && state.as_ref().is_ok_and(|(state, configured)| {
                *configured && state.enabled_intent && !state.paused_intent && !state.storage_paused
            });
        let (want_microphone, want_system) = match state {
            Ok((state, _)) => (
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
            let should_stop = suspended || slept || !wanted;
            if let Some(current) = slot.as_mut() {
                if should_stop {
                    current.stop.store(true, Ordering::Release);
                }
                current.slept |= suspended || slept;
            }
            // A stopping HAL source must not delay control of the other source.
            let finished = slot
                .as_ref()
                .is_some_and(|current| current.join.is_finished());
            if finished {
                let current = slot.take().unwrap();
                let ended_unexpectedly = !current.stop.load(Ordering::Acquire);
                let _ = current.join.join();
                if current.slept {
                    if let Ok(mut store) = shared.store.lock() {
                        let _ = if power.available != 0 && power.sleep_ms > 0 {
                            store.record_sleep_gap(current.session_id, power.sleep_ms)
                        } else {
                            store.record_gap_after_session(current.session_id, "sleep_or_clock_gap")
                        };
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
            if !wanted && (*was_wanted || finished) {
                reset_capture_retry(retry_at, retry_delay);
                if let Ok(mut store) = shared.store.lock() {
                    let _ = store.close_open_gaps(source);
                }
            }
            *was_wanted = wanted;
            if wanted
                && slot.is_none()
                && !slept
                && !suspended
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
    for slot in [&microphone, &system].into_iter().flatten() {
        slot.stop.store(true, Ordering::Release);
    }
    for slot in [microphone, system].into_iter().flatten() {
        let _ = slot.join.join();
    }
    if let Ok(mut store) = shared.store.lock() {
        let _ = store.close_open_gaps(AudioSource::Microphone);
        let _ = store.close_open_gaps(AudioSource::System);
    }
}

struct CoreState {
    readers: reads::Pool,
    store: Mutex<Store>,
    llm_outbound: Mutex<()>,
    workflow_output: events::workflow::LiveOutput,
    asr_outbound: Mutex<()>,
    voice_outbound: Mutex<()>,
    cleanup_gate: Mutex<()>,
    logs: LogWorker,
    launch_mode: LaunchMode,
    run_id: uuid::Uuid,
    directory: std::path::PathBuf,
    models: Arc<Mutex<ModelState>>,
    model_assets: model_pack::Assets,
    microphone_busy: AtomicBool,
    system_busy: AtomicBool,
    capture: Mutex<CaptureRuntime>,
    allow_capture: AtomicBool,
    // A live GUI may arm future actions without starting current capture.
    normal_run: AtomicBool,
    resources: Arc<resources::Governor>,
}

fn open_intent_gap(
    tx: &Transaction<'_>,
    source: &str,
    reason: &str,
    at: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason,source_session_id)
         SELECT ?1,?2,max(?3,coalesce(s.last_confirmed_ms,?3)),NULL,?4,s.id
         FROM service_state u LEFT JOIN recording_sessions s ON s.id=(
           SELECT id FROM recording_sessions WHERE source=?2 AND state='open'
           ORDER BY started_ms DESC,id LIMIT 1)
         WHERE u.id=1 AND u.enabled_intent=1 AND
           ((?2='microphone' AND u.microphone_enabled=1) OR (?2='system' AND u.system_enabled=1))
         AND NOT EXISTS(
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
    fn prepare_gui_playback(&self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let active = self
                .capture
                .lock()
                .map_err(|e| e.to_string())?
                .system_active;
            if !active && !self.system_busy.load(Ordering::Acquire) {
                return Ok(());
            }
            let generation = system_audio::playback_exclusion_generation();
            if self.resources.playback_excluded(generation) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("playback_exclusion_unavailable".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn outbound_gate(&self, provider: loppi_protocol::CloudProvider) -> &Mutex<()> {
        match provider {
            loppi_protocol::CloudProvider::Llm => &self.llm_outbound,
            loppi_protocol::CloudProvider::Asr => &self.asr_outbound,
        }
    }
    fn service_state(&self) -> Result<ServiceState, String> {
        let store = self.store.lock().map_err(|error| error.to_string())?;
        let mut state = store.state()?;
        let cloud_ready = events::configured(&store.db)?;
        drop(store);
        state.disk_free_bytes = fs2::available_space(&self.directory).unwrap_or(0);
        state.storage_recoverable = state.storage_paused
            && storage_has_capacity(
                state.audio_used_bytes,
                state.audio_limit_bytes,
                state.disk_free_bytes,
            );
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
            state.reason = Some(
                if state.recording_schedule.paused_by_schedule {
                    "recording was paused by schedule"
                } else {
                    "recording was paused by the user"
                }
                .to_owned(),
            );
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
        diagnostics::apply(
            &mut state,
            models_ready,
            cloud_ready,
            self.allow_capture.load(Ordering::Acquire),
            self.launch_mode == LaunchMode::Login,
            false,
            self.microphone_busy.load(Ordering::Acquire),
            self.system_busy.load(Ordering::Acquire),
        );
        Ok(state)
    }
}

enum LogCommand {
    Emit(ClientKind, uuid::Uuid, LogEvent),
    Window(
        LogFilter,
        mpsc::Sender<Result<loppi_protocol::LogWindow, String>>,
    ),
    Read(
        LogFilter,
        mpsc::Sender<Result<Vec<loppi_protocol::LogRecord>, String>>,
    ),
}

#[derive(Clone)]
struct LogWorker {
    sender: SyncSender<LogCommand>,
    dropped: Arc<AtomicU64>,
}

impl LogWorker {
    fn start(directory: std::path::PathBuf, run_id: uuid::Uuid) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1024);
        let dropped = Arc::new(AtomicU64::new(0));
        let pending_drops = Arc::clone(&dropped);
        thread::spawn(move || {
            let mut logs = match unilogs::UniLogs::open(&directory, run_id) {
                Ok(logs) => Some(logs),
                Err(error) => {
                    eprintln!("loppi-core unilogs unavailable: {error}");
                    None
                }
            };
            if let Some(logs) = logs.as_mut() {
                let mut event = LogEvent::simple("core", LogLevel::Info, "started");
                event.build = Some(loppi_protocol::build_info("core"));
                logs.emit(ClientKind::Core, uuid::Uuid::new_v4(), event);
            }
            loop {
                let command = match receiver.recv_timeout(Duration::from_secs(60)) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(logs) = logs.as_mut() {
                            logs.flush_summary();
                        }
                        continue;
                    }
                    Err(_) => {
                        if let Some(logs) = logs.as_mut() {
                            logs.flush_summary();
                        }
                        break;
                    }
                };
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
                    LogCommand::Window(filter, reply) => {
                        let result = logs
                            .as_mut()
                            .ok_or_else(|| "logs unavailable".to_owned())
                            .and_then(|logs| {
                                logs.flush_summary();
                                logs.window(&filter)
                            });
                        let _ = reply.send(result);
                    }
                    LogCommand::Read(filter, reply) => {
                        let result = logs
                            .as_mut()
                            .ok_or_else(|| "diagnostic logs are unavailable".to_owned())
                            .and_then(|logs| {
                                logs.flush_summary();
                                logs.read(&filter)
                            });
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

    fn window(&self, filter: LogFilter) -> Result<loppi_protocol::LogWindow, String> {
        let (reply, receiver) = mpsc::channel();
        self.sender
            .try_send(LogCommand::Window(filter, reply))
            .map_err(|_| "logs busy".to_owned())?;
        receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "logs timed out".to_owned())?
    }

    fn read(&self, filter: LogFilter) -> Result<Vec<loppi_protocol::LogRecord>, String> {
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
    if !direct {
        if source == AudioSource::System
            && system_audio::tap_available()
            && !system_audio::tap_ready()
        {
            return Err("system audio Tap requires foreground authorization".into());
        }
        return system_audio::probe_child(source, seconds, Duration::from_secs(seconds as u64 + 8));
    }
    match source {
        AudioSource::Microphone => microphone::Microphone::probe(seconds),
        AudioSource::System => {
            unsafe extern "C" {
                fn loppi_system_audio_probe(seconds: u32, peak_millionths_out: *mut u64) -> i64;
            }
            let mut peak_millionths = 0;
            let frames = unsafe { loppi_system_audio_probe(seconds as u32, &mut peak_millionths) };
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
                channels: if cfg!(windows) { 2 } else { 1 },
                frames: frames as u64,
                peak: peak_millionths as f32 / 1_000_000.0,
                dropped_packets: 0,
            })
        }
    }
}

fn acquire_lock(directory: &Path) -> Result<Option<File>, String> {
    acquire_named_lock(&directory.join("core.lock"))
}

fn acquire_named_lock(path: &Path) -> Result<Option<File>, String> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    match loppi_protocol::platform::try_lock_exclusive(&lock) {
        Ok(()) => Ok(Some(lock)),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn acquire_owner_lock(directory: &Path, mode: LaunchMode) -> Result<Option<File>, String> {
    // Serialize contenders before asking the current owner to leave. Keep this
    // separate from lifecycle.lock: the GUI holds that lock while awaiting its
    // child Core. The running owner never needs this short-lived startup gate.
    let deadline = Instant::now() + Duration::from_secs(10);
    let _startup = loop {
        if let Some(gate) = acquire_named_lock(&directory.join("core-start.lock"))? {
            break gate;
        }
        if mode == LaunchMode::OnDemand {
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Err("login item could not acquire the startup gate".to_owned());
        }
        thread::sleep(Duration::from_millis(10));
    };
    if mode == LaunchMode::OnDemand {
        return acquire_lock(directory);
    }
    loop {
        if let Some(lock) = acquire_lock(directory)? {
            return Ok(Some(lock));
        }
        if let Ok(response) = loppi_protocol::send_existing(ClientKind::Core, Request::YieldToLogin)
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

#[cfg(unix)]
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

#[cfg(unix)]
fn verify_peer(stream: &UnixStream) -> Result<(), String> {
    #[cfg(unix)]
    use std::os::fd::AsRawFd;
    let mut uid = 0;
    let mut gid = 0;
    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if result != 0 || uid != unsafe { libc::geteuid() } {
        return Err("client user differs from loppi-core user".to_owned());
    }
    Ok(())
}

#[cfg(unix)]
fn peer_pid(stream: &UnixStream) -> Result<i32, String> {
    #[cfg(unix)]
    use std::os::fd::AsRawFd;
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of_val(&pid) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut length,
        )
    };
    if result != 0 || pid <= 0 {
        return Err("cannot verify screenshot caller".into());
    }
    Ok(pid)
}

#[cfg(unix)]
#[repr(C)]
struct AuditInfo {
    user: u32,
    success_mask: u32,
    failure_mask: u32,
    terminal_port: i32,
    terminal_machine: u32,
    session: i32,
}

#[cfg(unix)]
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

fn read_response(id: uuid::Uuid, result: Result<Response, String>) -> Response {
    result.unwrap_or_else(|message| Response::Error {
        id,
        code: reads::error_code(&message).into(),
        message,
    })
}

fn ipc_failure(shared: &CoreState, id: Option<uuid::Uuid>, command: Option<&str>, code: &str) {
    shared.logs.emit(
        ClientKind::Core,
        id.unwrap_or_else(uuid::Uuid::new_v4),
        ipc_failure_event(id, command, code),
    );
}

fn ipc_failure_event(id: Option<uuid::Uuid>, command: Option<&str>, code: &str) -> LogEvent {
    let cancelled =
        matches!(command, Some("app_opened" | "status")) && code == "ipc_write_disconnected";
    let mut event = LogEvent::simple(
        "ipc",
        if cancelled {
            LogLevel::Info
        } else {
            LogLevel::Warn
        },
        if cancelled {
            "request_cancelled"
        } else {
            "request_failed"
        },
    );
    event.error_code = Some(code.to_owned());
    event.correlation_id = id;
    event.context = command.map(|command| loppi_protocol::LogContext {
        command: Some(command.into()),
        cause: Some(code.into()),
        ..Default::default()
    });
    event
}

fn ipc_io_code(error: &std::io::Error, writing: bool) -> &'static str {
    use std::io::ErrorKind::*;
    match (writing, error.kind()) {
        (false, TimedOut | WouldBlock) => "ipc_read_timeout",
        (true, TimedOut | WouldBlock) => "ipc_write_timeout",
        (
            false,
            BrokenPipe | ConnectionReset | ConnectionAborted | NotConnected | UnexpectedEof,
        ) => "ipc_read_disconnected",
        (true, BrokenPipe | ConnectionReset | ConnectionAborted | NotConnected | UnexpectedEof) => {
            "ipc_write_disconnected"
        }
        (false, _) => "ipc_read_failed",
        (true, _) => "ipc_write_failed",
    }
}

fn serve_one(stream: &mut UnixStream, shared: &Arc<CoreState>) -> Result<bool, String> {
    let input = read_request(stream).inspect_err(|code| ipc_failure(shared, None, None, code))?;
    if input.is_empty() {
        return Ok(false);
    }
    let envelope: Envelope = serde_json::from_slice(&input).map_err(|_| {
        ipc_failure(shared, None, None, "ipc_invalid_json");
        "ipc_invalid_json".to_owned()
    })?;
    let id = envelope.id;
    let command = envelope.request.name();
    let client = envelope.client;
    let started = Instant::now();
    let (body, quit) = match handle_request(stream, shared, envelope) {
        Ok(result) => result,
        Err(error) => {
            let mut event = LogEvent::simple("ipc", LogLevel::Warn, "request_failed");
            event.error_code = Some("ipc_execute_failed".into());
            event.correlation_id = Some(id);
            event.duration_ms = Some(started.elapsed().as_millis() as u64);
            event.context = Some(loppi_protocol::LogContext {
                command: Some(command.into()),
                cause: Some(reads::failure_cause(&error).into()),
                ..Default::default()
            });
            shared.logs.emit(client, id, event);
            // A valid envelope always receives a correlated error, including
            // failures that propagated with `?` before building a Response.
            (
                serde_json::to_vec(&read_response(id, Err(error))).map_err(|e| e.to_string())?,
                false,
            )
        }
    };
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .and_then(|()| stream.write_all(&body))
        .map_err(|error| {
            let code = ipc_io_code(&error, true);
            ipc_failure(shared, Some(id), Some(command), code);
            code.to_owned()
        })?;
    Ok(quit)
}

fn handle_request(
    stream: &UnixStream,
    shared: &Arc<CoreState>,
    envelope: Envelope,
) -> Result<(Vec<u8>, bool), String> {
    // Store only a digest: ConfigureCloud contains a user-supplied secret.
    let mutation_fingerprint = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&envelope.request).map_err(|error| error.to_string())?)
    );
    let request_started = Instant::now();
    let explicit_quit = matches!(
        envelope.request,
        Request::Quit
            | Request::Lifecycle {
                action: LifecycleAction::Quit
            }
    );
    let quit = explicit_quit
        || matches!(
            envelope.request,
            Request::Lifecycle {
                action: LifecycleAction::Handoff
            }
        )
        || matches!(envelope.request, Request::YieldToLogin)
            && shared.launch_mode == LaunchMode::OnDemand;
    let explicit_capture = matches!(
        envelope.request,
        Request::Start { .. } | Request::Resume { .. }
    );
    let request_name = envelope.request.name();
    let log_request = !matches!(
        request_name,
        "status" | "app_opened" | "logs" | "log" | "playback_valid"
    ) && !matches!(
        &envelope.request,
        Request::Workflow {
            action: loppi_protocol::workflow::Action::Output { .. },
        }
    );
    if log_request {
        let mut event = LogEvent::simple("ipc", LogLevel::Info, "request_received");
        event.correlation_id = Some(envelope.id);
        event.context = Some(loppi_protocol::LogContext {
            command: Some(request_name.into()),
            ..Default::default()
        });
        shared.logs.emit(envelope.client, envelope.id, event);
    }
    let stable_lifecycle = matches!(
        envelope.request,
        Request::Status | Request::Lifecycle { .. }
    );
    let response = if envelope.version != PROTOCOL_VERSION && !stable_lifecycle {
        Response::Error {
            id: envelope.id,
            code: "protocol_version".to_owned(),
            message: format!("expected protocol version {PROTOCOL_VERSION}"),
        }
    } else {
        match envelope.request {
            Request::Context { query } => {
                let result = match shared.readers.snapshot() {
                    Ok(reader) => {
                        let mut result = context::query(&reader, &query, envelope.id);
                        if matches!(query, loppi_protocol::context::Query::Status(_))
                            && result["ok"] == true
                        {
                            result["data"]["capture"] =
                                serde_json::to_value(shared.service_state()?)
                                    .map_err(|e| e.to_string())?;
                        }
                        result
                    }
                    Err(error) => loppi_protocol::context::error(envelope.id, "read_busy", error),
                };
                Response::Context {
                    id: envelope.id,
                    result,
                }
            }
            Request::Lifecycle { .. } => Response::Lifecycle {
                id: envelope.id,
                build: loppi_protocol::build_info("core"),
                allow_capture: shared.allow_capture.load(Ordering::Acquire),
                normal_run: shared.normal_run.load(Ordering::Acquire),
            },
            Request::UsageStatistics { boundaries } => {
                let reader = shared.readers.snapshot()?;
                Response::UsageStatistics {
                    id: envelope.id,
                    statistics: usage::statistics(&reader, &boundaries)?,
                }
            }
            Request::CloudStatus => {
                let queues = {
                    let reader = shared.readers.snapshot()?;
                    events::queue_states(&reader)?
                };
                Response::CloudStatus {
                    id: envelope.id,
                    queues,
                }
            }
            Request::CloudTasks { provider, limit } => {
                let reader = shared.readers.snapshot()?;
                Response::CloudTasks {
                    id: envelope.id,
                    tasks: events::task_views(&reader, provider, limit)?,
                }
            }
            Request::CloudConfigurationReceipt {
                provider,
                request_id,
            } => {
                // Never resend verification to discover its outcome. Holding the
                // provider gate proves no verification is still in flight.
                let gate = shared.outbound_gate(provider).try_lock();
                let pending = gate.is_err();
                let reader = shared.readers.snapshot()?;
                let json: Option<String> = reader
                    .query_row(
                        "SELECT state_json FROM mutation_receipts WHERE request_id=?1",
                        params![request_id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                let response = json
                    .map(|v| serde_json::from_str::<Response>(&v))
                    .transpose()
                    .map_err(|e| e.to_string())?;
                Response::CloudConfigurationReceipt {
                    id: envelope.id,
                    pending,
                    response: response.map(Box::new),
                }
            }
            Request::ConfigureCloud {
                provider,
                api_key,
                model,
            } => {
                let chosen = match provider {
                    loppi_protocol::CloudProvider::Llm => {
                        model.unwrap_or_else(|| "deepseek-flash".to_owned())
                    }
                    loppi_protocol::CloudProvider::Asr => "bigmodel".to_owned(),
                };
                let allowed = match provider {
                    loppi_protocol::CloudProvider::Llm => {
                        matches!(chosen.as_str(), "deepseek-flash" | "deepseek-v4-pro")
                    }
                    loppi_protocol::CloudProvider::Asr => chosen == "bigmodel",
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
                    let meter = usage::Recorder::verification(shared);
                    let verified = match provider {
                        loppi_protocol::CloudProvider::Llm => {
                            cloud::verify_llm(&api_key, &chosen, &meter)
                        }
                        loppi_protocol::CloudProvider::Asr => cloud::verify_asr(&api_key, &meter),
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
                        Err(error) => {
                            let response = Response::Error {
                                id: envelope.id,
                                code: error,
                                message:
                                    "cloud provider verification or macOS Keychain storage failed"
                                        .to_owned(),
                            };
                            let store = shared.store.lock().map_err(|e| e.to_string())?;
                            store.db.execute("INSERT INTO mutation_receipts (request_id,fingerprint,state_json) VALUES (?1,?2,?3)", params![envelope.id.to_string(), mutation_fingerprint, serde_json::to_string(&response).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
                            response
                        }
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
                    Err(error) if error == "audio_unavailable" => Response::Error {
                        id: envelope.id,
                        code: error,
                        message: "no retained audio for the failed task".to_owned(),
                    },
                    Err(_) => Response::Error {
                        id: envelope.id,
                        code: "task_not_retryable".to_owned(),
                        message: "task is not failed or no longer matches the current event"
                            .to_owned(),
                    },
                }
            }
            Request::Workflow { action } => {
                if let loppi_protocol::workflow::Action::Output { event_id, run_id, cursor } = &action {
                    let result = shared.readers.snapshot().and_then(|db| {
                        events::workflow::read_output(&db, &shared.workflow_output, *event_id, *run_id, cursor.as_deref())
                    });
                    workflow_reply(envelope.id, result)
                } else if action.read_only() {
                    let now = now_ms()?;
                    let result = shared
                        .readers
                        .snapshot()
                        .and_then(|db| events::workflow::read(&db, &action, now));
                    workflow_reply(envelope.id, result)
                } else {
                    let _asr = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    let result = store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        events::workflow::write(tx, action, envelope.id, now_ms()?)
                            .map(|data| workflow_reply(envelope.id, Ok(data)))
                    });
                    result.unwrap_or_else(|e| workflow_reply(envelope.id, Err(e)))
                }
            }
            Request::Vocabulary { action } => {
                let result = if matches!(action, loppi_protocol::vocabulary::Action::List) {
                    vocabulary::list(&*shared.readers.snapshot()?)
                } else {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    return_response_vocabulary(
                        &mut store,
                        envelope.id,
                        &mutation_fingerprint,
                        &action,
                    )
                };
                Response::Context {
                    id: envelope.id,
                    result: match result {
                        Ok(v) => loppi_protocol::context::success(envelope.id, v),
                        Err(e) => {
                            loppi_protocol::context::error(envelope.id, "invalid_argument", e)
                        }
                    },
                }
            }
            Request::RebuildUnderstanding { event_id } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    let count = events::rebuild_understanding(tx, event_id, now_ms()?)?;
                    Ok(Response::Context {
                        id: envelope.id,
                        result: loppi_protocol::context::success(
                            envelope.id,
                            serde_json::json!({"queued":count,"event_id":event_id}),
                        ),
                    })
                }) {
                    Ok(v) => v,
                    Err(e) => Response::Context {
                        id: envelope.id,
                        result: loppi_protocol::context::error(envelope.id, "derivation_failed", e),
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
                let result = (|| {
                    let reader = shared.readers.snapshot()?;
                    let mut items = events::timeline(&reader, from_ms, to_ms)?;
                    let now = now_ms()?;
                    let state = shared.service_state()?;
                    let mut active = Vec::new();
                    if matches!(state.capture_state, CaptureState::Recording) {
                        if state.microphone_phase == loppi_protocol::SourcePhase::Recording {
                            active.push(AudioSource::Microphone);
                        }
                        if state.system_phase == loppi_protocol::SourcePhase::Recording {
                            active.push(AudioSource::System);
                        }
                    }
                    events::mark_recording_items(&reader, &mut items, &active, now)?;
                    Ok(Response::Timeline {
                        id: envelope.id,
                        items,
                        gaps: events::timeline_gaps(&reader, from_ms, to_ms, now)?,
                    })
                })();
                read_response(envelope.id, result)
            }
            Request::CalendarDays { boundaries } => {
                let result = shared.readers.snapshot().and_then(|db| {
                    Ok(Response::CalendarDays {
                        id: envelope.id,
                        days: events::calendar_days(&db, &boundaries, now_ms()?)?,
                    })
                });
                read_response(envelope.id, result)
            }
            Request::CalendarAdjacent { from_ms, to_ms } => {
                let result = shared.readers.snapshot().and_then(|db| {
                    let (newer_ms, older_ms) =
                        events::calendar_adjacent(&db, from_ms, to_ms, now_ms()?)?;
                    Ok(Response::CalendarAdjacent {
                        id: envelope.id,
                        newer_ms,
                        older_ms,
                    })
                });
                read_response(envelope.id, result)
            }
            Request::EventDetail {
                event_id,
                offset,
                limit,
            } => {
                let result = shared.readers.snapshot().and_then(|reader| {
                    reader
                        .cached(("event", event_id, offset, limit), |db| {
                            events::event_detail(db, event_id, offset, limit)
                        })
                        .map(|event| Response::Event {
                            id: envelope.id,
                            event,
                            snapshot_version: Some(reader.version_key(shared.run_id)),
                        })
                });
                read_response(envelope.id, result)
            }
            Request::RangeDetail {
                started_ms,
                ended_ms,
                sources,
                offset,
                limit,
            } => {
                let result = shared.readers.snapshot().and_then(|reader| {
                    reader
                        .cached(
                            ("range", started_ms, ended_ms, sources, offset, limit),
                            |db| {
                                events::range_detail(
                                    db, started_ms, ended_ms, sources, offset, limit,
                                )
                            },
                        )
                        .map(|event| Response::Event {
                            id: envelope.id,
                            event,
                            snapshot_version: Some(reader.version_key(shared.run_id)),
                        })
                });
                read_response(envelope.id, result)
            }
            Request::ReorganizeEvents {
                started_ms,
                ended_ms,
                title,
                sources,
            } => {
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        let task_id = events::reorganization::enqueue(
                            tx,
                            envelope.id,
                            started_ms,
                            ended_ms,
                            &title,
                            sources,
                            now_ms()?,
                        )?;
                        Ok(Response::Accepted {
                            id: envelope.id,
                            object_id: task_id,
                        })
                    })
                };
                match result {
                    Ok(response) => response,
                    Err(error) => Response::Error {
                        id: envelope.id,
                        code: "reorganization_invalid".into(),
                        message: error,
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
            Request::ConfirmEventCompletion {
                event_id,
                expected_revision,
                started_ms,
                ended_ms,
            } => {
                let _outbound = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let result = {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                        let object_id = events::completion::confirm(
                            tx,
                            envelope.id,
                            event_id,
                            expected_revision,
                            started_ms,
                            ended_ms,
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
                        code: "event_invalid".into(),
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
            Request::SessionSpeakers { segment_id } => {
                let result = shared.readers.snapshot().and_then(|db| {
                    knowledge::session_speakers(&db, segment_id).map(|speakers| {
                        Response::SessionSpeakers {
                            id: envelope.id,
                            speakers,
                        }
                    })
                });
                read_response(envelope.id, result)
            }
            Request::AssignLocalSpeaker {
                segment_id,
                target_segment_id,
                entire_local_speaker,
                expected_revision,
                target_revision,
            } => {
                let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                match store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                    knowledge::assign_local_speaker(
                        tx,
                        segment_id,
                        target_segment_id,
                        entire_local_speaker,
                        expected_revision,
                        target_revision,
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
            Request::People => {
                let reader = shared.readers.snapshot()?;
                Response::People {
                    id: envelope.id,
                    people: reader.cached("people", knowledge::people)?,
                }
            }
            Request::ObjectRevision { kind, object_id } => {
                let reader = shared.readers.snapshot()?;
                match knowledge::object_revision(&reader, &kind, object_id) {
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
                let reader = shared.readers.snapshot()?;
                Response::VoiceSamples {
                    id: envelope.id,
                    samples: knowledge::voice_samples(&reader, person_id)?,
                }
            }
            Request::ReviewItems => {
                let reader = shared.readers.snapshot()?;
                Response::ReviewItems {
                    id: envelope.id,
                    items: knowledge::review_items(&reader)?,
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
                let reader = shared.readers.snapshot()?;
                match knowledge::search(
                    &reader,
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
                                &reader, from_ms, to_ms, person_id,
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
                let result = shared.readers.snapshot().and_then(|db| {
                    moments::list(&db, from_ms, to_ms, offset, limit).map(|moments| {
                        Response::Moments {
                            id: envelope.id,
                            moments,
                        }
                    })
                });
                read_response(envelope.id, result)
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
            Request::CaptureScreenshot {
                excluded_window_ids,
                anchor_ms,
            } => {
                let received_ms = now_ms()?;
                let trigger_ms = anchor_ms.unwrap_or(received_ms);
                let (prior, generation) = {
                    let store = shared.store.lock().map_err(|e| e.to_string())?;
                    (
                        store.prior_mutation_response(envelope.id, &mutation_fingerprint)?,
                        playback::generation(&store.db)?,
                    )
                };
                if let Some(prior) = prior {
                    prior
                } else {
                    match moments::capture(
                        &shared.directory,
                        trigger_ms,
                        received_ms,
                        &excluded_window_ids,
                        peer_pid(stream)?,
                    ) {
                        Ok((captured, scratch)) => {
                            let _gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                            let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                            let saved =
                                store.mutate_response(envelope.id, &mutation_fingerprint, |tx| {
                                    moments::publish_capture(
                                        tx,
                                        &shared.directory,
                                        &captured,
                                        scratch.path(),
                                        generation,
                                    )?;
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
                            code: moments::capture_error_code(&message).into(),
                            message,
                        },
                    }
                }
            }
            Request::ScreenshotData { moment_id } => {
                let _gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                let result = shared
                    .readers
                    .snapshot()
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
                let prepared = if matches!(envelope.client, ClientKind::Gui) {
                    shared.prepare_gui_playback()
                } else {
                    Ok(())
                };
                let result = prepared.and_then(|()| {
                    let _gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                    let db = shared.readers.snapshot()?;
                    let chunk = playback::chunk(&db, &shared.directory, from_ms, to_ms, sources)?;
                    Ok((chunk, playback::generation(&db)?))
                });
                match result {
                    Ok(((wav_base64, has_audio, next_audio_ms), generation)) => {
                        Response::PlaybackChunk {
                            id: envelope.id,
                            from_ms,
                            to_ms,
                            wav_base64,
                            has_audio,
                            next_audio_ms,
                            generation: Some(generation),
                        }
                    }
                    Err(message) => Response::Error {
                        id: envelope.id,
                        code: if message == "playback_exclusion_unavailable" {
                            message.clone()
                        } else {
                            "playback_unavailable".into()
                        },
                        message,
                    },
                }
            }
            Request::PlaybackValid {
                from_ms,
                to_ms,
                sources,
                generation,
            } => {
                let reader = shared.readers.snapshot()?;
                match playback::valid(&reader, from_ms, to_ms, sources, generation) {
                    Ok(valid) => Response::PlaybackValid {
                        id: envelope.id,
                        valid,
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
                let _llm = shared.llm_outbound.lock().map_err(|e| e.to_string())?;
                let _asr = shared.asr_outbound.lock().map_err(|e| e.to_string())?;
                let _voice = shared.voice_outbound.lock().map_err(|e| e.to_string())?;
                let _cleanup_gate = shared.cleanup_gate.lock().map_err(|e| e.to_string())?;
                // Core publication is fenced by cleanup_gate. Source helpers
                // keep sealing; generation/range tombstones filter late journals.
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
                let db_path = shared.directory.join("loppi.sqlite");
                let result = shared
                    .resources
                    .wait_admit(resources::Work::Export, Duration::from_secs(30))
                    .and_then(|_permit| {
                        rusqlite::Connection::open(db_path)
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
                            })
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
                        sample_count: None,
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
                        sample_count: Some(tx.query_row("SELECT count(*) FROM voice_samples WHERE person_id=?1 AND revoked_ms IS NULL", [person_id.to_string()], |r| r.get(0)).map_err(|e| e.to_string())?),
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
                        sample_count: None,
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
                        sample_count: None,
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
                        sample_count: None,
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
                    message: "wait for the bundled speech components to pass their load check"
                        .to_owned(),
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
                let (speech, speaker) = {
                    let mut state = shared.models.lock().map_err(|e| e.to_string())?;
                    begin_model_check(&mut state)
                };
                if speech || speaker {
                    check_models(shared, speech, speaker, envelope.id);
                }
                Response::Models {
                    id: envelope.id,
                    state: shared.models.lock().map_err(|e| e.to_string())?.clone(),
                }
            }
            Request::Probe { source, seconds } => match probe(source, seconds, false) {
                Ok(stats) => Response::ProbeResult {
                    id: envelope.id,
                    stats,
                },
                Err(error) => {
                    eprintln!("loppi-core audio source test failed: {error}");
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
                    let session_id = uuid::Uuid::new_v4();
                    let capture_result = (|| {
                        let clock = record::ClockAnchor::now()?;
                        shared
                            .store
                            .lock()
                            .map_err(|e| e.to_string())?
                            .start_recording_session(session_id, source, shared.run_id, clock)?;
                        let session = ingestion::session(
                            &shared.store.lock().map_err(|e| e.to_string())?.db,
                            session_id,
                        )?;
                        source_process::supervise(
                            &directory,
                            session,
                            Some(seconds),
                            &AtomicBool::new(false),
                            &AtomicU64::new(0),
                            || shared.resources.capture_progress(source_name(source)),
                            |pid, progress| {
                                shared.resources.capture_observed(
                                    source_name(source),
                                    session_id,
                                    pid,
                                    progress,
                                )
                            },
                        )
                    })();
                    flag.store(false, Ordering::Release);
                    shared.resources.capture_stopped(source_name(source));
                    let imported = ingest_session(shared, session_id, true);
                    let capture_result = capture_result.and_then(|stats| imported.map(|()| stats));
                    let end_result = shared
                        .store
                        .lock()
                        .map_err(|error| error.to_string())
                        .and_then(|mut store| {
                            store.end_recording_session(session_id, capture_result.is_ok())
                        });
                    match capture_result.and_then(|stats| end_result.map(|()| stats)) {
                        Ok(stats) => Response::RecordTestResult {
                            id: envelope.id,
                            source,
                            blocks: stats.blocks,
                            frames: stats.frames,
                            dropped_packets: stats.dropped_packets,
                        },
                        Err(error) => {
                            eprintln!("loppi-core short recording failed: {error}");
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
            Request::Diagnose { filter } => {
                let facts = diagnostic_report::validate(&filter).and_then(|_| {
                    let db = shared.readers.snapshot()?;
                    diagnostic_report::facts(&db, &filter, now_ms()?)
                });
                match facts {
                    Ok(facts) => {
                        // Release the DB snapshot before waiting on the log worker.
                        let logs = shared.logs.window(LogFilter { from_ms: Some(filter.from_ms as u64), to_ms: Some(filter.to_ms as u64),
                            job_id: filter.job_id, event_id: filter.event_id,
                            related_job_ids: Some(facts["cloud_tasks"].as_array().into_iter().flatten().filter_map(|t| t["job_id"].as_str()?.parse().ok()).collect()), limit: Some(500), ..Default::default() });
                        Response::Diagnostic { id: envelope.id, report: loppi_protocol::DiagnosticReport {
                            schema: 1, observed_ms: now_ms()?, core_build: loppi_protocol::build_info("core"), filter, facts,
                            logs_available: logs.is_ok(), logs: logs.unwrap_or_default(),
                        }}
                    }
                    Err(_) => Response::Error { id: envelope.id, code: "diagnostics_unavailable".into(),
                        message: "diagnostics require a valid range of at most seven days and an available read snapshot".into() },
                }
            }
            Request::Logs { filter } => match shared.logs.read(filter) {
                Ok(entries) => Response::Logs {
                    id: envelope.id,
                    entries,
                },
                Err(error) => {
                    eprintln!("loppi-core unilogs unavailable: {error}");
                    Response::Error {
                        id: envelope.id,
                        code: "logs_unavailable".to_owned(),
                        message: "diagnostic logs are unavailable".to_owned(),
                    }
                }
            },
            Request::AppOpened => {
                if !matches!(envelope.client, ClientKind::Gui) {
                    Response::Error {
                        id: envelope.id,
                        code: "gui_required".into(),
                        message: "application activation requires GUI".into(),
                    }
                } else {
                    let mut store = shared.store.lock().map_err(|e| e.to_string())?;
                    // The host repeats this with its existing state refresh, so
                    // a restarted query-only Core can recover. Only the first
                    // activation of this instance restores current intent.
                    if !shared.normal_run.load(Ordering::Acquire)
                        || !shared.allow_capture.load(Ordering::Acquire)
                    {
                        if store.state()?.recording_schedule.enabled {
                            store.settle_schedule()?;
                        }
                        store.clear_quit()?;
                        shared.allow_capture.store(true, Ordering::Release);
                        shared.normal_run.store(true, Ordering::Release);
                    }
                    Response::Status {
                        id: envelope.id,
                        state: store.state()?,
                    }
                }
            }
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
        let mut store = shared.store.lock().map_err(|error| error.to_string())?;
        store.clear_quit()?;
        shared.normal_run.store(true, Ordering::Release);
        shared.allow_capture.store(true, Ordering::Release);
    }
    if explicit_quit
        && matches!(
            response,
            Response::Status { .. } | Response::Lifecycle { .. }
        )
    {
        let mut store = shared.store.lock().map_err(|error| error.to_string())?;
        store.mark_quit(current_login_session()?)?;
        shared.normal_run.store(false, Ordering::Release);
        shared.allow_capture.store(false, Ordering::Release);
    }
    let response = match response {
        Response::Status { id, .. } => Response::Status {
            id,
            state: shared.service_state()?,
        },
        response => response,
    };
    let serialized_at = Instant::now();
    let body = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
    let serialize_us = serialized_at.elapsed().as_micros() as u64;
    let failure = match &response {
        Response::Error { code, message, .. } => {
            Some((code.clone(), reads::failure_cause(message).to_owned()))
        }
        Response::Context { result, .. } | Response::Workflow { result, .. }
            if result["ok"] == false =>
        {
            Some((
                result["error"]["code"]
                    .as_str()
                    .unwrap_or("query_failed")
                    .to_owned(),
                result["error"]["cause"]
                    .as_str()
                    .unwrap_or("query_failed")
                    .to_owned(),
            ))
        }
        _ => None,
    };
    if failure.is_some() || log_request {
        let mut event = LogEvent::simple(
            "ipc",
            if failure.is_some() {
                LogLevel::Warn
            } else {
                LogLevel::Info
            },
            request_name,
        );
        event.error_code = failure.as_ref().map(|f| f.0.clone());
        event.duration_ms = Some(request_started.elapsed().as_millis() as u64);
        event.context = Some(loppi_protocol::LogContext {
            command: Some(request_name.into()),
            cause: failure.map(|f| f.1),
            output_bytes: Some(body.len()),
            serialize_us: Some(serialize_us),
            ..Default::default()
        });
        event.correlation_id = Some(envelope.id);
        shared.logs.emit(envelope.client, envelope.id, event);
    }
    shared
        .resources
        .request_histogram
        .observe(request_started.elapsed());
    shared.resources.request_latency_ms.fetch_max(
        request_started.elapsed().as_millis() as u64,
        Ordering::AcqRel,
    );
    Ok((body, quit && !matches!(response, Response::Error { .. })))
}

fn read_request(stream: &mut UnixStream) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut input = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("ipc_read_timeout".to_owned());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| ipc_io_code(&error, false).to_owned())?;
        let count = stream
            .read(&mut chunk)
            .map_err(|error| ipc_io_code(&error, false).to_owned())?;
        if count == 0 {
            return Ok(input);
        }
        input.extend_from_slice(&chunk[..count]);
        if input.len() as u64 > MAX_REQUEST_BYTES {
            return Err("ipc_request_too_large".to_owned());
        }
    }
}

fn workflow_reply(id: uuid::Uuid, result: Result<serde_json::Value, String>) -> Response {
    let mut result = result.unwrap_or_else(|error| {
        let code = match reads::error_code(&error) {
            known if known != "query_failed" => known,
            _ if error.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') => error.as_str(),
            _ if error.contains("overlap") => "event_range_conflict",
            _ => "workflow_failed",
        };
        serde_json::json!({"ok":false,"error":{"code":code,"message":code}})
    });
    result["request_id"] = serde_json::json!(id);
    Response::Workflow { id, result }
}

fn now_ms() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?;
    i64::try_from(duration.as_millis()).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn verify_peer(stream: &UnixStream) -> Result<(), String> {
    stream.peer_pid().map(|_| ()).map_err(|e| e.to_string())
}
#[cfg(windows)]
fn peer_pid(stream: &UnixStream) -> Result<i32, String> {
    stream
        .peer_pid()
        .map(|p| p as i32)
        .map_err(|e| e.to_string())
}
#[cfg(windows)]
fn current_login_session() -> Result<i64, String> {
    loppi_protocol::platform::login_id().map_err(|e| e.to_string())
}
#[cfg(windows)]
fn watch_test_owner() -> Result<(), String> {
    let Some(owner) = std::env::var_os("LOPPI_TEST_PARENT_PID") else {
        return Ok(());
    };
    let owner = owner
        .to_str()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 1 && *v != std::process::id())
        .ok_or("invalid test owner pid")?;
    if std::env::var_os("LOPPI_DATA_DIR").is_none() {
        return Err("test owner requires explicit disposable data directory".into());
    }
    loppi_protocol::platform::watch_process_exit(owner).map_err(|e| e.to_string())
}

// Opt-in only. A test runner owns its disposable Core even when a CLI starts
// it indirectly. kqueue binds to that process lifetime, avoiding PID reuse and
// leaving ordinary Login ownership entirely unchanged.
#[cfg(unix)]
fn watch_test_owner() -> Result<(), String> {
    let Some(owner) = std::env::var_os("LOPPI_TEST_PARENT_PID") else {
        return Ok(());
    };
    let owner: i32 = owner
        .to_str()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 1 && *v != std::process::id() as i32)
        .ok_or("invalid test owner pid")?;
    if std::env::var_os("LOPPI_DATA_DIR").is_none() {
        return Err("test owner requires explicit disposable data directory".into());
    }
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        return Err("test owner watch unavailable".into());
    }
    let change = libc::kevent {
        ident: owner as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    if unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) } != 0 {
        unsafe {
            libc::close(queue);
        }
        return Err("test owner watch registration failed".into());
    }
    thread::spawn(move || {
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };
        loop {
            let result = unsafe {
                libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null())
            };
            if result < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            // A blocked native/model thread must not keep a disposable Core
            // alive after its owner; avoid racing C++ atexit destructors.
            unsafe {
                libc::_exit(70);
            }
        }
    });
    Ok(())
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
    transaction_ms: resources::Histogram,
    ingestion_wait_ms: resources::Histogram,
    ingestion_commit_ms: resources::Histogram,
}

struct PendingBlock {
    id: String,
    started_ms: i64,
    ended_ms: i64,
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
        storage::init(&db)?;
        events::init(&db)?;
        usage::init(&db, now_ms()?)?;
        moments::init(&db)?;
        vocabulary::init(&db)?;
        context::init(&db)?;
        cleanup::init(&db)?;
        ingestion::init(&db)?;
        inventory::init(&db)?;
        coarse::init(&db)?;
        // Earlier workers could publish an empty VAD marker and exhaust all
        // local attempts. Give retained failed blocks one bounded upgrade retry.
        // This neither resets cloud tasks nor changes their paid request IDs.
        storage::migrate(
            &db,
            "vad_nonempty_regions_v1",
            "UPDATE audio_blocks
          SET analysis_state='pending',analysis_attempts=0
          WHERE analysis_state='failed' AND retention_decision='pending_vad_v2'
            AND media_path IS NOT NULL",
        )?;
        schedule::init(&db)?;
        archive::init(&db)?;
        // SQLite 3.46+ bounds analysis internally. Check all tables on opening
        // this long-lived writer, after migrations have created their indexes.
        db.execute_batch("PRAGMA optimize=0x10002;")
            .map_err(|e| e.to_string())?;
        Ok(Self {
            db,
            transaction_ms: Default::default(),
            ingestion_wait_ms: Default::default(),
            ingestion_commit_ms: Default::default(),
        })
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
        let open_sessions = {
            let mut statement = recovery
                .prepare("SELECT id FROM recording_sessions WHERE state='open'")
                .map_err(|e| e.to_string())?;
            statement
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for id in open_sessions {
            ingestion::record_tail_gap(&recovery, &id, "interrupted_tail")?;
        }
        recovery.execute(
            "UPDATE recording_sessions SET ended_ms = ?1, state = 'interrupted' WHERE state = 'open'",
            params![current_ms],
        ).map_err(|error| error.to_string())?;
        recovery.execute(
            "UPDATE audio_blocks SET analysis_state = 'pending' WHERE analysis_state = 'processing'",
            [],
        ).map_err(|error| error.to_string())?;
        coarse::recover(&recovery)?;
        recovery.commit().map_err(|error| error.to_string())?;

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

    fn settle_schedule(&mut self) -> Result<bool, String> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let consumed = schedule::settle(&tx, now_ms()?).map_err(|e| e.to_string())?;
        if consumed {
            tx.execute("DELETE FROM explicit_exit WHERE id=1", [])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(consumed)
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
        tx.execute(
            "INSERT INTO session_generation SELECT ?1,generation FROM ingestion_state WHERE id=1",
            [id.to_string()],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO ingestion_pending_sessions VALUES(?1)",
            [id.to_string()],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn end_recording_session(&mut self, id: uuid::Uuid, completed: bool) -> Result<(), String> {
        let at = now_ms()?;
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let open: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM recording_sessions WHERE id=?1 AND state='open')",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !completed && open {
            ingestion::record_tail_gap(&tx, &id.to_string(), "source_failure")?;
        }
        tx.execute(
            "UPDATE recording_sessions SET ended_ms = ?2, state = ?3 WHERE id = ?1 AND state = 'open'",
            params![id.to_string(), at, if completed { "closed" } else { "failed" }],
        ).map_err(|error| error.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    fn record_gap_after_session(
        &mut self,
        session_id: uuid::Uuid,
        reason: &str,
    ) -> Result<(), String> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        ingestion::record_tail_gap(&tx, &session_id.to_string(), reason)?;
        tx.commit().map_err(|e| e.to_string())
    }

    fn record_open_sleep_gaps(&mut self, sleep_ms: i64) -> Result<(), String> {
        let sessions = self
            .db
            .prepare(
                "SELECT DISTINCT tail_session_id FROM media_gaps
            WHERE ended_ms IS NULL AND tail_session_id IS NOT NULL
            AND reason NOT IN ('manual_pause','schedule_pause','storage_limit','source_disabled')",
            )
            .map_err(|error| error.to_string())?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?;
        for id in sessions {
            self.record_sleep_gap(
                uuid::Uuid::parse_str(&id).map_err(|error| error.to_string())?,
                sleep_ms,
            )?;
        }
        Ok(())
    }

    fn record_sleep_gap(&mut self, session_id: uuid::Uuid, sleep_ms: i64) -> Result<(), String> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let id = session_id.to_string();
        ingestion::record_tail_gap(&tx, &id, "source_failure")?;
        // Preserve a real failure preceding sleep. Late journal import still
        // shortens both pieces through their existing tail_session_id owner.
        tx.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason,tail_session_id)
            SELECT lower(hex(randomblob(16))),source,?2,ended_ms,'machine_sleep',tail_session_id
            FROM media_gaps WHERE tail_session_id=?1 AND started_ms<?2 AND coalesce(ended_ms,?2+1)>?2", params![id,sleep_ms]).map_err(|e| e.to_string())?;
        tx.execute("UPDATE media_gaps SET ended_ms=?2 WHERE tail_session_id=?1 AND started_ms<?2 AND coalesce(ended_ms,?2+1)>?2",params![id,sleep_ms]).map_err(|e| e.to_string())?;
        tx.execute("UPDATE media_gaps SET reason='machine_sleep' WHERE tail_session_id=?1 AND started_ms>=?2",params![id,sleep_ms]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    fn close_open_gaps(&mut self, source: AudioSource) -> Result<(), String> {
        let at = now_ms()?;
        self.db
            .execute(
                "UPDATE media_gaps SET ended_ms = max(started_ms, ?2)
             WHERE source = ?1 AND ended_ms IS NULL
             AND reason NOT IN ('manual_pause','schedule_pause','storage_limit','source_disabled')",
                params![source_name(source), at],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[cfg(test)]
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
                (
                    Some(loppi_protocol::platform::relative_key(&relative)),
                    media.bytes,
                )
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
                "SELECT (SELECT audio_bytes+orphan_bytes FROM storage_usage WHERE id=1), audio_limit_bytes
             FROM capture_guard WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        let free = fs2::available_space(directory).map_err(|error| error.to_string())?;
        Ok(storage_has_capacity(
            used.max(0) as u64,
            limit.max(0) as u64,
            free,
        ))
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
        // A closed session or old wall time is not end-of-stream while the
        // durable journal is still importing. Require an adjacent manifest or
        // a committed discontinuity; the final tail waits for journal drain.
        // Do not skip a waiting prefix and feed a later block from that stream.
        // A retained successor only continues VAD if it will actually run.
        // Historical retries can precede already-complete or exhausted blocks;
        // flush that retry tail instead of leaving speech in an idle worker.
        let idle_before_ms = now_ms()?.saturating_sub(8_000);
        let block = self
            .db
            .query_row(
                "SELECT b.id, b.session_id, b.source, b.sample_start, b.frames, b.media_path,
                    (EXISTS(SELECT 1 FROM ingestion_pending_sessions p WHERE p.session_id=b.session_id)
                    OR s.state != 'open' OR b.ended_ms <= ?1 OR EXISTS (
                      SELECT 1 FROM audio_blocks later
                      WHERE later.session_id = b.session_id
                        AND later.sample_start > b.sample_start
                        AND later.sample_start != b.sample_start + b.frames
                    )) AND NOT EXISTS (
                      SELECT 1 FROM audio_blocks adjacent
                      WHERE adjacent.session_id = b.session_id
                        AND adjacent.sample_start = b.sample_start + b.frames
                        AND adjacent.media_path IS NOT NULL
                        AND adjacent.analysis_state = 'pending' AND adjacent.analysis_attempts < 3
                    ), b.started_ms, b.ended_ms
             FROM audio_blocks b JOIN recording_sessions s ON s.id = b.session_id
             WHERE b.analysis_state = 'pending' AND b.analysis_attempts < 3
                   AND b.media_path IS NOT NULL
                   AND (s.state != 'open' OR b.ended_ms <= ?1 OR EXISTS (
                     SELECT 1 FROM audio_blocks later
                     WHERE later.session_id = b.session_id AND later.sample_start > b.sample_start
                   ))
                   AND NOT EXISTS(SELECT 1 FROM audio_blocks earlier
                     WHERE earlier.session_id=b.session_id AND earlier.sample_start<b.sample_start
                     AND earlier.analysis_state='pending' AND earlier.analysis_attempts<3 AND earlier.media_path IS NOT NULL)
                   AND (NOT EXISTS(SELECT 1 FROM ingestion_pending_sessions p WHERE p.session_id=b.session_id)
                     OR EXISTS(SELECT 1 FROM audio_blocks adjacent WHERE adjacent.session_id=b.session_id
                       AND adjacent.sample_start=b.sample_start+b.frames)
                     OR EXISTS(SELECT 1 FROM media_gaps g WHERE g.source_session_id=b.session_id
                       AND g.started_ms<=b.ended_ms+1 AND g.ended_ms>b.ended_ms)
                     OR EXISTS(SELECT 1 FROM ingestion_clears g JOIN clear_ranges c ON c.id=g.clear_id
                       JOIN session_generation sg ON sg.session_id=b.session_id
                       WHERE g.generation>=sg.generation AND c.from_ms<=b.ended_ms
                       AND min(c.to_ms,c.committed_ms)>b.ended_ms
                       AND c.source_mask&(CASE b.source WHEN 'microphone' THEN 1 ELSE 2 END)!=0))
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
                        started_ms: row.get(7)?,
                        ended_ms: row.get(8)?,
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
                    (coalesce(speech_active, 0) OR retention_decision='pending_vad_v2'), started_ms, ended_ms
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
                            started_ms: row.get(7)?,
                            ended_ms: row.get(8)?,
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
        result: analysis::VadResult,
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
        coarse::enqueue(&tx, block, result.segments)?;
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
            result.speech_pending,
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
                    schedule::manual(tx, at)?;
                    close_intent_gaps(tx, "manual_pause", at)?;
                    tx.execute("UPDATE service_state SET enabled_intent=1,paused_intent=0,revision=revision+1 WHERE id=1",[])
                })
            }
            Request::Pause { expected_revision } => {
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    schedule::manual(tx, at)?;
                    open_intent_gap(tx, "microphone", "manual_pause", at)?;
                    open_intent_gap(tx, "system", "manual_pause", at)?;
                    tx.execute("UPDATE service_state SET paused_intent = 1, revision = revision + 1 WHERE id = 1", [])
                })
            }
            Request::Resume { expected_revision } => {
                if self.state()?.storage_paused
                    && !self.storage_available(&loppi_protocol::data_dir()?)?
                {
                    return Ok(Response::Error {
                        id,
                        code: "storage_limit".to_owned(),
                        message: "free audio space before resuming recording".to_owned(),
                    });
                }
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    schedule::manual(tx, at)?;
                    close_intent_gaps(tx, "manual_pause", at)?;
                    close_intent_gaps(tx, "storage_limit", at)?;
                    tx.execute("UPDATE capture_guard SET pause_reason = NULL WHERE id = 1", [])?;
                    tx.execute("UPDATE service_state SET paused_intent = 0, revision = revision + 1 WHERE id = 1", [])
                })
            }
            Request::SetRecordingSchedule {
                enabled,
                start_minute,
                stop_minute,
                weekday_mask,
                expected_revision,
            } => {
                if start_minute >= 1440 || stop_minute >= 1440 || start_minute == stop_minute {
                    return Ok(Response::Error {
                        id,
                        code: "invalid_schedule".into(),
                        message: "choose two different valid times".into(),
                    });
                }
                if weekday_mask.is_some_and(|days| {
                    !(1..=loppi_protocol::ALL_SCHEDULE_WEEKDAYS).contains(&days)
                }) {
                    return Ok(Response::Error {
                        id,
                        code: "invalid_schedule".into(),
                        message: "choose at least one valid weekday".into(),
                    });
                }
                let at = now_ms()?;
                self.mutate(id, expected_revision, fingerprint, |tx| {
                    schedule::save(tx, enabled, start_minute, stop_minute, weekday_mask, at)
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
                           (SELECT audio_bytes+orphan_bytes FROM storage_usage WHERE id=1) >= ?1
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
            Request::Context { .. }
            | Request::Workflow { .. }
            | Request::Lifecycle { .. }
            | Request::Models
            | Request::PrepareModels
            | Request::Probe { .. }
            | Request::RecordTest { .. }
            | Request::Log { .. }
            | Request::Diagnose { .. }
            | Request::Logs { .. }
            | Request::YieldToLogin
            | Request::AppOpened => {
                unreachable!("handled by IPC dispatcher")
            }
            Request::CloudStatus
            | Request::UsageStatistics { .. }
            | Request::CloudTasks { .. }
            | Request::CloudConfigurationReceipt { .. }
            | Request::ConfigureCloud { .. }
            | Request::SetCloudPaused { .. }
            | Request::RetryCloudTask { .. }
            | Request::ProcessNow
            | Request::Vocabulary { .. }
            | Request::RebuildUnderstanding { .. }
            | Request::Timeline { .. }
            | Request::CalendarDays { .. }
            | Request::CalendarAdjacent { .. }
            | Request::EventDetail { .. }
            | Request::RangeDetail { .. }
            | Request::ReorganizeEvents { .. }
            | Request::CreateEvent { .. }
            | Request::EditEvent { .. }
            | Request::ConfirmEventCompletion { .. }
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
            | Request::SessionSpeakers { .. }
            | Request::AssignLocalSpeaker { .. }
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
            | Request::CaptureScreenshot { .. }
            | Request::ScreenshotData { .. }
            | Request::PlaybackChunk { .. }
            | Request::PlaybackValid { .. }
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
        let _timer = self.transaction_ms.timer();
        let tx = self.db.transaction().map_err(|error| error.to_string())?;
        if let Some(response) = prior_mutation_response(&tx, id, fingerprint)? {
            return Ok(response);
        }
        let response = change(&tx)?;
        if matches!(response, Response::Error { .. })
            || matches!(&response, Response::Workflow { result, .. } if result["ok"] == false)
        {
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
        let _timer = self.transaction_ms.timer();
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

fn storage_has_capacity(used: u64, limit: u64, free: u64) -> bool {
    used < limit && free > 1_073_741_824
}

fn read_state(db: &Connection) -> Result<ServiceState, String> {
    let recording_schedule = schedule::view(db)?;
    db.query_row(
        "SELECT revision, enabled_intent, paused_intent, microphone_enabled, system_enabled,
                (SELECT pause_reason IS NOT NULL FROM capture_guard WHERE id = 1),
                (SELECT audio_bytes+orphan_bytes FROM storage_usage WHERE id=1),
                (SELECT audio_limit_bytes FROM capture_guard WHERE id=1)
         FROM service_state WHERE id = 1",
        [],
        |row| {
            Ok(ServiceState {
                recording_schedule: recording_schedule.clone(),
                revision: row.get(0)?,
                enabled_intent: row.get(1)?,
                paused_intent: row.get(2)?,
                storage_paused: row.get(5)?,
                storage_recoverable: false,
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
                microphone_phase: loppi_protocol::SourcePhase::Idle,
                system_phase: loppi_protocol::SourcePhase::Idle,
                capture_blocker: loppi_protocol::CaptureBlocker::None,
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

    #[test]
    fn bundled_model_recheck_is_single_flight_and_can_retry_failure() {
        let mut state = ModelState {
            phase: ModelPhase::Ready,
            speaker_phase: ModelPhase::Failed,
            progress_percent: 100,
            error: None,
            speaker_error: Some("model_resources_busy".into()),
        };
        assert_eq!(begin_model_check(&mut state), (false, true));
        assert_eq!(begin_model_check(&mut state), (false, false));
        assert_eq!(state.progress_percent, 100);
        assert!(state.speaker_error.is_none());
        let models = Mutex::new(state);
        finish_model(&models, false, Err("model_resources_busy".into()));
        finish_model(&models, true, Ok(()));
        let mut state = models.lock().unwrap();
        assert_eq!(state.error.as_deref(), Some("model_resources_busy"));
        assert_eq!(begin_model_check(&mut state), (true, false));
        assert!(state.error.is_none());
    }

    #[test]
    fn delayed_vad_failure_keeps_claimed_audio_range_in_diagnostic_logs() {
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let session = Uuid::new_v4();
        let job = Uuid::new_v4();
        store
            .db
            .execute(
                "INSERT INTO recording_sessions VALUES(?1,'microphone',1000,2000,2000,'closed')",
                [session.to_string()],
            )
            .unwrap();
        store.db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state) VALUES(?1,?2,'microphone',1000,2000,0,48000,100,'media/microphone/private.opus','pending_vad_v2','pending')", params![job.to_string(),session.to_string()]).unwrap();
        let block = store.take_pending_block().unwrap().unwrap();
        let event = vad_log_event(
            &block,
            &Err("private decode error".into()),
            "local_analysis_decode",
        );
        assert_eq!(event.event, "vad_failed");
        assert_eq!(event.job_id, Some(job));
        assert_eq!(event.capture_span_id, Some(session));
        let context = event.context.as_ref().unwrap();
        assert_eq!((context.start_ms, context.end_ms), (Some(1000), Some(2000)));
        assert_eq!(context.source, Some(AudioSource::Microphone));
        let directory = tempfile::tempdir().unwrap();
        let mut logs = unilogs::UniLogs::open(directory.path(), uuid::Uuid::new_v4()).unwrap();
        let now = now_ms().unwrap();
        logs.emit(ClientKind::Worker, Uuid::new_v4(), event);
        let filter = loppi_protocol::DiagnosticFilter {
            from_ms: now - 60_000,
            to_ms: now + 60_000,
            event_id: None,
            job_id: Some(job),
        };
        let facts = diagnostic_report::facts(&store.db, &filter, now).unwrap();
        assert!(facts["window_local_jobs"].as_array().unwrap().is_empty());
        let window = logs
            .window(&LogFilter {
                from_ms: Some(filter.from_ms as u64),
                to_ms: Some(filter.to_ms as u64),
                job_id: Some(job),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(window.entries.len(), 1);
        let context = window.entries[0].event.context.as_ref().unwrap();
        assert_eq!((context.start_ms, context.end_ms), (Some(1000), Some(2000)));
        assert!(!serde_json::to_string(&window).unwrap().contains("private"));
    }

    #[test]
    fn vad_upgrade_retries_retained_failures_once_without_reviving_cleared_media() {
        let store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store
            .db
            .execute(
                "DELETE FROM core_migrations WHERE name='vad_nonempty_regions_v1'",
                [],
            )
            .unwrap();
        for (id, decision, path) in [
            ("failed", "pending_vad_v2", Some("media/system/a.opus")),
            ("cleared", "all_data", None),
            ("missing", "pending_vad_v2", None),
            (
                "legacy",
                "pending_conservative_v1",
                Some("media/system/old.opus"),
            ),
        ] {
            store.db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,analysis_attempts)
              VALUES(?1,'s','system',0,5000,0,240000,100,?2,?3,'failed',3)", params![id,path,decision]).unwrap();
        }
        let store = Store::new(store.db).unwrap();
        assert_eq!(store.db.query_row("SELECT analysis_attempts FROM audio_blocks WHERE id='failed' AND analysis_state='pending'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT count(*) FROM audio_blocks WHERE analysis_state='failed'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            3
        );
        store.db.execute("UPDATE audio_blocks SET analysis_state='failed',analysis_attempts=3 WHERE id='failed'", []).unwrap();
        let store = Store::new(store.db).unwrap();
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT count(*) FROM audio_blocks WHERE analysis_state='pending'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    use uuid::Uuid;

    #[test]
    fn historical_retry_flushes_uncontinued_speech_but_keeps_pending_successors_contiguous() {
        for (successor_state, attempts, expect_flush) in [
            ("complete", 1, true),
            ("failed", 3, true),
            ("pending", 3, true),
            ("pending", 0, false),
        ] {
            let store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
            store
                .db
                .execute(
                    "INSERT INTO recording_sessions VALUES('s','system',0,15000,15000,'closed')",
                    [],
                )
                .unwrap();
            store
                .db
                .execute(
                    "DELETE FROM core_migrations WHERE name='vad_nonempty_regions_v1'",
                    [],
                )
                .unwrap();
            store.db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,analysis_attempts)
              VALUES('retry','s','system',5000,10000,240000,240000,100,'media/system/b.opus','pending_vad_v2','failed',3)", []).unwrap();
            store.db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,analysis_attempts,speech_active)
              VALUES('successor','s','system',10000,15000,480000,240000,100,'media/system/c.opus','retain_speech_v2',?1,?2,1)", params![successor_state,attempts]).unwrap();
            let mut store = Store::new(store.db).unwrap();
            let retry = store.take_pending_block().unwrap().unwrap();
            assert_eq!(retry.id, "retry");
            assert_eq!(
                retry.flush, expect_flush,
                "successor={successor_state}/{attempts}"
            );
            // Model the worker's open-tail contract: a region is returned only
            // after a flush or subsequent audio. Exercise real publication and
            // scheduling so a completed recovery cannot abandon the tail.
            let region_end = if expect_flush { 160_000 } else { 240_000 };
            let result = |closed| analysis::VadResult {
                segments: if closed {
                    vec![analysis::SpeechRegion {
                        start_sample: 80_000,
                        end_sample: region_end,
                    }]
                } else {
                    vec![]
                },
                speech_active: true,
                speech_pending: !closed,
                error: None,
            };
            store.publish_analysis(&retry, result(retry.flush)).unwrap();
            if !expect_flush {
                let successor = store.take_pending_block().unwrap().unwrap();
                assert_eq!(successor.id, "successor");
                assert!(successor.flush);
                store
                    .publish_analysis(&successor, result(successor.flush))
                    .unwrap();
            }
            assert!(store.take_pending_block().unwrap().is_none());
            assert_eq!(store.db.query_row("SELECT count(*) FROM coarse_jobs WHERE session_id='s' AND start_sample=80000 AND end_sample=?1", [region_end as i64], |r| r.get::<_,i64>(0)).unwrap(), 1);
            assert_eq!(
                store
                    .db
                    .query_row(
                        "SELECT count(*) FROM audio_blocks WHERE media_path IS NOT NULL",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                2
            );
        }
    }

    #[test]
    fn recovered_capacity_stays_manual() {
        assert!(storage_has_capacity(99, 100, 1_073_741_825));
        assert!(!storage_has_capacity(100, 100, 1_073_741_825));
        assert!(!storage_has_capacity(99, 100, 1_073_741_824));
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store.pause_storage().unwrap();
        let reopened = Store::new(store.db).unwrap();
        assert!(reopened.state().unwrap().storage_paused);
    }

    #[test]
    fn sleep_gap_does_not_relabel_the_preceding_capture_failure() {
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let id = uuid::Uuid::new_v4();
        store
            .db
            .execute(
                "INSERT INTO recording_sessions VALUES(?1,'microphone',1000,2000,9000,'failed')",
                [id.to_string()],
            )
            .unwrap();
        store.record_sleep_gap(id, 5000).unwrap();
        let rows = store
            .db
            .prepare("SELECT started_ms,ended_ms,reason FROM media_gaps ORDER BY started_ms")
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (2000, Some(5000), "source_failure".into()),
                (5000, None, "machine_sleep".into())
            ]
        );
    }

    #[test]
    fn sleep_during_source_backoff_splits_the_durable_gap_without_a_slot() {
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let failed = Uuid::new_v4();
        store
            .db
            .execute(
                "INSERT INTO recording_sessions(id,source,started_ms,last_confirmed_ms,ended_ms,state) VALUES(?1,'microphone',1000,2000,NULL,'open')",
                [failed.to_string()],
            )
            .unwrap();
        store.end_recording_session(failed, false).unwrap();
        // The supervisor has already joined the failed helper and discarded
        // its slot. A power transition must act on its durable gap anyway.
        let slot: Option<SourceSlot> = None;
        let mut retry_delay = Duration::from_secs(30);
        let mut retry_at = Instant::now() + retry_delay;
        assert!(slot.is_none());
        reset_capture_retry(&mut retry_at, &mut retry_delay);
        store.record_open_sleep_gaps(5000).unwrap();
        // Completed wake may deliver the boundary again. It remains idempotent
        // through the old session owner, rather than inventing a new gap owner.
        store.record_open_sleep_gaps(5000).unwrap();
        let rows = store.db.prepare("SELECT started_ms,ended_ms,reason,tail_session_id FROM media_gaps ORDER BY started_ms").unwrap().query_map([], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    2000,
                    Some(5000),
                    "source_failure".into(),
                    failed.to_string()
                ),
                (5000, None, "machine_sleep".into(), failed.to_string())
            ]
        );
        assert_eq!(retry_delay, Duration::ZERO);
        let recovered = Uuid::new_v4();
        store
            .start_recording_session(
                recovered,
                AudioSource::Microphone,
                Uuid::new_v4(),
                record::ClockAnchor::now().unwrap(),
            )
            .unwrap();
        let ended: i64 = store
            .db
            .query_row(
                "SELECT ended_ms FROM media_gaps WHERE reason='machine_sleep'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restarted: i64 = store
            .db
            .query_row(
                "SELECT started_ms FROM recording_sessions WHERE id=?1",
                [recovered.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ended, restarted);
        assert_eq!(
            store
                .db
                .query_row::<i64, _, _>(
                    "SELECT ended_ms FROM media_gaps WHERE reason='source_failure'",
                    [],
                    |r| r.get(0)
                )
                .unwrap(),
            5000
        );
    }

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
            voice_error_code(&events::voice_manifest(&db, "microphone", 0, 300).unwrap_err()),
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
        let missing =
            events::voice_pcm_16k(directory.path(), &task, &parts, &mut Default::default())
                .unwrap_err();
        assert_eq!(voice_error_code(&missing), "audio_unavailable");

        let media_directory = directory.path().join("media/microphone");
        fs::create_dir_all(&media_directory).unwrap();
        let mut block = media::OpusBlock::create(&media_directory, id).unwrap();
        block.push(&vec![0.0; 144_000]).unwrap();
        block.seal().unwrap();
        let quiet = events::voice_pcm_16k(directory.path(), &task, &parts, &mut Default::default())
            .unwrap_err();
        assert_eq!(voice_error_code(&quiet), "audio_quality");
    }

    #[test]
    fn stalled_voice_worker_cannot_hold_the_core_forever_while_stdin_is_full() {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join(if cfg!(windows) {
            "stalled-worker.py"
        } else {
            "stalled-worker"
        });
        fs::write(
            &worker,
            if cfg!(windows) {
                b"import time;time.sleep(60)".as_slice()
            } else {
                b"#!/bin/sh\nexec sleep 60\n".as_slice()
            },
        )
        .unwrap();
        loppi_protocol::platform::set_mode(&worker, 0o700).unwrap();
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
        let db_path = directory.path().join("loppi.sqlite");
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
            started_ms: 2000,
            ended_ms: 3000,
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
        store = inventory::scan_all(store, directory.path());
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
    fn abandoned_observations_do_not_hide_mutation_or_timeout_failures() {
        let id = Uuid::new_v4();
        for command in ["status", "app_opened", "resume", "timeline"] {
            for error in [
                "ipc_write_disconnected",
                "ipc_write_timeout",
                "ipc_execute_failed",
            ] {
                let event = ipc_failure_event(Some(id), Some(command), error);
                let cancelled =
                    matches!(command, "status" | "app_opened") && error == "ipc_write_disconnected";
                assert_eq!(
                    event.level,
                    if cancelled {
                        LogLevel::Info
                    } else {
                        LogLevel::Warn
                    }
                );
                assert_eq!(
                    event.event,
                    if cancelled {
                        "request_cancelled"
                    } else {
                        "request_failed"
                    }
                );
                assert_eq!(event.correlation_id, Some(id));
                assert_eq!(event.error_code.as_deref(), Some(error));
                assert!(crate::unilogs::valid_event(&event));
            }
        }
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
    fn orphan_over_limit_pauses_when_background_reconciliation_discovers_it() {
        let directory = tempfile::tempdir().unwrap();
        let media_dir = directory.path().join("media/microphone");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(
            media_dir.join(format!("{}.opus", Uuid::new_v4())),
            vec![1; 200],
        )
        .unwrap();
        let db_path = directory.path().join("loppi.sqlite");
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
        store = inventory::scan_all(store, directory.path());
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
                    loppi_protocol::EventSources::Microphone,
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
                        loppi_protocol::EventSources::Microphone,
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
                    loppi_protocol::CloudProvider::Llm,
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
                    loppi_protocol::EventSources::Microphone,
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
                    loppi_protocol::CloudProvider::Llm,
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
            events::credential_reference(&store.db, loppi_protocol::CloudProvider::Llm).unwrap(),
            Some(original_ref)
        );
        assert_eq!(
            events::model(&store.db, loppi_protocol::CloudProvider::Llm)
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

fn return_response_vocabulary(
    store: &mut Store,
    id: uuid::Uuid,
    fingerprint: &str,
    action: &loppi_protocol::vocabulary::Action,
) -> Result<serde_json::Value, String> {
    let response = store.mutate_response(id, fingerprint, |tx| {
        Ok(Response::Context {
            id,
            result: loppi_protocol::context::success(id, vocabulary::apply(tx, action)?),
        })
    })?;
    if let Response::Context { result, .. } = response {
        Ok(result["data"].clone())
    } else {
        Err("unexpected vocabulary receipt".into())
    }
}
