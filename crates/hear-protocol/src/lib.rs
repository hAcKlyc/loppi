use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    pub id: Uuid,
    pub client: ClientKind,
    pub request: Request,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Core,
    Cli,
    Gui,
    Worker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Status,
    Probe {
        source: AudioSource,
        seconds: u8,
    },
    Start {
        expected_revision: i64,
    },
    Pause {
        expected_revision: i64,
    },
    Resume {
        expected_revision: i64,
    },
    SetSource {
        source: AudioSource,
        enabled: bool,
        expected_revision: i64,
    },
    Log {
        event: LogEvent,
    },
    Logs {
        filter: LogFilter,
    },
    YieldToLogin,
    Quit,
}

impl Request {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Probe { .. } => "probe",
            Self::Start { .. } => "start",
            Self::Pause { .. } => "pause",
            Self::Resume { .. } => "resume",
            Self::SetSource { .. } => "set_source",
            Self::Log { .. } => "log",
            Self::Logs { .. } => "logs",
            Self::YieldToLogin => "yield_to_login",
            Self::Quit => "quit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub component: String,
    pub level: LogLevel,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_span_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogFilter {
    pub component: Option<String>,
    pub level: Option<LogLevel>,
    pub correlation_id: Option<Uuid>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub timestamp_ms: u64,
    pub run_id: Uuid,
    pub elapsed_ms: u64,
    pub client: ClientKind,
    pub request_id: Uuid,
    #[serde(flatten)]
    pub event: LogEvent,
}

impl LogEvent {
    pub fn simple(component: &str, level: LogLevel, event: &str) -> Self {
        Self {
            component: component.to_owned(),
            level,
            event: event.to_owned(),
            correlation_id: None,
            source_id: None,
            capture_span_id: None,
            event_id: None,
            job_id: None,
            revision: None,
            duration_ms: None,
            dropped_count: None,
            error_code: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSource {
    Microphone,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Status {
        id: Uuid,
        state: ServiceState,
    },
    ProbeResult {
        id: Uuid,
        stats: ProbeStats,
    },
    Logs {
        id: Uuid,
        entries: Vec<LogRecord>,
    },
    Error {
        id: Uuid,
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeStats {
    pub source: AudioSource,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub peak: f32,
    pub dropped_packets: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    pub revision: i64,
    pub enabled_intent: bool,
    pub paused_intent: bool,
    pub microphone_enabled: bool,
    pub system_enabled: bool,
    pub capture_state: CaptureState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    NotReady,
    Stopped,
    Paused,
    Recording,
}

pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("HEAR_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    dirs::data_dir()
        .map(|base| base.join("hear"))
        .ok_or_else(|| "cannot locate the user's application data directory".to_owned())
}

pub fn socket_path() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("core.sock"))
}

pub fn send(request: Request) -> Result<Response, String> {
    send_from(ClientKind::Cli, request)
}

pub fn send_from(client: ClientKind, request: Request) -> Result<Response, String> {
    send_from_with_id(client, request, Uuid::new_v4())
}

// The envelope ID is also the durable duplicate-submit key for mutations.
// A retry must reuse both this ID and its original expected revision.
pub fn send_from_with_id(
    client: ClientKind,
    request: Request,
    id: Uuid,
) -> Result<Response, String> {
    send_with_policy(client, request, id, true)
}

pub fn send_existing(client: ClientKind, request: Request) -> Result<Response, String> {
    send_with_policy(client, request, Uuid::new_v4(), false)
}

fn send_with_policy(
    client: ClientKind,
    mut request: Request,
    id: Uuid,
    start_on_demand: bool,
) -> Result<Response, String> {
    let socket = socket_path()?;
    let mut stream = match UnixStream::connect(&socket) {
        Ok(stream) => stream,
        Err(_) if start_on_demand => {
            start_core_on_demand()?;
            wait_for_core(&socket)?
        }
        Err(error) => return Err(error.to_string()),
    };
    let timeout = match request {
        Request::Probe { seconds, .. } => Duration::from_secs(seconds as u64 + 70),
        _ => Duration::from_secs(5),
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    attach_correlation(&mut request, id);
    let envelope = Envelope {
        version: PROTOCOL_VERSION,
        id,
        client,
        request,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
    stream
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| error.to_string())?;

    let mut response = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    if response.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("core response exceeded protocol limit".to_owned());
    }
    let decoded: Response = serde_json::from_slice(&response).map_err(|error| error.to_string())?;
    match &decoded {
        Response::Status { id, .. }
        | Response::ProbeResult { id, .. }
        | Response::Logs { id, .. }
        | Response::Error { id, .. }
            if *id == envelope.id =>
        {
            Ok(decoded)
        }
        _ => Err("core response ID did not match request".to_owned()),
    }
}

fn attach_correlation(request: &mut Request, id: Uuid) {
    if let Request::Log { event } = request {
        event.correlation_id.get_or_insert(id);
    }
}

fn wait_for_core(socket: &Path) -> Result<UnixStream, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(stream) = UnixStream::connect(socket) {
            return Ok(stream);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("hear-core did not become available".to_owned())
}

fn start_core_on_demand() -> Result<(), String> {
    let path = core_binary_path()?;
    Command::new(path)
        .arg("serve")
        .arg("--on-demand")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("cannot launch hear-core: {error}"))
}

fn core_binary_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("HEAR_CORE_BIN") {
        return Ok(PathBuf::from(path));
    }
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let sibling = executable
        .parent()
        .ok_or_else(|| "cannot locate hear binary directory".to_owned())?;
    let installed = sibling.join("../Library/LoginItems/HearCore.app/Contents/MacOS/hear-core");
    if installed.is_file() {
        return fs::canonicalize(installed).map_err(|error| error.to_string());
    }
    let development = sibling.join("hear-core");
    if development.is_file() {
        return Ok(development);
    }
    Err("hear-core is not installed beside the app or CLI".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_request_inherits_ipc_correlation() {
        let id = Uuid::new_v4();
        let mut request = Request::Log {
            event: LogEvent::simple("gui", LogLevel::Info, "opened"),
        };
        attach_correlation(&mut request, id);
        let Request::Log { event } = request else {
            unreachable!()
        };
        assert_eq!(event.correlation_id, Some(id));
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("null"));
    }
}
