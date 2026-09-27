use fs2::FileExt;
use hear_protocol::{
    AudioSource, CaptureState, ClientKind, Envelope, LogEvent, LogFilter, LogLevel,
    PROTOCOL_VERSION, ProbeStats, Request, Response, ServiceState, data_dir,
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

const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
const MAX_CLIENTS: usize = 32;
mod microphone;
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
    let store = Store::new(connection)?;
    let logs = LogWorker::start(directory.clone());
    logs.emit(
        ClientKind::Core,
        uuid::Uuid::new_v4(),
        LogEvent::simple("core", LogLevel::Info, "started"),
    );
    let shared = Arc::new(CoreState {
        store: Mutex::new(store),
        logs,
        launch_mode,
    });
    let stop = Arc::new(AtomicBool::new(false));
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
    drop(listener);
    fs::remove_file(&socket).map_err(|error| error.to_string())?;
    drop(lock);
    Ok(())
}

struct CoreState {
    store: Mutex<Store>,
    logs: LogWorker,
    launch_mode: LaunchMode,
}

enum LogCommand {
    Emit(ClientKind, uuid::Uuid, LogEvent),
    Read(
        LogFilter,
        mpsc::Sender<Result<Vec<hear_protocol::LogRecord>, String>>,
    ),
}

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
    let quit = matches!(envelope.request, Request::Quit)
        || matches!(envelope.request, Request::YieldToLogin)
            && shared.launch_mode == LaunchMode::OnDemand;
    let request_name = envelope.request.name();
    let response = if envelope.version != PROTOCOL_VERSION {
        Response::Error {
            id: envelope.id,
            code: "protocol_version".to_owned(),
            message: format!("expected protocol version {PROTOCOL_VERSION}"),
        }
    } else {
        match envelope.request {
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
            Request::Log { event } => {
                shared.logs.emit(envelope.client, envelope.id, event);
                let mut received = LogEvent::simple("ipc", LogLevel::Info, "client_event_received");
                received.correlation_id = Some(envelope.id);
                shared.logs.emit(ClientKind::Core, envelope.id, received);
                Response::Status {
                    id: envelope.id,
                    state: shared
                        .store
                        .lock()
                        .map_err(|error| error.to_string())?
                        .state()?,
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
                        state: shared
                            .store
                            .lock()
                            .map_err(|error| error.to_string())?
                            .state()?,
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

struct Store {
    db: Connection,
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
             CREATE TABLE IF NOT EXISTS mutation_receipts (
               request_id TEXT PRIMARY KEY,
               fingerprint TEXT NOT NULL,
               state_json TEXT NOT NULL
             );",
        )
        .map_err(|error| error.to_string())?;
        Ok(Self { db })
    }

    fn state(&self) -> Result<ServiceState, String> {
        read_state(&self.db)
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
            Request::Resume { expected_revision } => self.mutate(
                id,
                expected_revision,
                fingerprint,
                |tx| tx.execute("UPDATE service_state SET paused_intent = 0, revision = revision + 1 WHERE id = 1", []),
            ),
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
            Request::Probe { .. }
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
        "SELECT revision, enabled_intent, paused_intent, microphone_enabled, system_enabled FROM service_state WHERE id = 1",
        [],
        |row| {
            Ok(ServiceState {
                revision: row.get(0)?,
                enabled_intent: row.get(1)?,
                paused_intent: row.get(2)?,
                microphone_enabled: row.get(3)?,
                system_enabled: row.get(4)?,
                capture_state: CaptureState::NotReady,
                reason: Some("local speech models and both cloud providers must be configured before recording".to_owned()),
            })
        },
    )
    .map_err(|error| error.to_string())
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
