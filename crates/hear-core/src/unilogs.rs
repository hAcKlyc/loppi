use hear_protocol::{ClientKind, LogEvent, LogFilter, LogRecord};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const LOG_BYTES: u64 = 4 * 1024 * 1024;
const ARCHIVES: usize = 4;

pub struct UniLogs {
    directory: PathBuf,
    file: File,
    run_id: Uuid,
    started: Instant,
}

impl UniLogs {
    pub fn open(data_directory: &Path) -> Result<Self, String> {
        let directory = data_directory.join("logs");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let file = Self::open_file(&directory)?;
        Ok(Self {
            directory,
            file,
            run_id: Uuid::new_v4(),
            started: Instant::now(),
        })
    }

    fn open_file(directory: &Path) -> Result<File, String> {
        let path = directory.join("hear.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)
            .map_err(|error| error.to_string())?;
        if !file
            .metadata()
            .map_err(|error| error.to_string())?
            .file_type()
            .is_file()
        {
            return Err("diagnostic log path is not a regular file".to_owned());
        }
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        Ok(file)
    }

    pub fn emit(&mut self, client: ClientKind, request_id: Uuid, event: LogEvent) {
        if !valid_event(&event) {
            return;
        }
        if let Err(error) = self.try_emit(client, request_id, event) {
            eprintln!("hear-core unilogs unavailable: {error}");
        }
    }

    fn try_emit(
        &mut self,
        client: ClientKind,
        request_id: Uuid,
        event: LogEvent,
    ) -> Result<(), String> {
        let record = LogRecord {
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_millis() as u64,
            run_id: self.run_id,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
            client,
            request_id,
            event,
        };
        let mut encoded = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
        encoded.push(b'\n');
        if self
            .file
            .metadata()
            .map_err(|error| error.to_string())?
            .len()
            + encoded.len() as u64
            > LOG_BYTES
        {
            self.rotate()?;
        }
        self.file
            .write_all(&encoded)
            .map_err(|error| error.to_string())
    }

    fn rotate(&mut self) -> Result<(), String> {
        for archive in (1..=ARCHIVES).rev() {
            let previous = if archive == 1 {
                self.directory.join("hear.jsonl")
            } else {
                self.directory
                    .join(format!("hear.{:02}.jsonl", archive - 1))
            };
            let next = self.directory.join(format!("hear.{archive:02}.jsonl"));
            if previous.exists() {
                fs::rename(previous, next).map_err(|error| error.to_string())?;
            }
        }
        self.file = Self::open_file(&self.directory)?;
        Ok(())
    }

    pub fn read(&self, filter: &LogFilter) -> Result<Vec<LogRecord>, String> {
        let limit = filter.limit.unwrap_or(100).min(500);
        let mut entries = Vec::new();
        for archive in (1..=ARCHIVES).rev() {
            let path = self.directory.join(format!("hear.{archive:02}.jsonl"));
            read_matching(&path, filter, &mut entries)?;
        }
        read_matching(&self.directory.join("hear.jsonl"), filter, &mut entries)?;
        if entries.len() > limit {
            entries.drain(..entries.len() - limit);
        }
        Ok(entries)
    }
}

fn read_matching(
    path: &Path,
    filter: &LogFilter,
    entries: &mut Vec<LogRecord>,
) -> Result<(), String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > LOG_BYTES {
        return Err("diagnostic log archive is not a bounded regular file".to_owned());
    }
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| error.to_string())?;
        let Ok(record) = serde_json::from_str::<LogRecord>(&line) else {
            continue;
        };
        if filter
            .component
            .as_ref()
            .is_some_and(|value| *value != record.event.component)
        {
            continue;
        }
        if filter
            .level
            .is_some_and(|value| value != record.event.level)
        {
            continue;
        }
        if filter
            .correlation_id
            .is_some_and(|value| record.event.correlation_id != Some(value))
        {
            continue;
        }
        entries.push(record);
    }
    Ok(())
}

pub(crate) fn valid_event(event: &LogEvent) -> bool {
    fn token(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 80
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'_'
            })
    }
    token(&event.component) && token(&event.event) && event.error_code.as_deref().is_none_or(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hear_protocol::LogLevel;

    #[test]
    fn log_filter_and_validation_exclude_free_text() {
        let directory = tempfile::tempdir().unwrap();
        let mut logs = UniLogs::open(directory.path()).unwrap();
        let id = Uuid::new_v4();
        logs.emit(
            ClientKind::Cli,
            id,
            LogEvent::simple("ipc", LogLevel::Info, "request"),
        );
        logs.emit(
            ClientKind::Gui,
            id,
            LogEvent::simple("ipc", LogLevel::Error, "private transcript"),
        );
        let filter = LogFilter {
            component: Some("ipc".to_owned()),
            level: Some(LogLevel::Info),
            correlation_id: None,
            limit: Some(10),
        };
        let found = logs.read(&filter).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].event.event, "request");
    }

    #[test]
    fn correlation_connects_gui_and_core_without_personal_content() {
        let directory = tempfile::tempdir().unwrap();
        let mut logs = UniLogs::open(directory.path()).unwrap();
        let id = Uuid::new_v4();
        let mut gui = LogEvent::simple("gui", LogLevel::Info, "opened");
        gui.correlation_id = Some(id);
        logs.emit(ClientKind::Gui, id, gui);
        let mut core = LogEvent::simple("ipc", LogLevel::Info, "client_event_received");
        core.correlation_id = Some(id);
        logs.emit(ClientKind::Core, id, core);
        let entries = logs
            .read(&LogFilter {
                correlation_id: Some(id),
                ..LogFilter::default()
            })
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].request_id, entries[1].request_id);
        let bytes = fs::read(directory.path().join("logs/hear.jsonl")).unwrap();
        assert!(!String::from_utf8(bytes).unwrap().contains("null"));
        assert_eq!(
            fs::metadata(directory.path().join("logs/hear.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
