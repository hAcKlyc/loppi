use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
};

use clap::{Args, Subcommand, ValueEnum};
use loppi_protocol::workflow::{Action, Config, ManualInput};
use loppi_protocol::{
    ClientKind, LifecycleAction, PROTOCOL_VERSION, Request, Response, send_existing,
    send_from_with_id,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Args)]
pub struct Workflow {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read or update the complete workflow configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// List configured strategies without reading their prompts.
    Strategies,
    /// Execute a strategy for an already determined event.
    Run {
        /// UUID of an event or a fixed, manually recognized range.
        event_id: Uuid,
        /// Strategy UUID returned by workflow strategies.
        strategy_id: Uuid,
        /// Revision returned by workflow strategies.
        #[arg(long)]
        config_revision: i64,
        /// Use configured input, explicit coarse text, or arrange the needed precise transcript.
        #[arg(long, value_enum, default_value = "configured")]
        input: InputChoice,
    },
    /// Read the progress of one or more runs.
    Runs {
        /// UUID of an event or a fixed, manually recognized range.
        event_id: Uuid,
        /// Filter by a run UUID returned when the action was accepted.
        #[arg(long)]
        run: Option<Uuid>,
        /// Filter by strategy UUID.
        #[arg(long)]
        strategy: Option<Uuid>,
        /// Continue a previous page using its next_cursor.
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Read the latest successful result or an exact version.
    Result {
        /// UUID of an event or a fixed, manually recognized range.
        event_id: Uuid,
        /// Strategy UUID returned by workflow strategies.
        strategy_id: Uuid,
        /// Read this successful version, defaulting to the latest successful one.
        #[arg(long)]
        version: Option<Uuid>,
        /// Continue a previous page using its next_cursor.
        #[arg(long)]
        cursor: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Read the complete configuration and its revision.
    Show,
    /// Replace the configuration. Use --preview for read-only validation.
    Set {
        /// JSON configuration document, or - for standard input.
        #[arg(long)]
        file: PathBuf,
        /// Validate and report affected rules without modifying settings.
        #[arg(long)]
        preview: bool,
        /// Also add missing trigger tags to the automatic ASR rules.
        #[arg(long)]
        add_required_asr_tags: bool,
    },
}

#[derive(Clone, ValueEnum)]
enum InputChoice {
    Configured,
    Coarse,
    PreparePrecise,
}

impl From<InputChoice> for ManualInput {
    fn from(choice: InputChoice) -> Self {
        match choice {
            InputChoice::Configured => Self::Configured,
            InputChoice::Coarse => Self::Coarse,
            InputChoice::PreparePrecise => Self::PreparePrecise,
        }
    }
}

fn read_config(path: PathBuf) -> Result<Config, String> {
    let mut data = Vec::new();
    let max = 512 * 1024;
    if path.as_os_str() == "-" {
        io::stdin().lock().take(max + 1).read_to_end(&mut data)
    } else {
        File::open(path).and_then(|file| file.take(max + 1).read_to_end(&mut data))
    }
    .map_err(|_| "无法读取配置文件".to_owned())?;
    if data.len() as u64 > max {
        return Err("配置文件过大".into());
    }
    serde_json::from_slice(&data)
        .map_err(|error| format!("配置 JSON 无效（第 {} 行）", error.line()))
}

pub fn parse_error(message: impl AsRef<str>) -> i32 {
    let id = Uuid::new_v4();
    emit(error(id, "invalid_argument", message.as_ref()), 2)
}

pub fn is_workflow_path(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    let mut args = args.into_iter();
    let mut command = || loop {
        let arg = args.next()?;
        match arg.to_str() {
            Some("--if-revision" | "--request-id") => {
                args.next()?;
            }
            Some(value)
                if value.starts_with("--if-revision=") || value.starts_with("--request-id=") => {}
            _ => return Some(arg),
        }
    };
    match command().as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("workflow") => true,
        Some("event") => matches!(
            command().as_deref().and_then(std::ffi::OsStr::to_str),
            Some("complete" | "confirm-range" | "tags")
        ),
        _ => false,
    }
}

fn error(id: Uuid, code: &str, message: &str) -> Value {
    json!({"ok":false,"request_id":id,"error":{"code":code,"message":message}})
}

fn emit(result: Value, code: i32) -> i32 {
    println!("{result}");
    code
}

pub fn execute(action: Action, id: Option<Uuid>) -> i32 {
    let id = id.unwrap_or_else(Uuid::new_v4);
    let existing = action.read_only();
    let request = Request::Workflow { action };
    let response = if existing {
        send_existing(ClientKind::Cli, request)
    } else {
        send_from_with_id(ClientKind::Cli, request, id)
    };
    match response {
        Ok(Response::Workflow { result, .. }) => {
            let code = if result["ok"] == true {
                0
            } else if matches!(
                result["error"]["code"].as_str(),
                Some("read_busy" | "query_timeout" | "db_locked")
            ) {
                4
            } else {
                5
            };
            emit(result, code)
        }
        Ok(Response::Error { code, message, .. }) => {
            let exit = if code == "protocol_version" { 3 } else { 5 };
            emit(error(id, &code, &message), exit)
        }
        Ok(_) => emit(error(id, "core_incompatible", "使用与应用配套的 CLI"), 3),
        Err(message) => {
            if message.starts_with("EOF while parsing")
                && matches!(send_existing(ClientKind::Cli, Request::Lifecycle {action:LifecycleAction::Inspect}),
                    Ok(Response::Lifecycle {build,..}) if build.protocol_version!=PROTOCOL_VERSION)
            {
                return emit(
                    error(id, "core_incompatible", "请使用与当前 loppi 应用配套的 CLI"),
                    3,
                );
            }
            let timed_out =
                message.contains("timed out") || message.contains("temporarily unavailable");
            let (code, exit) = if timed_out {
                ("request_timeout", 4)
            } else {
                ("core_unavailable", 3)
            };
            emit(
                error(
                    id,
                    code,
                    "请检查 Core 状态；写入结果不明时使用原请求 ID 核对",
                ),
                exit,
            )
        }
    }
}

pub fn run(args: Workflow, expected: Option<i64>, id: Option<Uuid>) -> i32 {
    let action = match args.command {
        Command::Config {
            command: ConfigCommand::Show,
        } => Action::ConfigShow,
        Command::Config {
            command:
                ConfigCommand::Set {
                    file,
                    preview,
                    add_required_asr_tags,
                },
        } => {
            let Some(expected_revision) = expected else {
                return parse_error("请提供 --if-revision");
            };
            let config = match read_config(file) {
                Ok(config) => config,
                Err(error) => return parse_error(error),
            };
            Action::ConfigSet {
                config,
                expected_revision,
                preview,
                add_required_asr_tags,
            }
        }
        Command::Strategies => Action::Strategies,
        Command::Run {
            event_id,
            strategy_id,
            config_revision,
            input,
        } => {
            let Some(expected_revision) = expected else {
                return parse_error("请提供 --if-revision");
            };
            Action::Run {
                event_id,
                strategy_id,
                expected_revision,
                config_revision,
                input: input.into(),
            }
        }
        Command::Runs {
            event_id,
            run,
            strategy,
            cursor,
        } => Action::Runs {
            event_id,
            run_id: run,
            strategy_id: strategy,
            cursor,
            latest_only: false,
        },
        Command::Result {
            event_id,
            strategy_id,
            version,
            cursor,
        } => Action::Result {
            event_id,
            strategy_id,
            version_id: version,
            cursor,
        },
    };
    execute(action, id)
}
