use clap::{Parser, Subcommand, ValueEnum};
use hear_protocol::{
    AudioSource, CleanupMode, ClientKind, CloudProvider, EventSources, LogFilter, LogLevel,
    MomentKind, Request, Response, send, send_from_with_id,
};
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "hear", version, about = "hear local command line")]
struct Cli {
    #[arg(long, global = true)]
    if_revision: Option<i64>,
    #[arg(long, global = true)]
    request_id: Option<Uuid>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Status,
    /// Build identity of this CLI; does not start Core.
    BuildInfo,
    /// Content-free report from an already running Core. Range is [from, to).
    Diagnose {
        #[arg(long)]
        from_ms: i64,
        #[arg(long)]
        to_ms: i64,
        #[arg(long)]
        event_id: Option<Uuid>,
        #[arg(long)]
        job_id: Option<Uuid>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Models {
        #[command(subcommand)]
        action: ModelCommand,
    },
    Probe {
        source: Source,
        #[arg(long, default_value_t = 3)]
        seconds: u8,
    },
    RecordTest {
        source: Source,
        #[arg(long, default_value_t = 10)]
        seconds: u8,
    },
    Start,
    Pause,
    Resume,
    Schedule {
        enabled: Switch,
        #[arg(long, value_parser = parse_schedule_time)]
        start: Option<u16>,
        #[arg(long, value_parser = parse_schedule_time)]
        stop: Option<u16>,
    },
    Source {
        source: Source,
        enabled: Switch,
    },
    AudioLimit {
        mebibytes: u64,
    },
    Cloud {
        #[command(subcommand)]
        action: CloudCommand,
    },
    ProcessNow,
    Timeline {
        from_ms: i64,
        to_ms: i64,
    },
    Moments {
        from_ms: i64,
        to_ms: i64,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    Mark {
        #[arg(long)]
        at_ms: Option<i64>,
    },
    Comment {
        text: String,
        #[arg(long)]
        at_ms: Option<i64>,
    },
    Screenshot,
    RangeDetail {
        started_ms: i64,
        ended_ms: i64,
        #[arg(long, default_value = "both")]
        sources: Sources,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    Event {
        #[command(subcommand)]
        action: EventCommand,
    },
    TranscribeRange {
        started_ms: i64,
        ended_ms: i64,
        #[arg(long)]
        sources: Sources,
    },
    Text {
        #[command(subcommand)]
        action: TextCommand,
    },
    Person {
        #[command(subcommand)]
        action: PersonCommand,
    },
    Speaker {
        #[command(subcommand)]
        action: SpeakerCommand,
    },
    Review {
        #[command(subcommand)]
        action: ReviewCommand,
    },
    Search {
        query: Option<String>,
        #[arg(long, default_value_t = 0)]
        from_ms: i64,
        #[arg(long,default_value_t=i64::MAX)]
        to_ms: i64,
        #[arg(long)]
        person: Option<Uuid>,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    Cleanup {
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        #[arg(long, default_value = "both")]
        sources: Sources,
        #[arg(long)]
        all_data: bool,
        #[arg(long)]
        preview_only: bool,
        /// Reuse the preview token from a previous cleanup with the same --request-id.
        #[arg(long)]
        retry_preview_token: Option<Uuid>,
    },
    Export {
        from_ms: i64,
        to_ms: i64,
        destination: PathBuf,
        #[arg(long, default_value = "both")]
        sources: Sources,
        #[arg(long)]
        audio: bool,
        #[arg(long)]
        screenshots: bool,
    },
    Logs {
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        level: Option<Level>,
        #[arg(long)]
        correlation: Option<Uuid>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        from_ms: Option<u64>,
        #[arg(long)]
        to_ms: Option<u64>,
        #[arg(long)]
        job_id: Option<Uuid>,
        #[arg(long)]
        event_id: Option<Uuid>,
        #[arg(long)]
        capture_span_id: Option<Uuid>,
    },
    Quit,
}

#[derive(Subcommand)]
enum ModelCommand {
    Status,
    Install,
}

#[derive(Subcommand)]
enum CloudCommand {
    Status,
    Usage {
        #[arg(long, value_delimiter = ',')]
        boundaries: Vec<i64>,
    },
    Tasks {
        provider: Provider,
        #[arg(long, default_value_t = 50)]
        limit: u16,
    },
    Configure {
        provider: Provider,
        #[arg(long)]
        model: Option<String>,
    },
    Pause {
        provider: Provider,
    },
    Resume {
        provider: Provider,
    },
    Retry {
        task_id: Uuid,
    },
}

#[derive(Subcommand)]
enum EventCommand {
    Show {
        event_id: Uuid,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    Create {
        started_ms: i64,
        ended_ms: i64,
        title: String,
        #[arg(long)]
        sources: Sources,
    },
    Edit {
        event_id: Uuid,
        revision: i64,
        started_ms: i64,
        ended_ms: i64,
        title: String,
        #[arg(long)]
        sources: Option<Sources>,
    },
    Retranscribe {
        event_id: Uuid,
        revision: i64,
    },
    Delete {
        event_id: Uuid,
        revision: i64,
    },
    Split {
        event_id: Uuid,
        revision: i64,
        at_ms: i64,
        left_title: String,
        right_title: String,
    },
    Merge {
        left_id: Uuid,
        left_revision: i64,
        right_id: Uuid,
        right_revision: i64,
        title: String,
    },
}

#[derive(Subcommand)]
enum TextCommand {
    Correct { segment_id: Uuid },
    Revert { segment_id: Uuid },
}

#[derive(Subcommand)]
enum PersonCommand {
    List,
    Samples {
        person_id: Uuid,
    },
    Create {
        display_name: String,
    },
    Rename {
        person_id: Uuid,
        display_name: String,
    },
    Delete {
        person_id: Uuid,
    },
}

#[derive(Subcommand)]
enum ReviewCommand {
    List,
    Apply { item_id: Uuid, segment_id: Uuid },
    Dismiss { item_id: Uuid },
}

#[derive(Subcommand)]
enum SpeakerCommand {
    List {
        segment_id: Uuid,
    },
    AssignLocal {
        segment_id: Uuid,
        target_segment_id: Uuid,
        #[arg(long)]
        segment_only: bool,
    },
    Bind {
        segment_id: Uuid,
        person_id: Uuid,
        #[arg(long)]
        segment_only: bool,
    },
    Unbind {
        segment_id: Uuid,
        #[arg(long)]
        segment_only: bool,
    },
    SampleAdd {
        segment_id: Uuid,
    },
    SampleRemove {
        sample_id: Uuid,
    },
}

#[derive(Clone, ValueEnum)]
enum Sources {
    Both,
    Microphone,
    System,
}
impl From<Sources> for EventSources {
    fn from(value: Sources) -> Self {
        match value {
            Sources::Both => Self::Both,
            Sources::Microphone => Self::Microphone,
            Sources::System => Self::System,
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Provider {
    Llm,
    Asr,
}
impl From<Provider> for CloudProvider {
    fn from(value: Provider) -> Self {
        match value {
            Provider::Llm => Self::Llm,
            Provider::Asr => Self::Asr,
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, ValueEnum)]
enum Source {
    Microphone,
    System,
}

#[derive(Clone, ValueEnum)]
enum Switch {
    On,
    Off,
}

fn current_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Command::BuildInfo => {
            println!(
                "{}",
                serde_json::to_string_pretty(&hear_protocol::build_info("cli")).unwrap()
            );
            return;
        }
        Command::Diagnose {
            from_ms,
            to_ms,
            event_id,
            job_id,
            output,
        } => {
            let result = (|| -> Result<(), String> {
                let response = hear_protocol::send_existing(
                    ClientKind::Cli,
                    Request::Diagnose {
                        filter: hear_protocol::DiagnosticFilter {
                            from_ms: *from_ms,
                            to_ms: *to_ms,
                            event_id: *event_id,
                            job_id: *job_id,
                        },
                    },
                )?;
                let Response::Diagnostic { report, .. } = response else {
                    return Err(
                        "diagnostics unavailable; check the time range and matching Core version"
                            .into(),
                    );
                };
                let data = serde_json::to_vec_pretty(&serde_json::json!({"cli_build":hear_protocol::build_info("cli"),"report":report})).map_err(|e|e.to_string())?;
                if let Some(path) = output {
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)
                        .map_err(|e| e.to_string())?;
                    file.write_all(&data)
                        .and_then(|_| file.write_all(b"\n"))
                        .map_err(|e| e.to_string())?;
                    println!("{}", path.display());
                } else {
                    println!("{}", String::from_utf8(data).unwrap());
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("hear: {error}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }
    if let Command::Cleanup {
        from_ms,
        to_ms,
        sources,
        all_data,
        preview_only,
        retry_preview_token,
    } = &cli.command
    {
        let (start, end) = match (from_ms, to_ms) {
            (None, None) => (0, i64::MAX),
            (Some(start), Some(end)) => (*start, *end),
            _ => {
                eprintln!("hear: supply both range boundaries, or neither for all history");
                std::process::exit(2)
            }
        };
        run_cleanup(
            start,
            end,
            sources.clone().into(),
            *all_data,
            *preview_only,
            cli.request_id,
            *retry_preview_token,
        );
        return;
    }
    let expected_revision = || -> i64 {
        if let Some(revision) = cli.if_revision {
            return revision;
        }
        match send(Request::Status) {
            Ok(Response::Status { state, .. }) => state.revision,
            Ok(Response::Error { code, message, .. }) => {
                eprintln!("hear: {code}: {message}");
                std::process::exit(2);
            }
            Ok(_) => {
                eprintln!("hear: unexpected status response");
                std::process::exit(1);
            }
            Err(error) => {
                eprintln!("hear: {error}");
                std::process::exit(1);
            }
        }
    };
    let object_revision = |kind: &str, object_id: Uuid| -> i64 {
        if let Some(revision) = cli.if_revision {
            return revision;
        }
        match send(Request::ObjectRevision {
            kind: kind.into(),
            object_id,
        }) {
            Ok(Response::ObjectRevision { revision, .. }) => revision,
            Ok(Response::Error { code, message, .. }) => {
                eprintln!("hear: {code}: {message}");
                std::process::exit(2)
            }
            Ok(_) => {
                eprintln!("hear: unexpected revision response");
                std::process::exit(1)
            }
            Err(error) => {
                eprintln!("hear: {error}");
                std::process::exit(1)
            }
        }
    };
    let request = match cli.command {
        Command::BuildInfo | Command::Diagnose { .. } => {
            unreachable!("local command handled above")
        }
        Command::Status => Request::Status,
        Command::Models { action } => match action {
            ModelCommand::Status => Request::Models,
            ModelCommand::Install => Request::PrepareModels,
        },
        Command::Probe { source, seconds } => Request::Probe {
            source: match source {
                Source::Microphone => AudioSource::Microphone,
                Source::System => AudioSource::System,
            },
            seconds,
        },
        Command::RecordTest { source, seconds } => Request::RecordTest {
            source: match source {
                Source::Microphone => AudioSource::Microphone,
                Source::System => AudioSource::System,
            },
            seconds,
        },
        Command::Start => Request::Start {
            expected_revision: expected_revision(),
        },
        Command::Pause => Request::Pause {
            expected_revision: expected_revision(),
        },
        Command::Resume => Request::Resume {
            expected_revision: expected_revision(),
        },
        Command::Schedule {
            enabled,
            start,
            stop,
        } => {
            let state = match send(Request::Status) {
                Ok(Response::Status { state, .. }) => state,
                other => {
                    eprintln!("hear: cannot read recording schedule: {other:?}");
                    std::process::exit(1);
                }
            };
            Request::SetRecordingSchedule {
                enabled: matches!(enabled, Switch::On),
                start_minute: start.unwrap_or(state.recording_schedule.start_minute),
                stop_minute: stop.unwrap_or(state.recording_schedule.stop_minute),
                expected_revision: cli.if_revision.unwrap_or(state.revision),
            }
        }
        Command::Quit => Request::Quit,
        Command::Cloud { action } => match action {
            CloudCommand::Status => Request::CloudStatus,
            CloudCommand::Usage { boundaries } => Request::UsageStatistics { boundaries },
            CloudCommand::Tasks { provider, limit } => Request::CloudTasks {
                provider: provider.into(),
                limit,
            },
            CloudCommand::Configure { provider, model } => {
                let api_key = if std::io::stdin().is_terminal() {
                    match rpassword::prompt_password("API key: ") {
                        Ok(value) => value,
                        Err(_) => {
                            eprintln!("hear: could not read credential from terminal");
                            std::process::exit(1);
                        }
                    }
                } else {
                    let mut value = String::new();
                    if std::io::stdin().read_to_string(&mut value).is_err() {
                        eprintln!("hear: could not read credential from standard input");
                        std::process::exit(1);
                    }
                    value
                };
                Request::ConfigureCloud {
                    provider: provider.into(),
                    api_key: api_key.trim().to_owned(),
                    model,
                }
            }
            CloudCommand::Pause { provider } => Request::SetCloudPaused {
                provider: provider.into(),
                paused: true,
            },
            CloudCommand::Resume { provider } => Request::SetCloudPaused {
                provider: provider.into(),
                paused: false,
            },
            CloudCommand::Retry { task_id } => Request::RetryCloudTask { task_id },
        },
        Command::ProcessNow => Request::ProcessNow,
        Command::Timeline { from_ms, to_ms } => Request::Timeline { from_ms, to_ms },
        Command::Moments {
            from_ms,
            to_ms,
            offset,
            limit,
        } => Request::Moments {
            from_ms,
            to_ms,
            offset,
            limit,
        },
        Command::Mark { at_ms } => Request::AddMoment {
            kind: MomentKind::Mark,
            anchor_ms: at_ms.unwrap_or_else(current_ms),
            text: None,
        },
        Command::Comment { text, at_ms } => Request::AddMoment {
            kind: MomentKind::Comment,
            anchor_ms: at_ms.unwrap_or_else(current_ms),
            text: Some(text),
        },
        Command::Screenshot => Request::CaptureScreenshot {
            excluded_window_ids: vec![],
            anchor_ms: None,
        },
        Command::RangeDetail {
            started_ms,
            ended_ms,
            sources,
            offset,
            limit,
        } => Request::RangeDetail {
            started_ms,
            ended_ms,
            sources: sources.into(),
            offset,
            limit,
        },
        Command::Event { action } => match action {
            EventCommand::Show {
                event_id,
                offset,
                limit,
            } => Request::EventDetail {
                event_id,
                offset,
                limit,
            },
            EventCommand::Create {
                started_ms,
                ended_ms,
                title,
                sources,
            } => Request::CreateEvent {
                started_ms,
                ended_ms,
                title,
                sources: sources.into(),
            },
            EventCommand::Edit {
                event_id,
                revision,
                started_ms,
                ended_ms,
                title,
                sources,
            } => Request::EditEvent {
                event_id,
                expected_revision: revision,
                started_ms,
                ended_ms,
                title,
                sources: sources.map(Into::into),
            },
            EventCommand::Retranscribe { event_id, revision } => Request::RetranscribeEvent {
                event_id,
                expected_revision: revision,
            },
            EventCommand::Delete { event_id, revision } => Request::DeleteEvent {
                event_id,
                expected_revision: revision,
            },
            EventCommand::Split {
                event_id,
                revision,
                at_ms,
                left_title,
                right_title,
            } => Request::SplitEvent {
                event_id,
                expected_revision: revision,
                at_ms,
                left_title,
                right_title,
            },
            EventCommand::Merge {
                left_id,
                left_revision,
                right_id,
                right_revision,
                title,
            } => Request::MergeEvents {
                left_id,
                left_revision,
                right_id,
                right_revision,
                title,
            },
        },
        Command::TranscribeRange {
            started_ms,
            ended_ms,
            sources,
        } => Request::TranscribeRange {
            started_ms,
            ended_ms,
            sources: sources.into(),
        },
        Command::Text { action } => match action {
            TextCommand::Correct { segment_id } => {
                let mut text = String::new();
                if std::io::stdin().read_to_string(&mut text).is_err() {
                    eprintln!("hear: could not read corrected text from standard input");
                    std::process::exit(1);
                }
                Request::CorrectText {
                    segment_id,
                    text: text.trim_end_matches('\n').to_owned(),
                    expected_revision: object_revision("segment", segment_id),
                }
            }
            TextCommand::Revert { segment_id } => Request::RevertText {
                segment_id,
                expected_revision: object_revision("segment", segment_id),
            },
        },
        Command::Person { action } => match action {
            PersonCommand::List => Request::People,
            PersonCommand::Samples { person_id } => Request::VoiceSamples { person_id },
            PersonCommand::Create { display_name } => Request::CreatePerson { display_name },
            PersonCommand::Rename {
                person_id,
                display_name,
            } => Request::RenamePerson {
                person_id,
                display_name,
                expected_revision: object_revision("person", person_id),
            },
            PersonCommand::Delete { person_id } => Request::DeletePerson {
                person_id,
                expected_revision: object_revision("person", person_id),
            },
        },
        Command::Speaker { action } => match action {
            SpeakerCommand::List { segment_id } => Request::SessionSpeakers { segment_id },
            SpeakerCommand::AssignLocal {
                segment_id,
                target_segment_id,
                segment_only,
            } => Request::AssignLocalSpeaker {
                segment_id,
                target_segment_id,
                entire_local_speaker: !segment_only,
                expected_revision: object_revision(
                    if segment_only {
                        "segment"
                    } else {
                        "speaker_group"
                    },
                    segment_id,
                ),
                target_revision: object_revision("speaker_group", target_segment_id),
            },
            SpeakerCommand::Bind {
                segment_id,
                person_id,
                segment_only,
            } => Request::BindSpeaker {
                segment_id,
                person_id,
                entire_local_speaker: !segment_only,
                expected_revision: object_revision(
                    if segment_only {
                        "segment"
                    } else {
                        "speaker_group"
                    },
                    segment_id,
                ),
            },
            SpeakerCommand::Unbind {
                segment_id,
                segment_only,
            } => Request::UnbindSpeaker {
                segment_id,
                entire_local_speaker: !segment_only,
                expected_revision: object_revision(
                    if segment_only {
                        "segment"
                    } else {
                        "speaker_group"
                    },
                    segment_id,
                ),
            },
            SpeakerCommand::SampleAdd { segment_id } => Request::AddVoiceSample {
                segment_id,
                expected_revision: object_revision("segment", segment_id),
            },
            SpeakerCommand::SampleRemove { sample_id } => Request::RemoveVoiceSample {
                sample_id,
                expected_revision: object_revision("sample", sample_id),
            },
        },
        Command::Review { action } => match action {
            ReviewCommand::List => Request::ReviewItems,
            ReviewCommand::Apply {
                item_id,
                segment_id,
            } => Request::ResolveReview {
                item_id,
                target_segment_id: Some(segment_id),
                expected_revision: object_revision("review", item_id),
            },
            ReviewCommand::Dismiss { item_id } => Request::ResolveReview {
                item_id,
                target_segment_id: None,
                expected_revision: object_revision("review", item_id),
            },
        },
        Command::Search {
            query,
            from_ms,
            to_ms,
            person,
            offset,
            limit,
        } => Request::Search {
            query,
            from_ms,
            to_ms,
            person_id: person,
            offset,
            limit,
        },
        Command::Cleanup { .. } => unreachable!("handled before request dispatch"),
        Command::Export {
            from_ms,
            to_ms,
            destination,
            sources,
            audio,
            screenshots,
        } => Request::ExportRange {
            from_ms,
            to_ms,
            destination,
            sources: sources.into(),
            include_audio: audio,
            include_screenshots: screenshots,
        },
        Command::Logs {
            from_ms,
            to_ms,
            job_id,
            event_id,
            capture_span_id,
            component,
            level,
            correlation,
            limit,
        } => Request::Logs {
            filter: LogFilter {
                from_ms,
                to_ms,
                job_id,
                event_id,
                capture_span_id,
                component,
                level: level.map(|value| match value {
                    Level::Debug => LogLevel::Debug,
                    Level::Info => LogLevel::Info,
                    Level::Warn => LogLevel::Warn,
                    Level::Error => LogLevel::Error,
                }),
                correlation_id: correlation,
                limit: Some(limit),
                ..Default::default()
            },
        },
        Command::Source { source, enabled } => Request::SetSource {
            source: match source {
                Source::Microphone => AudioSource::Microphone,
                Source::System => AudioSource::System,
            },
            enabled: matches!(enabled, Switch::On),
            expected_revision: expected_revision(),
        },
        Command::AudioLimit { mebibytes } => Request::SetAudioLimit {
            limit_bytes: mebibytes.saturating_mul(1_048_576),
            expected_revision: expected_revision(),
        },
    };

    match send_from_with_id(
        ClientKind::Cli,
        request,
        cli.request_id.unwrap_or_else(Uuid::new_v4),
    ) {
        Ok(response @ Response::CloudConfigurationReceipt { .. }) => {
            println!("{}", serde_json::to_string(&response).unwrap())
        }
        Ok(
            response @ (Response::Status { .. }
            | Response::Diagnostic { .. }
            | Response::Models { .. }
            | Response::ProbeResult { .. }
            | Response::RecordTestResult { .. }
            | Response::UsageStatistics { .. }
            | Response::CloudStatus { .. }
            | Response::CloudTasks { .. }
            | Response::Timeline { .. }
            | Response::CalendarDays { .. }
            | Response::Event { .. }
            | Response::Accepted { .. }
            | Response::TranscriptionQueued { .. }
            | Response::EventsChanged { .. }
            | Response::TextChanged { .. }
            | Response::SessionSpeakers { .. }
            | Response::People { .. }
            | Response::VoiceSamples { .. }
            | Response::ReviewItems { .. }
            | Response::ReviewResolved { .. }
            | Response::ObjectRevision { .. }
            | Response::PlaybackValid { .. }
            | Response::PersonChanged { .. }
            | Response::Search { .. }
            | Response::Moments { .. }
            | Response::MomentSaved { .. }
            | Response::CleanupPreview { .. }
            | Response::CleanupComplete { .. }
            | Response::ExportComplete { .. }),
        ) => {
            println!("{}", serde_json::to_string_pretty(&response).unwrap());
        }
        Ok(Response::PlaybackChunk { .. } | Response::ScreenshotData { .. }) => {
            eprintln!("hear: binary data response is unavailable from this command");
            std::process::exit(1);
        }
        Ok(Response::Logs { entries, .. }) => {
            for entry in entries {
                println!("{}", serde_json::to_string(&entry).unwrap());
            }
        }
        Ok(Response::Error { code, message, .. }) => {
            eprintln!("hear: {code}: {message}");
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("hear: {error}");
            std::process::exit(1);
        }
    }
}

fn run_cleanup(
    from_ms: i64,
    to_ms: i64,
    sources: EventSources,
    all_data: bool,
    preview_only: bool,
    request_id: Option<Uuid>,
    retry_preview_token: Option<Uuid>,
) {
    if retry_preview_token.is_some() && (request_id.is_none() || preview_only) {
        eprintln!(
            "hear: --retry-preview-token requires --request-id and cannot be used with --preview-only"
        );
        std::process::exit(2);
    }
    let mode = if all_data {
        CleanupMode::AllData
    } else {
        CleanupMode::RawAudio
    };
    let preview = if let Some(token) = retry_preview_token {
        eprintln!(
            "hear: retrying cleanup request {id} with preview token {token}",
            id = request_id.unwrap()
        );
        None
    } else {
        Some(
            match send(Request::PreviewCleanup {
                from_ms,
                to_ms,
                sources,
                mode,
            }) {
                Ok(Response::CleanupPreview { preview, .. }) => preview,
                Ok(Response::Error { code, message, .. }) => {
                    eprintln!("hear: {code}: {message}");
                    std::process::exit(2);
                }
                Ok(_) => {
                    eprintln!("hear: unexpected cleanup preview response");
                    std::process::exit(1);
                }
                Err(error) => {
                    eprintln!("hear: {error}");
                    std::process::exit(1);
                }
            },
        )
    };
    if let Some(preview) = &preview {
        println!("{}", serde_json::to_string_pretty(preview).unwrap());
    }
    if preview_only {
        return;
    }
    if all_data {
        // The separate native sheet is intentional, including CLI callers.
        // Without a graphical login session, osascript fails and no request is sent.
        let outcome=std::process::Command::new("/usr/bin/osascript")
            .args(["-e","display dialog \"将永久清除选中范围的录音、文字、笔记、截图和相关声纹样例。已经导出或发送给服务商的副本无法从这里撤回。\" with title \"清除全部数据\" buttons {\"取消\", \"清除全部数据\"} default button \"取消\" cancel button \"取消\" with icon caution"])
            .output();
        match outcome {
            Ok(result)
                if result.status.success()
                    && String::from_utf8_lossy(&result.stdout)
                        .contains("button returned:清除全部数据") => {}
            Ok(result) if result.status.success() => {
                eprintln!("hear: cleanup cancelled");
                return;
            }
            _ => {
                eprintln!("hear: a graphical macOS session is required to confirm 清除全部数据");
                std::process::exit(2);
            }
        }
    } else {
        if !std::io::stdin().is_terminal() {
            eprintln!("hear: an interactive terminal is required to confirm 清除原始音频");
            std::process::exit(2);
        }
        print!("确认清除原始音频？输入 清除 继续: ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() || answer.trim() != "清除" {
            eprintln!("hear: cleanup cancelled");
            return;
        }
    }
    match send_from_with_id(
        ClientKind::Cli,
        Request::ExecuteCleanup {
            preview_token: retry_preview_token.unwrap_or_else(|| preview.unwrap().token),
            confirmed_all_data: all_data,
        },
        request_id.unwrap_or_else(Uuid::new_v4),
    ) {
        Ok(result @ Response::CleanupComplete { .. }) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap())
        }
        Ok(Response::Error { code, message, .. }) => {
            eprintln!("hear: {code}: {message}");
            std::process::exit(2);
        }
        Ok(_) => {
            eprintln!("hear: unexpected cleanup response");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("hear: {error}");
            std::process::exit(1);
        }
    }
}

fn parse_schedule_time(value: &str) -> Result<u16, String> {
    let (hour, minute) = value.split_once(':').ok_or("expected HH:MM")?;
    let hour = hour.parse::<u16>().map_err(|_| "expected HH:MM")?;
    let minute = minute.parse::<u16>().map_err(|_| "expected HH:MM")?;
    if hour > 23 || minute > 59 {
        return Err("expected a time from 00:00 to 23:59".into());
    }
    Ok(hour * 60 + minute)
}
