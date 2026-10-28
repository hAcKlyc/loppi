use clap::{Args, Subcommand, ValueEnum};
use hear_protocol::{
    agent_assets,
    context::{self, Query},
};
use serde_json::{Value, json};
use std::path::PathBuf;
use uuid::Uuid;
#[derive(Clone, ValueEnum)]
pub enum Format {
    Json,
    Text,
}
#[derive(Args)]
pub struct Context {
    #[arg(long, global = true, value_enum, default_value = "json")]
    format: Format,
    #[command(subcommand)]
    action: Action,
}
#[derive(Args)]
struct Time {
    #[arg(long,conflicts_with_all=["from","to"])]
    date: Option<String>,
    #[arg(long, requires = "to")]
    from: Option<String>,
    #[arg(long, requires = "from")]
    to: Option<String>,
    #[arg(long)]
    timezone: Option<String>,
}
impl Time {
    fn value(&self) -> Option<Value> {
        if self.date.is_none()
            && self.from.is_none()
            && self.to.is_none()
            && self.timezone.is_none()
        {
            return None;
        }
        let mut v = json!({});
        for (k, s) in [
            ("date", &self.date),
            ("from", &self.from),
            ("to", &self.to),
            ("timezone", &self.timezone),
        ] {
            if let Some(s) = s {
                v[k] = json!(s);
            }
        }
        Some(v)
    }
}
#[derive(Args)]
struct Page {
    #[arg(long)]
    limit: Option<u16>,
    #[arg(long)]
    cursor: Option<String>,
}
impl Page {
    fn add(&self, v: &mut Value) {
        if let Some(n) = self.limit {
            v["limit"] = json!(n);
        }
        if let Some(s) = &self.cursor {
            v["cursor"] = json!(s);
        }
    }
}
#[derive(Subcommand)]
enum Action {
    /// Availability only; never launches hear.
    Status {
        #[arg(long, value_delimiter = ',')]
        client_tools: Option<Vec<String>>,
    },
    /// Browse event cards and unorganized ranges. Defaults to today.
    List {
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
    },
    /// Browse one compact row per event; use full mode for existing detail.
    Digest {
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
        #[arg(long, default_value = "compact", value_parser = ["compact", "full"])]
        mode: String,
    },
    /// Literal keyword/phrase search, optionally filtered by an identified speaker.
    Search {
        query: Option<String>,
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        #[arg(long)]
        person: Option<Uuid>,
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
    },
    /// Overview or one structured section of an event.
    Event {
        event_id: Uuid,
        #[arg(long, default_value = "overview")]
        section: String,
        #[arg(long,action=clap::ArgAction::Set)]
        include_coverage: Option<bool>,
        #[command(flatten)]
        page: Page,
    },
    /// Read current text by event, time range, or returned evidence references.
    Read {
        #[arg(long)]
        event: Option<Uuid>,
        #[arg(long = "ref")]
        refs: Vec<String>,
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        #[arg(long)]
        transcript: Option<String>,
        #[arg(long,action=clap::ArgAction::Set)]
        include_moments: Option<bool>,
        #[arg(long,action=clap::ArgAction::Set)]
        include_coverage: Option<bool>,
        #[arg(long)]
        min_chars: Option<u16>,
        #[arg(long,action=clap::ArgAction::Set)]
        dedupe: Option<bool>,
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
    },
    /// Resolve known speaker names to person IDs.
    People {
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        page: Page,
    },
    /// Print the input and output schema, without contacting Core.
    Schema { operation: String },
    /// Read a short guide: cli, mcp, evidence, workflows, troubleshooting.
    Guide { topic: String },
}
fn source(s: &super::Sources) -> Value {
    json!(match s {
        super::Sources::Both => "both",
        super::Sources::Microphone => "microphone",
        super::Sources::System => "system",
    })
}
pub fn run(c: Context) -> i32 {
    let mut args = json!({});
    let op = match c.action {
        Action::Status { client_tools } => {
            if let Some(v) = client_tools {
                args["client_tools"] = json!(v);
            }
            "status"
        }
        Action::List {
            time,
            page,
            sources,
        } => {
            if let Some(t) = time.value() {
                args["time"] = t;
            }
            if let Some(s) = sources {
                args["sources"] = source(&s);
            }
            page.add(&mut args);
            "list"
        }
        Action::Digest {
            time,
            page,
            sources,
            mode,
        } => {
            if let Some(t) = time.value() {
                args["time"] = t;
            }
            if let Some(s) = sources {
                args["sources"] = source(&s);
            }
            args["mode"] = json!(mode);
            page.add(&mut args);
            "digest"
        }
        Action::Search {
            query,
            time,
            page,
            person,
            sources,
        } => {
            if let Some(t) = time.value() {
                args["time"] = t;
            }
            if let Some(s) = sources {
                args["sources"] = source(&s);
            }
            if let Some(q) = query {
                args["query"] = json!(q);
            }
            if let Some(p) = person {
                args["person_id"] = json!(p);
            }
            page.add(&mut args);
            "search"
        }
        Action::Event {
            event_id,
            section,
            include_coverage,
            page,
        } => {
            args["event_id"] = json!(event_id);
            args["section"] = json!(section);
            if let Some(v) = include_coverage {
                args["include_coverage"] = json!(v);
            }
            page.add(&mut args);
            "event"
        }
        Action::Read {
            event,
            refs,
            time,
            page,
            transcript,
            include_moments,
            include_coverage,
            min_chars,
            dedupe,
            sources,
        } => {
            let mut selector = json!({});
            if let Some(e) = event {
                selector["event_id"] = json!(e);
            }
            if !refs.is_empty() {
                selector["refs"] = json!(refs);
            }
            if let Some(t) = time.value() {
                selector["time"] = t;
            }
            if let Some(s) = sources {
                selector["sources"] = source(&s);
            }
            args["selector"] = selector;
            if let Some(t) = transcript {
                args["transcript"] = json!(t);
            }
            if let Some(m) = include_moments {
                args["include_moments"] = json!(m);
            }
            if let Some(v) = include_coverage {
                args["include_coverage"] = json!(v);
            }
            if let Some(v) = min_chars {
                args["min_chars"] = json!(v);
            }
            if let Some(v) = dedupe {
                args["dedupe"] = json!(v);
            }
            page.add(&mut args);
            "read"
        }
        Action::People { name, page } => {
            if let Some(n) = name {
                args["name"] = json!(n);
            }
            page.add(&mut args);
            "people"
        }
        Action::Schema { operation } => {
            return match context::input_schema(&operation) {
                Some(schema) => {
                    print(
                        &json!({"inputSchema":schema,"outputSchema":context::output_schema()}),
                        &c.format,
                    );
                    0
                }
                None => emit_error("invalid_argument", "Unknown context operation"),
            };
        }
        Action::Guide { topic } => {
            return match agent_assets::guide(&topic) {
                Some(guide) => {
                    let text = guide.replace(
                        "{{HEAR_CLI}}",
                        &std::env::current_exe()
                            .unwrap_or_default()
                            .to_string_lossy(),
                    );
                    if matches!(c.format, Format::Text) {
                        println!("{text}");
                    } else {
                        print(&json!({"topic":topic,"text":text}), &c.format);
                    }
                    0
                }
                None => emit_error(
                    "invalid_argument",
                    "Unknown guide topic; choose: cli, mcp, evidence, workflows, troubleshooting",
                ),
            };
        }
    };
    let result = match Query::parse(op, args) {
        Ok(q) => context::execute(q),
        Err(e) => context::error(Uuid::new_v4(), "invalid_argument", e),
    };
    print(&result, &c.format);
    exit_code(&result)
}
pub fn emit_error(code: &str, message: impl AsRef<str>) -> i32 {
    let v = context::error(Uuid::new_v4(), code, message);
    println!("{}", v);
    exit_code(&v)
}
fn print(v: &Value, format: &Format) {
    if matches!(format, Format::Json) {
        println!("{v}");
    } else {
        println!("{}", serde_json::to_string_pretty(v).unwrap());
    }
}
pub fn exit_code(v: &Value) -> i32 {
    if v["ok"] != false {
        return 0;
    }
    match v["error"]["code"].as_str().unwrap_or("") {
        "invalid_argument" => 2,
        "core_unavailable" | "core_incompatible" => 3,
        "read_busy" | "query_timeout" => 4,
        _ => 5,
    }
}

