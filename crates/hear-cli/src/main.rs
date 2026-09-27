use clap::{Parser, Subcommand, ValueEnum};
use hear_protocol::{
    AudioSource, ClientKind, LogFilter, LogLevel, Request, Response, send, send_from_with_id,
};
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
    Probe {
        source: Source,
        #[arg(long, default_value_t = 3)]
        seconds: u8,
    },
    Start,
    Pause,
    Resume,
    Source {
        source: Source,
        enabled: Switch,
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
        Command::Probe { source, seconds } => Request::Probe {
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
        Ok(response @ (Response::Status { .. } | Response::ProbeResult { .. })) => {
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
