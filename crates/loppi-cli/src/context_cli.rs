use clap::{Args, Subcommand, ValueEnum};
use loppi_protocol::{
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
    /// 输出格式；默认 json，text 供人阅读。
    #[arg(long, global = true, value_enum, default_value = "json")]
    format: Format,
    #[command(subcommand)]
    action: Action,
}
#[derive(Args)]
struct Time {
    /// today、yesterday 或 YYYY-MM-DD；与 --from/--to 互斥，list/digest 默认今天；read 必须明确选择日期/范围或 event/ref。
    #[arg(long,conflicts_with_all=["from","to"])]
    date: Option<String>,
    /// 包含的范围起点，RFC3339 且带时区偏移；须与 --to 一起使用。
    #[arg(long, requires = "to")]
    from: Option<String>,
    /// 不包含的范围终点，RFC3339 且带时区偏移；须与 --from 一起使用。
    #[arg(long, requires = "from")]
    to: Option<String>,
    /// 解析日期使用的 IANA 时区，如 Asia/Shanghai，默认机器时区。
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
    /// 分页条数；默认 20（read 为 50），范围 1–100（read 最大 200）。
    #[arg(long)]
    limit: Option<u16>,
    /// 原样传入上页 next_cursor，保留原查询条件。
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
    #[command(
        after_help = "例子：loppi context status\n只检查已有 Core；离线也是成功结果，不启动录音。"
    )]
    /// Availability only; never launches loppi.
    Status {
        /// 客户端实际暴露的 MCP 工具名列表，以逗号分隔，用于核对工具清单。
        #[arg(long, value_delimiter = ',')]
        client_tools: Option<Vec<String>>,
    },
    #[command(
        after_help = "例子：loppi context list --date today\n日期/范围最多 31 天，未整理范围与事件卡片一起返回；原文用 read。"
    )]
    /// Browse event cards and unorganized ranges. Defaults to today.
    List {
        /// 按事件的稳定标签 UUID 筛选。
        #[arg(long)]
        tag: Option<Uuid>,
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        /// 声音来源：both、microphone 或 system；省略读取两种来源。
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
    },
    #[command(
        after_help = "例子：loppi context digest --date yesterday --mode compact\n日期/范围最多 31 天；默认 compact，每个事件一行，需详情用 full 或 event。"
    )]
    /// Browse one compact row per event; use full mode for existing detail.
    Digest {
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        /// 声音来源：both、microphone 或 system；省略读取两种来源。
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
        /// compact 每事件一行含标题/topics/要点数量；full 展开已有摘要和理解。
        #[arg(long, default_value = "compact", value_parser = ["compact", "full"])]
        mode: String,
    },
    #[command(
        after_help = "例子：loppi context search 报价 --date yesterday\n不传时间搜索全部历史；人物筛选表示发言人身份。返回 next_cursor 时原样续页。"
    )]
    /// Literal keyword/phrase search, optionally filtered by an identified speaker.
    Search {
        /// 词法关键词或短语；无日期时搜索全部历史，省略时需提供 --person。
        query: Option<String>,
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        /// 已识别发言人的 UUID，从 people 获取；不是正文提及此人的筛选。
        #[arg(long)]
        person: Option<Uuid>,
        /// 声音来源：both、microphone 或 system；省略读取两种来源。
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
    },
    #[command(
        after_help = "例子：loppi context event EVENT_UUID --section key_points\n返回已有理解；摘要和结构化内容是派生信息，原话用 read 核对。"
    )]
    /// Overview or one structured section of an event.
    Event {
        /// 事件 UUID，从 list/digest/search 返回结果取得。
        event_id: Uuid,
        /// 事件部分：overview、summary、topics、key_points、mentioned_objects 或 moments。
        #[arg(long, default_value = "overview")]
        section: String,
        /// 是否返回覆盖与缺口信息；显式 true/false。
        #[arg(long,action=clap::ArgAction::Set)]
        include_coverage: Option<bool>,
        #[command(flatten)]
        page: Page,
    },
    #[command(
        after_help = "例子：loppi context read --event EVENT_UUID\n或：loppi context read --ref RETURNED_REF\n或：loppi context read --from 2026-11-01T09:00:00+08:00 --to 2026-11-01T10:00:00+08:00\n三种入口互斥；日期/范围最多 31 天。--ref 禁止附正文过滤参数；引用过期需重新定位，不拼接不同版本。"
    )]
    /// Read current text by event, time range, or returned evidence references.
    Read {
        /// 要读取原文的事件 UUID；与 --ref 和时间范围互斥。
        #[arg(long)]
        event: Option<Uuid>,
        /// 原样传入返回的证据引用，可重复 --ref；与 --event、时间范围及正文过滤参数互斥。
        #[arg(long = "ref")]
        refs: Vec<String>,
        #[command(flatten)]
        time: Time,
        #[command(flatten)]
        page: Page,
        /// best_available（默认）、coarse 或 precise；只用于事件/时间读取，不用于 --ref。
        #[arg(long)]
        transcript: Option<String>,
        /// 是否包含现场记录；默认 true，仅事件/时间读取适用，显式 true/false。
        #[arg(long,action=clap::ArgAction::Set)]
        include_moments: Option<bool>,
        /// 是否返回覆盖与缺口信息；显式 true/false。
        #[arg(long,action=clap::ArgAction::Set)]
        include_coverage: Option<bool>,
        /// 最少转录字符数；默认 1，0 保留空文字，仅事件/时间读取适用。
        #[arg(long)]
        min_chars: Option<u16>,
        /// 默认 false；true 还会隐藏与精转部分重叠的粗转，可能省略独有文字；仅事件/时间读取适用。
        #[arg(long,action=clap::ArgAction::Set)]
        dedupe: Option<bool>,
        /// 声音来源：both、microphone 或 system；省略读取两种来源。
        #[arg(long, value_enum)]
        sources: Option<super::Sources>,
    },
    #[command(
        after_help = "例子：loppi context people --name 张三\n返回人物 UUID 及显示名称，供 search --person 使用。"
    )]
    /// Resolve known speaker names to person IDs.
    People {
        /// 按人物名称查找发言人身份，省略时分页浏览人物。
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        page: Page,
    },
    #[command(
        after_help = "例子：loppi context schema read\n离线输出输入/输出 schema，不连接 Core。"
    )]
    /// Print the input and output schema, without contacting Core.
    Schema {
        /// 操作名：status、list、digest、search、event、read、people。
        operation: String,
    },
    #[command(
        after_help = "例子：loppi context guide evidence --format text\n保留主题参考资料；最新完整 Agent 指南与 Skill 版本用 loppi readme。"
    )]
    /// Read a short guide: cli, mcp, evidence, workflows, troubleshooting.
    Guide {
        /// 主题：cli、mcp、evidence、workflows、troubleshooting。
        topic: String,
    },
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
            tag,
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
            if let Some(tag) = tag {
                args["tag_id"] = json!(tag);
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
                        "{{LOPPI_CLI}}",
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
        /// 客户端实际暴露的 MCP 工具名，以逗号分隔，用于工具清单诊断。
        #[arg(long, value_delimiter = ',')]
        client_tools: Option<Vec<String>>,
    },
    /// Generate config; does not modify the Agent's configuration.
    Configure {
        /// 输出 generic JSON 或 codex TOML；只生成配置，不写入 Agent。
        #[arg(long,default_value="generic",value_parser=["generic","codex"])]
        target: String,
    },
    /// Install the bundled skill in an empty destination, preserving customized copies.
    InstallSkill {
        /// Skill 目标文件夹；相同内容幂等，拒绝覆盖不同的已有副本。
        #[arg(long)]
        destination: PathBuf,
    },
    /// Install a user-selected command symlink. Existing unrelated files are preserved.
    InstallCli {
        /// 命令启动器目录；不自动修改 PATH，不覆盖无关文件。
        #[arg(long)]
        directory: PathBuf,
    },
}
pub fn integrations(c: Integrations) -> i32 {
    let result = (|| -> Result<Value, String> {
        let cli = std::fs::canonicalize(std::env::current_exe().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        match c.action {
            Integration::Inspect => agent_assets::info(&cli),
            Integration::Doctor { client_tools } => {
                let mut info = agent_assets::info(&cli)?;
                info["status"] =
                    context::execute(Query::Status(context::StatusArgs { client_tools }));
                Ok(info)
            }
            Integration::Configure { target } => {
                let info = agent_assets::info(&cli)?;
                Ok(
                    json!({"target":target,"configuration":if target=="codex"{info["codex_config"].clone()}else{info["mcp_config"].clone()},"applied":false}),
                )
            }
            Integration::InstallSkill { destination } => {
                agent_assets::install_skill(&destination, &cli)?;
                Ok(json!({"installed":destination,"api_version":context::API_VERSION}))
            }
            Integration::InstallCli { directory } => {
                #[cfg(windows)]
                {
                    let link = crate::cli_launcher::install(&directory, &cli)?;
                    return Ok(json!({"installed":link,"target":cli}));
                }
                #[cfg(unix)]
                {
                    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                    let link = directory.join(loppi_protocol::platform::executable("loppi"));
                    if std::fs::symlink_metadata(&link).is_ok() {
                        if std::fs::canonicalize(&link).ok().as_ref() != Some(&cli) {
                            return Err(
                                "An unrelated loppi command exists in this directory".into()
                            );
                        }
                    } else {
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(&cli, &link).map_err(|e| e.to_string())?;
                    }
                    Ok(json!({"installed":link,"target":cli}))
                }
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
