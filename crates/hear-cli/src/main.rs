use clap::{Parser, Subcommand, ValueEnum};
use hear_protocol::{
    AudioSource, ClientKind, CloudProvider, EventSources, LogFilter, LogLevel, Request, Response,
    send, send_from_with_id,
};
use std::io::{IsTerminal, Read};
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
    Source {
        source: Source,
        enabled: Switch,
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
    Logs {
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        level: Option<Level>,
        #[arg(long)]
        correlation: Option<Uuid>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
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

fn main() {
    let cli = Cli::parse();
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
    let request = match cli.command {
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
        Command::Quit => Request::Quit,
        Command::Cloud { action } => match action {
            CloudCommand::Status => Request::CloudStatus,
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
        Command::Logs {
            component,
            level,
            correlation,
            limit,
        } => Request::Logs {
            filter: LogFilter {
                component,
                level: level.map(|value| match value {
                    Level::Debug => LogLevel::Debug,
                    Level::Info => LogLevel::Info,
                    Level::Warn => LogLevel::Warn,
                    Level::Error => LogLevel::Error,
                }),
                correlation_id: correlation,
                limit: Some(limit),
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
    };

    match send_from_with_id(
        ClientKind::Cli,
        request,
        cli.request_id.unwrap_or_else(Uuid::new_v4),
    ) {
        Ok(
            response @ (Response::Status { .. }
            | Response::Models { .. }
            | Response::ProbeResult { .. }
            | Response::RecordTestResult { .. }
            | Response::CloudStatus { .. }
            | Response::CloudTasks { .. }
            | Response::Timeline { .. }
            | Response::Event { .. }
            | Response::Accepted { .. }
            | Response::EventsChanged { .. }),
        ) => {
            println!("{}", serde_json::to_string_pretty(&response).unwrap());
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
