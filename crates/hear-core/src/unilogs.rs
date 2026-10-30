use hear_protocol::{ClientKind, LogContext, LogEvent, LogFilter, LogLevel, LogRecord, LogWindow};
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
    vad_counts: [u64; 3],
    summary_since: Instant,
}

impl UniLogs {
    pub fn open(data_directory: &Path, run_id: Uuid) -> Result<Self, String> {
        let directory = data_directory.join("logs");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let file = Self::open_file(&directory)?;
        Ok(Self {
            directory,
            file,
            run_id,
            started: Instant::now(),
            vad_counts: [0; 3],
            summary_since: Instant::now(),
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
        if self.summary_since.elapsed().as_secs() >= 60 {
            self.flush_summary();
        }
        if event.component == "speech"
            && event.event == "vad_complete"
            && matches!(event.level, LogLevel::Info)
        {
            let index = match event.context.as_ref().and_then(|c| c.source) {
                Some(hear_protocol::AudioSource::Microphone) => 0,
                Some(hear_protocol::AudioSource::System) => 1,
                None => 2,
            };
            self.vad_counts[index] += 1;
            return;
        }
        if let Err(error) = self.try_emit(client, request_id, event) {
            eprintln!("hear-core unilogs unavailable: {error}");
        }
    }

    pub fn flush_summary(&mut self) {
        let counts = std::mem::take(&mut self.vad_counts);
        for (index, count) in counts.into_iter().enumerate().filter(|(_, n)| *n > 0) {
            let mut event = LogEvent::simple("speech", LogLevel::Info, "vad_completed_summary");
            event.duration_ms = Some(self.summary_since.elapsed().as_millis() as u64);
            event.context = Some(LogContext {
                count: Some(count),
                source: [
                    Some(hear_protocol::AudioSource::Microphone),
                    Some(hear_protocol::AudioSource::System),
                    None,
                ][index],
                ..Default::default()
            });
            if let Err(error) = self.try_emit(ClientKind::Core, Uuid::new_v4(), event) {
                eprintln!("hear-core unilogs unavailable: {error}");
            }
        }
        self.summary_since = Instant::now();
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
        Ok(self.window(filter)?.entries)
    }

    pub fn window(&self, filter: &LogFilter) -> Result<LogWindow, String> {
        let mut window = LogWindow::default();
        for archive in (1..=ARCHIVES).rev() {
            read_matching(
                &self.directory.join(format!("hear.{archive:02}.jsonl")),
                filter,
                &mut window,
            )?;
        }
        read_matching(&self.directory.join("hear.jsonl"), filter, &mut window)?;
        window.truncated = window.matched > window.entries.len();
        Ok(window)
    }
}

fn read_matching(path: &Path, filter: &LogFilter, window: &mut LogWindow) -> Result<(), String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > LOG_BYTES {
        return Err("diagnostic log archive is not a bounded regular file".into());
    }
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let record = match serde_json::from_str::<LogRecord>(&line) {
            Ok(record) if valid_event(&record.event) => record,
            _ => {
                window.malformed_lines += 1;
                continue;
            }
        };
        window.retained_from_ms = Some(
            window
                .retained_from_ms
                .map_or(record.timestamp_ms, |t| t.min(record.timestamp_ms)),
        );
        window.retained_to_ms = Some(
            window
                .retained_to_ms
                .map_or(record.timestamp_ms, |t| t.max(record.timestamp_ms)),
        );
        if filter
            .component
            .as_ref()
            .is_some_and(|v| *v != record.event.component)
            || filter.level.is_some_and(|v| v != record.event.level)
            || filter
                .correlation_id
                .is_some_and(|v| record.event.correlation_id != Some(v))
            || filter
                .job_id
                .is_some_and(|v| record.event.job_id != Some(v))
            || filter.event_id.is_some_and(|v| {
                record.event.event_id != Some(v)
                    && !filter
                        .related_job_ids
                        .as_ref()
                        .is_some_and(|ids| record.event.job_id.is_some_and(|j| ids.contains(&j)))
            })
            || filter
                .capture_span_id
                .is_some_and(|v| record.event.capture_span_id != Some(v))
            || filter.from_ms.is_some_and(|v| record.timestamp_ms < v)
            || filter.to_ms.is_some_and(|v| record.timestamp_ms >= v)
        {
            continue;
        }
        window.matched += 1;
        if matches!(record.event.level, LogLevel::Warn | LogLevel::Error) {
            let key = format!(
                "{}.{}.{}",
                record.event.component,
                record.event.event,
                record.event.error_code.as_deref().unwrap_or("none")
            );
            if window.failure_counts.contains_key(&key) || window.failure_counts.len() < 128 {
                *window.failure_counts.entry(key).or_default() += 1;
            } else {
                window.failure_groups_truncated = true;
            }
        }
        let limit = filter.limit.unwrap_or(100).min(500);
        if limit > 0 {
            window.entries.push(record);
            if window.entries.len() > limit {
                window.entries.remove(0);
            }
        }
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
    token(&event.component)
        && token(&event.event)
        && event.error_code.as_deref().is_none_or(token)
        && event.context.as_ref().is_none_or(|c| {
            c.command.as_deref().is_none_or(token) && c.cause.as_deref().is_none_or(token)
        })
        && event.build.as_ref().is_none_or(|b| {
            matches!(b.component.as_str(), "core" | "cli" | "gui" | "worker")
                && b.git_sha.len() <= 64
                && b.git_sha.bytes().all(|c| c.is_ascii_alphanumeric())
                && b.version.len() <= 32
                && b.version.bytes().all(|c| c.is_ascii_digit() || c == b'.')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hear_protocol::LogLevel;

    #[test]
    fn log_filter_and_validation_exclude_free_text() {
        let directory = tempfile::tempdir().unwrap();
        let mut logs = UniLogs::open(directory.path(), Uuid::new_v4()).unwrap();
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
            ..Default::default()
        };
        let found = logs.read(&filter).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].event.event, "request");
    }