#[derive(Args)]
pub struct Integrations {
    #[command(subcommand)]
    action: Integration,
}
#[derive(Subcommand)]
enum Integration {
    /// Show actual CLI location and ready-to-copy configuration.
    Inspect,
    /// Verify Core connectivity without starting it.
    Doctor {
        #[arg(long, value_delimiter = ',')]
        client_tools: Option<Vec<String>>,
    },
    /// Generate config; does not modify the Agent's configuration.
    Configure {
        #[arg(long,default_value="generic",value_parser=["generic","codex"])]
        target: String,
    },
    /// Install the bundled skill in an empty destination, preserving customized copies.
    InstallSkill {
        #[arg(long)]
        destination: PathBuf,
    },
    /// Install a user-selected command symlink. Existing unrelated files are preserved.
    InstallCli {
        #[arg(long)]
        directory: PathBuf,
    },
}
pub fn integrations(c: Integrations) -> i32 {
    let result = (|| -> Result<Value, String> {
        let cli = std::fs::canonicalize(std::env::current_exe().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let info = agent_assets::info(&cli);
        match c.action {
            Integration::Inspect => Ok(info),
            Integration::Doctor { client_tools } => {
                let mut info = info;
                info["status"] =
                    context::execute(Query::Status(context::StatusArgs { client_tools }));
                Ok(info)
            }
            Integration::Configure { target } => Ok(
                json!({"target":target,"configuration":if target=="codex"{info["codex_config"].clone()}else{info["mcp_config"].clone()},"applied":false}),
            ),
            Integration::InstallSkill { destination } => {
                agent_assets::install_skill(&destination, &cli)?;
                Ok(json!({"installed":destination,"api_version":context::API_VERSION}))
            }
            Integration::InstallCli { directory } => {
                std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                let link = directory.join("hear");
                if std::fs::symlink_metadata(&link).is_ok() {
                    if std::fs::canonicalize(&link).ok().as_ref() != Some(&cli) {
                        return Err("An unrelated hear command exists in this directory".into());
                    }
                } else {
                    std::os::unix::fs::symlink(&cli, &link).map_err(|e| e.to_string())?;
                }
                Ok(json!({"installed":link,"target":cli}))
            }
        }
    })();
    match result {
        Ok(v) => {
            println!("{v}");
            if v["status"]["data"]["tool_inventory"]["state"] == "mismatch" {
                5
            } else if v["status"]["ok"] == false || v["status"]["data"]["online"] == false {
                3
            } else {
                0
            }
        }
        Err(e) => emit_error("invalid_argument", e),
    }
}