    #[test]
    fn correlation_connects_gui_and_core_without_personal_content() {
        let directory = tempfile::tempdir().unwrap();
        let mut logs = UniLogs::open(directory.path(), Uuid::new_v4()).unwrap();
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
    #[test]
    fn summaries_do_not_hide_failures_and_filters_report_truncation() {
        let directory = tempfile::tempdir().unwrap();
        let mut logs = UniLogs::open(directory.path(), Uuid::new_v4()).unwrap();
        let job = Uuid::new_v4();
        for _ in 0..700 {
            logs.emit(
                ClientKind::Core,
                job,
                LogEvent::simple("speech", LogLevel::Info, "vad_complete"),
            );
        }
        for _ in 0..3 {
            let mut failed = LogEvent::simple("speech", LogLevel::Warn, "vad_failed");
            failed.job_id = Some(job);
            failed.error_code = Some("local_analysis_publish_invalid_region".into());
            logs.emit(ClientKind::Core, job, failed);
        }
        logs.flush_summary();
        let window = logs
            .window(&LogFilter {
                limit: Some(500),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(window.entries.len(), 4);
        assert_eq!(
            window.entries[3].event.context.as_ref().unwrap().count,
            Some(700)
        );
        assert_eq!(window.failure_counts.values().sum::<usize>(), 3);
        let window = logs
            .window(&LogFilter {
                job_id: Some(job),
                limit: Some(1),
                ..Default::default()
            })
            .unwrap();
        assert!(window.truncated);
        assert_eq!(window.matched, 3);
        assert_eq!(window.entries.len(), 1);
        assert_eq!(window.failure_counts.values().sum::<usize>(), 3);
        let time = window.entries[0].timestamp_ms;
        let window = logs
            .window(&LogFilter {
                to_ms: Some(time),
                ..Default::default()
            })
            .unwrap();
        assert!(window.entries.iter().all(|r| r.timestamp_ms < time));
        let window = logs
            .window(&LogFilter {
                event_id: Some(Uuid::new_v4()),
                related_job_ids: Some(vec![job]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(window.matched, 3);
    }

    #[test]
    fn old_logs_parse_but_private_or_malformed_lines_are_not_exported() {
        let directory = tempfile::tempdir().unwrap();
        let logs = UniLogs::open(directory.path(), Uuid::new_v4()).unwrap();
        let id = Uuid::new_v4();
        let good = serde_json::json!({"timestamp_ms":42,"run_id":id,"elapsed_ms":0,"client":"core","request_id":id,"component":"speech","level":"warn","event":"coarse_failed","error_code":"failed"});
        let mut private = good.clone();
        private["error_code"] = serde_json::json!("/private/secret");
        fs::write(
            directory.path().join("logs/hear.jsonl"),
            format!("{good}\n{private}\ninvalid\n"),
        )
        .unwrap();
        let window = logs.window(&LogFilter::default()).unwrap();
        assert_eq!(window.matched, 1);
        assert_eq!(window.malformed_lines, 2);
        assert_eq!(window.retained_from_ms, Some(42));
        assert!(!serde_json::to_string(&window).unwrap().contains("private"));
    }
}
