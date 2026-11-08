#[cfg(windows)]
mod cli_launcher;
mod context_cli;
mod mcp;
mod workflow_cli;
use clap::{Parser, Subcommand, ValueEnum};
use loppi_protocol::{
    AudioSource, CleanupMode, ClientKind, CloudProvider, EventSources, LogFilter, LogLevel,
    MomentKind, Request, Response, send, send_from_with_id,
};
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "loppi",
    version,
    about = "日常声音与现场记录的查询和管理 CLI",
    long_about = "查询和管理本机 loppi：记录、转录、事件、人物、词库、识别队列、导出和清理。先用 readme 获取当前 Agent 指南与 Skill 版本，再逐级查看命令 --help。readme/help 不连接 Core。",
    after_help = "例子：\n  loppi readme\n  loppi context digest --date yesterday\n  loppi context read --help\n  loppi schedule --help\n\n输出：context 默认 JSON（可用 --format text），其他管理命令一般为 JSON，logs 为 JSONL。失败检查 stderr/退出码。管理命令 *_ms 均为 Unix 毫秒，范围为 [起点,终点)。写入按用户请求执行；重试需复用 request-id、原适用修订与最终负载；省略修订会重新查询，未知结果先核实。LOPPI_DATA_DIR 可指定数据目录。"
)]
struct Cli {
    /// 预期修订：状态控制用全局 revision，text/person/speaker/review 用对应对象 revision；省略会查询，事件用位置参数 revision。
    #[arg(long, global = true)]
    if_revision: Option<i64>,
    /// 写入请求 UUID；幂等重试需复用原适用修订与最终负载，仅重复 CLI 参数不足以保证，未知结果先核实。
    #[arg(long, global = true)]
    request_id: Option<Uuid>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum VocabularyCommand {
    /// 读取规范名词库。
    List,
    /// 新增或更新词条；同名更新，别名列表完整替换，改名用 --id。
    #[command(
        after_help = "例子：loppi vocabulary set MyAgents --kind product --alias 我的助手\n同名更新；--id 用于改名。aliases 完整替换；--enabled false 暂停注入。只影响后续处理，不全局改写历史。"
    )]
    Set {
        /// 规范名称，最多 100 字。
        name: String,
        /// 词条类型，如 term、product、project，最多 50 字。
        #[arg(long, default_value = "term")]
        kind: String,
        /// 别名，可重复传 --alias；本次列表完整替换旧别名，最多 20 个。
        #[arg(long = "alias")]
        aliases: Vec<String>,
        /// 已有词条 UUID；改名时传入以保留身份，省略时按规范名称匹配。
        #[arg(long)]
        id: Option<Uuid>,
        /// 是否启用；布尔选项显式传 true/false，开关位置参数使用 on/off。
        #[arg(long,default_value_t=true,action=clap::ArgAction::Set)]
        enabled: bool,
    },
    /// 删除词条，不改写历史转录。
    Remove {
        /// 要删除的词条 UUID，必填，从 vocabulary list 获取。
        id: Uuid,
    },
    /// 设置词库对各引擎的注入开关，未指定项保持原值。
    Configure {
        /// 是否将词库注入后续 LLM 处理；true/false。
        #[arg(long,action=clap::ArgAction::Set)]
        llm: Option<bool>,
        /// 是否将词库注入后续本地识别；true/false。
        #[arg(long,action=clap::ArgAction::Set)]
        local_asr: Option<bool>,
        /// 是否将词库注入后续云精转；true/false。
        #[arg(long,action=clap::ArgAction::Set)]
        cloud_asr: Option<bool>,
    },
}
#[derive(Subcommand)]
enum UnderstandingCommand {
    /// 从已有粗转重建事件摘要和理解，可能产生 LLM 费用；不重跑 ASR。
    Rebuild {
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        event_id: Uuid,
    },
}

#[derive(Subcommand)]
enum Command {
    #[command(
        after_help = "例子：loppi readme\n输出当前完整指南、独立 Skill 版本、官方来源和 CLI 实际路径；离线提供，不连接 Core。"
    )]
    /// 读取当前 Agent 指南、Skill 版本及官方来源目录；不连接 Core。
    Readme,
    #[cfg(windows)]
    #[command(hide = true)]
    PrepareInstall,
    /// Read existing context for an Agent. Never launches Core.
    Context(context_cli::Context),
    /// Manage event workflow rules and generated results.
    Workflow(workflow_cli::Workflow),
    /// Local stdio Model Context Protocol server.
    Mcp {
        #[command(subcommand)]
        action: McpCommand,
    },
    /// Connection configuration, skill installation and diagnostics.
    Integrations(context_cli::Integrations),
    /// 读取录音、容量、声源及持久意图状态。
    Status,
    /// Build identity of this CLI; does not start Core.
    BuildInfo,
    #[command(
        after_help = "例子：loppi diagnose --from-ms 1793500000000 --to-ms 1793510000000\n只向已运行的 Core 查询脱敏报告；--output 指定新的 JSON 文件。"
    )]
    /// Content-free report from an already running Core. Range is [from, to).
    Diagnose {
        /// 范围起点，Unix 毫秒，包含该时刻。
        #[arg(long)]
        from_ms: i64,
        /// 范围终点，Unix 毫秒，不包含该时刻。
        #[arg(long)]
        to_ms: i64,
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        #[arg(long)]
        event_id: Option<Uuid>,
        /// 按处理任务 UUID 过滤。
        #[arg(long)]
        job_id: Option<Uuid>,
        /// 诊断 JSON 输出文件；必须是尚不存在的路径，省略则输出到 stdout。
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// 查看或重新校验随包离线语音模型。
    Models {
        #[command(subcommand)]
        action: ModelCommand,
    },
    /// 实际探测指定声源权限和采集状态；会短暂采集。
    Probe {
        /// 声音来源：microphone 为麦克风，system 为电脑声音。
        source: Source,
        /// 实际探测秒数，范围 1–20 秒。
        #[arg(long, default_value_t = 3)]
        seconds: u8,
    },
    /// 实际录制指定声源的验证样本；会采集音频。
    RecordTest {
        /// 声音来源：microphone 为麦克风，system 为电脑声音。
        source: Source,
        /// 实际试录秒数，范围 1–60 秒。
        #[arg(long, default_value_t = 10)]
        seconds: u8,
    },
    /// 启用录音意图，按权限、模型、计划、来源和容量条件开始记录。
    Start,
    /// 暂停录音并保存暂停意图。
    Pause,
    /// 恢复录音意图；实际采集仍受权限、容量和计划约束。
    Resume,
    /// 配置按星期重复的本地录制计划。
    #[command(
        after_help = "例子：loppi schedule on --start 09:00 --stop 18:00 --days mon,tue,wed,thu,fri\n时间按机器本地时区，跨午夜按开始日归属。关闭计划：loppi schedule off。保存后从下一个定时点生效。"
    )]
    Schedule {
        /// 是否启用；布尔选项显式传 true/false，开关位置参数使用 on/off。
        enabled: Switch,
        /// 开始的本地时间 HH:MM（00:00–23:59）；省略沿用已保存的开始时间。
        #[arg(long, value_parser = parse_schedule_time)]
        start: Option<u16>,
        /// 停止的本地时间 HH:MM；允许跨午夜；省略沿用已保存的停止时间。
        #[arg(long, value_parser = parse_schedule_time)]
        stop: Option<u16>,
        /// 逗号分隔的录制开始日，省略沿用已保存的星期。
        #[arg(long, value_enum, value_delimiter = ',')]
        days: Vec<ScheduleDay>,
    },
    /// 启用或关闭指定声音来源。
    Source {
        /// 声音来源：microphone 为麦克风，system 为电脑声音。
        source: Source,
        /// 是否启用；布尔选项显式传 true/false，开关位置参数使用 on/off。
        enabled: Switch,
    },
    /// 设置音频容量上限；达到上限暂停录音，不覆盖旧记录。
    #[command(
        after_help = "例子：loppi audio-limit 10240\n单位 MiB；有效范围 64–1048576 MiB。降低到低于已用容量时暂停录音，提高后按状态手动恢复。"
    )]
    AudioLimit {
        /// 音频上限，单位 MiB（1 MiB = 1048576 字节）。
        mebibytes: u64,
    },
    /// 查看和管理识别队列、供应商配置及用量。
    Cloud {
        #[command(subcommand)]
        action: CloudCommand,
    },
    /// 请求立即检查当前待处理内容，复用现有处理队列。
    ProcessNow,
    /// Manage canonical terms, aliases and per-engine injection switches.
    Vocabulary {
        #[command(subcommand)]
        action: VocabularyCommand,
    },
    /// Explicitly regenerate existing understanding using retained coarse text.
    Understanding {
        #[command(subcommand)]
        action: UnderstandingCommand,
    },
    /// 读取指定 Unix 毫秒范围内的事件时间线。
    Timeline {
        /// 范围起点，Unix 毫秒，包含该时刻。
        from_ms: i64,
        /// 范围终点，Unix 毫秒，不包含该时刻。
        to_ms: i64,
    },
    /// 分页读取标记、评论和截图记录。
    Moments {
        /// 范围起点，Unix 毫秒，包含该时刻。
        from_ms: i64,
        /// 范围终点，Unix 毫秒，不包含该时刻。
        to_ms: i64,
        /// 从零开始的分页偏移；事件/范围详情最大 1000000，续页保留原查询条件。
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// 本页条数，默认见下方；详情/搜索/现场记录为 1–200，云任务截断到 1–200，日志最多 500。
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    /// 在指定时刻保存标记，省略时间使用当前时刻。
    Mark {
        /// 锚定/拆分时刻，Unix 毫秒；标记和评论省略时使用当前时刻。
        #[arg(long)]
        at_ms: Option<i64>,
    },
    /// 保存现场评论，省略时间使用当前时刻。
    Comment {
        /// 要保存的评论正文。
        text: String,
        /// 锚定/拆分时刻，Unix 毫秒；标记和评论省略时使用当前时刻。
        #[arg(long)]
        at_ms: Option<i64>,
    },
    /// 主动截图并保存原图/OCR；已配置的云端理解可能产生费用。
    Screenshot,
    /// 分页读取时间范围内的转录与记录详情。
    RangeDetail {
        /// 开始时刻，Unix 毫秒，包含该时刻。
        started_ms: i64,
        /// 结束时刻，Unix 毫秒，不包含该时刻且必须晚于开始。
        ended_ms: i64,
        /// 声音来源：both 为麦克风和电脑声音，或选 microphone/system。
        #[arg(long, default_value = "both")]
        sources: Sources,
        /// 从零开始的分页偏移；事件/范围详情最大 1000000，续页保留原查询条件。
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// 本页条数，默认见下方；详情/搜索/现场记录为 1–200，云任务截断到 1–200，日志最多 500。
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    /// 读取、新建、修订、拆合、重组或删除事件。
    Event {
        #[command(subcommand)]
        action: EventCommand,
    },
    /// 为指定范围和声源请求云精转，可能产生供应商费用。
    #[command(
        after_help = "例子：loppi transcribe-range 1793500000000 1793510000000 --sources both\n提交异步云精转；状态和错误通过 cloud tasks 查询，可能产生供应商费用。"
    )]
    TranscribeRange {
        /// 开始时刻，Unix 毫秒，包含该时刻。
        started_ms: i64,
        /// 结束时刻，Unix 毫秒，不包含该时刻且必须晚于开始。
        ended_ms: i64,
        /// 声音来源：both 为麦克风和电脑声音，或选 microphone/system。
        #[arg(long)]
        sources: Sources,
    },
    /// 从标准输入纠正文字，或撤销片段纠正。
    Text {
        #[command(subcommand)]
        action: TextCommand,
    },
    /// 管理人物名称和声纹样例。
    Person {
        #[command(subcommand)]
        action: PersonCommand,
    },
    /// 管理匿名说话人分组、人物绑定及声纹样例。
    Speaker {
        #[command(subcommand)]
        action: SpeakerCommand,
    },
    /// 处理历史修订的待复核条目。
    Review {
        #[command(subcommand)]
        action: ReviewCommand,
    },
    /// 按词法关键词和 Unix 毫秒范围搜索记录；person 筛选发言人身份。
    #[command(
        after_help = "例子：loppi search 报价 --from-ms 1793500000000 --to-ms 1793510000000\n省略 query 时可按时间/人物检索。Agent 日期查询优先使用 context search，支持日期与游标。"
    )]
    Search {
        /// 词法关键词或短语；不进行自然语言语义搜索。
        query: Option<String>,
        /// 范围起点，Unix 毫秒，包含该时刻。
        #[arg(long, default_value_t = 0)]
        from_ms: i64,
        /// 范围终点，Unix 毫秒，不包含该时刻。
        #[arg(long,default_value_t=i64::MAX)]
        to_ms: i64,
        /// 发言人 UUID 筛选，表示身份，不是正文提及此人。
        #[arg(long)]
        person: Option<Uuid>,
        /// 从零开始的分页偏移；事件/范围详情最大 1000000，续页保留原查询条件。
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// 本页条数，默认见下方；详情/搜索/现场记录为 1–200，云任务截断到 1–200，日志最多 500。
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    /// 预览并清理范围资料；默认清原声，--all-data 清全部数据，执行需交互确认。
    #[command(
        after_help = "例子：loppi cleanup 1793500000000 1793510000000 --preview-only\n不传两个范围参数时预览/清理全部历史。执行默认原声清理需要交互终端输入“清除”；--all-data 需要图形会话系统确认。不能在非交互 Agent 中绕过确认。"
    )]
    Cleanup {
        /// 范围起点，Unix 毫秒，包含该时刻。
        from_ms: Option<i64>,
        /// 范围终点，Unix 毫秒，不包含该时刻。
        to_ms: Option<i64>,
        /// 声音来源：both 为麦克风和电脑声音，或选 microphone/system。
        #[arg(long, default_value = "both")]
        sources: Sources,
        /// 清理范围内原声、文字、评论、截图及相关派生内容；默认只清原声，执行前系统弹窗确认。
        #[arg(long)]
        all_data: bool,
        /// 只读取影响预览，不执行删除或弹出执行确认。
        #[arg(long)]
        preview_only: bool,
        /// Reuse the preview token from a previous cleanup with the same --request-id.
        #[arg(long)]
        retry_preview_token: Option<Uuid>,
    },
    /// 导出范围内的文字和可选原音/截图为 tar 文件，不覆盖已有目标。
    #[command(
        after_help = "例子：loppi export 1793500000000 1793510000000 /path/history.tar --audio --screenshots\n仅导出仍可用的范围内容；目标必须不存在。失败检查退出码，不假定目标文件代表成功。"
    )]
    Export {
        /// 范围起点，Unix 毫秒，包含该时刻。
        from_ms: i64,
        /// 范围终点，Unix 毫秒，不包含该时刻。
        to_ms: i64,
        /// 目标文件路径，必须尚不存在；父目录需已存在且可写。
        destination: PathBuf,
        /// 声音来源：both 为麦克风和电脑声音，或选 microphone/system。
        #[arg(long, default_value = "both")]
        sources: Sources,
        /// 导出时包含范围内仍保留的原声音频。
        #[arg(long)]
        audio: bool,
        /// 导出时包含范围内仍保留的截图原图。
        #[arg(long)]
        screenshots: bool,
    },
    /// 读取脱敏运行日志，按行输出 JSON；不含记录正文或密钥。
    Logs {
        /// 按日志组件名过滤。
        #[arg(long)]
        component: Option<String>,
        /// 日志等级过滤。
        #[arg(long)]
        level: Option<Level>,
        /// 按关联请求 UUID 过滤。
        #[arg(long)]
        correlation: Option<Uuid>,
        /// 本页条数，默认见下方；详情/搜索/现场记录为 1–200，云任务截断到 1–200，日志最多 500。
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// 范围起点，Unix 毫秒，包含该时刻。
        #[arg(long)]
        from_ms: Option<u64>,
        /// 范围终点，Unix 毫秒，不包含该时刻。
        #[arg(long)]
        to_ms: Option<u64>,
        /// 按处理任务 UUID 过滤。
        #[arg(long)]
        job_id: Option<Uuid>,
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        #[arg(long)]
        event_id: Option<Uuid>,
        /// 按采集跨度 UUID 过滤。
        #[arg(long)]
        capture_span_id: Option<Uuid>,
    },
    /// 停止 Core 及其采集活动，保存本次登录的退出状态。
    Quit,
}

#[derive(Subcommand)]
enum McpCommand {
    /// 启动本机 stdio MCP 服务；标准输入/输出供协议使用，读取已有 Core 记录。
    Serve,
}

#[derive(Subcommand)]
enum ModelCommand {
    /// 读取随包模型检查状态。
    Status,
    /// 校验并准备随包本地模型，不修改历史转录。
    Install,
}

#[derive(Subcommand)]
enum CloudCommand {
    /// 读取云队列配置、暂停和处理状态。
    Status,
    /// 读取按给定日边界聚合的用量与费用估算，不是实际账单。
    #[command(
        after_help = "例子：loppi cloud usage --boundaries 1793462400000,1793548800000,1793635200000,1793721600000,1793808000000,1793894400000,1793980800000,1794067200000\n必须提供 8/31/91 个日边界，每段 20–28 小时。金额是估算，未知用量不等于零。"
    )]
    Usage {
        /// 逗号分隔的递增 Unix 毫秒日边界，必须有 8、31 或 91 个，相邻边界形成 7、30 或 90 天，每段 20–28 小时。
        #[arg(long, value_delimiter = ',')]
        boundaries: Vec<i64>,
    },
    /// 读取指定供应商的近期任务和错误状态。
    Tasks {
        /// 供应商队列：llm（DeepSeek）或 asr（火山精转）。
        provider: Provider,
        /// 本页条数，默认见下方；详情/搜索/现场记录为 1–200，云任务截断到 1–200，日志最多 500。
        #[arg(long, default_value_t = 50)]
        limit: u16,
    },
    /// 从终端隐藏输入或标准输入读取 API key，验证并保存到系统凭据库，可能产生验证费用。
    #[command(
        after_help = "例子：loppi cloud configure llm\n交互终端隐藏输入 API key；自动化通过标准输入提供，避免把密钥放入参数/日志。验证成功后存入系统凭据库。"
    )]
    Configure {
        /// 供应商队列：llm（DeepSeek）或 asr（火山精转）。
        provider: Provider,
        /// 供应商模型标识；省略使用该队列的默认模型。
        #[arg(long)]
        model: Option<String>,
    },
    /// 暂停供应商队列的新提交，不撤回已发送请求。
    Pause {
        /// 供应商队列：llm（DeepSeek）或 asr（火山精转）。
        provider: Provider,
    },
    /// 恢复供应商队列并继续有效积压。
    Resume {
        /// 供应商队列：llm（DeepSeek）或 asr（火山精转）。
        provider: Provider,
    },
    /// 重试指定失败任务，沿用任务身份与现有重试规则。
    Retry {
        /// 云任务 UUID，从 cloud tasks 返回值取得。
        task_id: Uuid,
    },
}

#[derive(Subcommand)]
enum EventCommand {
    /// Confirm an existing event has ended, then evaluate automatic rules.
    Complete {
        /// Event UUID from context event or event show.
        event_id: Uuid,
        /// Expected event revision, from the latest event read.
        revision: i64,
        /// Optional adjusted start (inclusive Unix milliseconds).
        #[arg(long, requires = "to_ms")]
        from_ms: Option<i64>,
        /// Optional adjusted end (exclusive Unix milliseconds).
        #[arg(long, requires = "from_ms")]
        to_ms: Option<i64>,
    },
    /// Confirm a historical unorganized time range as an event.
    ConfirmRange {
        /// Inclusive Unix milliseconds of the selected historical range.
        from: i64,
        /// Exclusive Unix milliseconds of the selected historical range.
        to: i64,
        /// Source or sources within the selected range.
        #[arg(long, value_enum)]
        sources: Sources,
        /// Optional title for the new event.
        #[arg(long)]
        title: Option<String>,
    },
    /// Preview or apply the complete tag selection for an event.
    Tags {
        /// Event UUID from context event or event show.
        event_id: Uuid,
        /// Expected event revision, from the latest event read.
        revision: i64,
        /// Repeat to specify the complete set of selected tag UUIDs.
        #[arg(long, required = true)]
        tag: Vec<Uuid>,
        /// Apply the previously previewed plan using its returned token.
        #[arg(long)]
        apply: Option<String>,
    },

    #[command(
        after_help = "例子：loppi event reorganize 1793500000000 1793510000000 \"按讨论主题重新整理\"\nLLM 队列暂停/未配置时等待；重组结果由 Core 校验后发布，任务可能产生费用。"
    )]
    /// Reorganize existing events with a one-time user instruction.
    Reorganize {
        /// 开始时刻，Unix 毫秒，包含该时刻。
        started_ms: i64,
        /// 结束时刻，Unix 毫秒，不包含该时刻且必须晚于开始。
        ended_ms: i64,
        /// 事件标题；reorganize 时作为一次性重组指示。
        title: String,
        /// 声音来源：both 为麦克风和电脑声音，或选 microphone/system。
        #[arg(long, default_value = "both")]
        sources: Sources,
    },
    /// 分页读取事件详情及当前事件 revision。
    Show {
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        event_id: Uuid,
        /// 从零开始的分页偏移；事件/范围详情最大 1000000，续页保留原查询条件。
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// 本页条数，默认见下方；详情/搜索/现场记录为 1–200，云任务截断到 1–200，日志最多 500。
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    /// 在指定范围人工创建事件。
    Create {
        /// 开始时刻，Unix 毫秒，包含该时刻。
        started_ms: i64,
        /// 结束时刻，Unix 毫秒，不包含该时刻且必须晚于开始。
        ended_ms: i64,
        /// 事件标题；reorganize 时作为一次性重组指示。
        title: String,
        /// 声音来源：both 为麦克风和电脑声音，或选 microphone/system。
        #[arg(long)]
        sources: Sources,
    },
    /// 按当前事件 revision 修改边界、标题或声音来源。
    Edit {
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        event_id: Uuid,
        /// 预期事件 revision，从 event show 等当前结果取得，不是全局状态 revision。
        revision: i64,
        /// 开始时刻，Unix 毫秒，包含该时刻。
        started_ms: i64,
        /// 结束时刻，Unix 毫秒，不包含该时刻且必须晚于开始。
        ended_ms: i64,
        /// 事件标题；reorganize 时作为一次性重组指示。
        title: String,
        /// 声音来源：both 为麦克风和电脑声音，或选 microphone/system。
        #[arg(long)]
        sources: Option<Sources>,
    },
    /// 为事件重新请求精转，可能产生 ASR 费用。
    Retranscribe {
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        event_id: Uuid,
        /// 预期事件 revision，从 event show 等当前结果取得，不是全局状态 revision。
        revision: i64,
    },
    /// 删除事件及其整理归属；不等于清除范围内全部原声和记录。
    #[command(
        after_help = "例子：loppi event delete EVENT_UUID REVISION\n删除事件整理归属与抑制自动重建；需要清除原声或其他记录时使用 cleanup 并先预览。"
    )]
    Delete {
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        event_id: Uuid,
        /// 预期事件 revision，从 event show 等当前结果取得，不是全局状态 revision。
        revision: i64,
    },
    /// 按当前事件 revision 在指定时刻拆成两个事件。
    Split {
        /// 事件 UUID，从 timeline/context/event show 返回值取得。
        event_id: Uuid,
        /// 预期事件 revision，从 event show 等当前结果取得，不是全局状态 revision。
        revision: i64,
        /// 锚定/拆分时刻，Unix 毫秒；标记和评论省略时使用当前时刻。
        at_ms: i64,
        /// 拆分后左侧事件标题。
        left_title: String,
        /// 拆分后右侧事件标题。
        right_title: String,
    },
    /// 按两个事件当前 revision 合并事件。
    Merge {
        /// 左侧事件 UUID。
        left_id: Uuid,
        /// 左侧事件当前 revision。
        left_revision: i64,
        /// 右侧事件 UUID。
        right_id: Uuid,
        /// 右侧事件当前 revision。
        right_revision: i64,
        /// 事件标题；reorganize 时作为一次性重组指示。
        title: String,
    },
}

#[derive(Subcommand)]
enum TextCommand {
    /// 从标准输入读取完整纠正文字；自动获取当前对象修订再写入。
    #[command(
        after_help = "例子：loppi text correct SEGMENT_UUID < corrected.txt\n读取标准输入至 EOF 作为完整替换文本；参数是片段 UUID。"
    )]
    Correct {
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
    },
    /// 撤销该片段的人工文字纠正，自动获取当前对象修订。
    Revert {
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
    },
}

#[derive(Subcommand)]
enum PersonCommand {
    /// 读取人物列表。
    List,
    /// 读取该人物的声纹样例元信息。
    Samples {
        /// 人物 UUID，从 person list 或 context people 返回值取得。
        person_id: Uuid,
    },
    /// 创建人物名称。
    Create {
        /// 人物显示名称。
        display_name: String,
    },
    /// 修改人物名称，自动获取当前对象修订。
    Rename {
        /// 人物 UUID，从 person list 或 context people 返回值取得。
        person_id: Uuid,
        /// 人物显示名称。
        display_name: String,
    },
    /// 删除人物及相关身份关联；不等于清除原始录音。
    Delete {
        /// 人物 UUID，从 person list 或 context people 返回值取得。
        person_id: Uuid,
    },
}

#[derive(Subcommand)]
enum ReviewCommand {
    /// 读取需要复核的历史修订。
    List,
    /// 将待复核条目关联到指定当前片段。
    Apply {
        /// 待复核条目 UUID，从 review list 取得。
        item_id: Uuid,
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
    },
    /// 忽略待复核条目。
    Dismiss {
        /// 待复核条目 UUID，从 review list 取得。
        item_id: Uuid,
    },
}

#[derive(Subcommand)]
enum SpeakerCommand {
    /// 读取与该片段相关的局部说话人分组。
    List {
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
    },
    /// 把片段或整组归到目标片段的匿名说话人分组。
    AssignLocal {
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
        /// 目标当前转录片段 UUID。
        target_segment_id: Uuid,
        /// 只处理当前片段；省略时处理同一局部说话人整组。
        #[arg(long)]
        segment_only: bool,
    },
    /// 将片段或整组说话人绑定到人物。
    Bind {
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
        /// 人物 UUID，从 person list 或 context people 返回值取得。
        person_id: Uuid,
        /// 只处理当前片段；省略时处理同一局部说话人整组。
        #[arg(long)]
        segment_only: bool,
    },
    /// 解除片段或整组的人物绑定。
    Unbind {
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
        /// 只处理当前片段；省略时处理同一局部说话人整组。
        #[arg(long)]
        segment_only: bool,
    },
    /// 从指定片段加入声纹样例，原声与质量需满足入库条件。
    SampleAdd {
        /// 转录片段 UUID，从详情/读取结果取得。
        segment_id: Uuid,
    },
    /// 移除声纹样例，不等于删除原始音频。
    SampleRemove {
        /// 声纹样例 UUID，从 person samples 返回值取得。
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

#[derive(Clone, Copy, ValueEnum)]
enum ScheduleDay {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

fn current_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn main() {
    #[cfg(windows)]
    if let Err(error) = loppi_protocol::platform::initialize_process() {
        eprintln!("loppi: {error}");
        std::process::exit(1);
    }
    #[cfg(windows)]
    match cli_launcher::forward_if_installed() {
        Ok(Some(code)) => std::process::exit(code),
        Ok(None) => {}
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if std::env::args().nth(1).as_deref() == Some("context") && error.use_stderr() {
                std::process::exit(context_cli::emit_error(
                    "invalid_argument",
                    error.to_string(),
                ));
            }
            if error.use_stderr() && workflow_cli::is_workflow_path(std::env::args_os().skip(1)) {
                std::process::exit(workflow_cli::parse_error(error.to_string()));
            }
            error.exit();
        }
    };
    match cli.command {
        #[cfg(windows)]
        Command::PrepareInstall => {
            let result = (|| -> Result<(), String> {
                let own = std::env::current_exe().map_err(|e| e.to_string())?;
                let directory = own.parent().ok_or("installation directory missing")?;
                loppi_protocol::platform::stop_installed_gui(directory)
                    .map_err(|e| e.to_string())?;
                loppi_protocol::quit_gui_core()?;
                loppi_protocol::platform::wait_installed_helpers(directory)
                    .map_err(|e| e.to_string())
            })();
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
            println!("{}", serde_json::json!({"stopped":true,"forced":false}));
            return;
        }
        Command::Context(command) => std::process::exit(context_cli::run(command)),
        Command::Workflow(command) => {
            std::process::exit(workflow_cli::run(command, cli.if_revision, cli.request_id))
        }
        Command::Integrations(command) => std::process::exit(context_cli::integrations(command)),
        Command::Mcp {
            action: McpCommand::Serve,
        } => {
            if let Err(error) = mcp::serve() {
                eprintln!("loppi MCP: {error}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }
    match &cli.command {
        Command::Readme => {
            let result = std::env::current_exe()
                .and_then(std::fs::canonicalize)
                .map_err(|e| e.to_string())
                .and_then(|cli| loppi_protocol::agent_assets::readme(&cli));
            match result {
                Ok(text) => println!("{text}"),
                Err(error) => {
                    eprintln!("loppi: {error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Command::BuildInfo => {
            println!(
                "{}",
                serde_json::to_string_pretty(&loppi_protocol::build_info("cli")).unwrap()
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
                let response = loppi_protocol::send_existing(
                    ClientKind::Cli,
                    Request::Diagnose {
                        filter: loppi_protocol::DiagnosticFilter {
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
                let data = serde_json::to_vec_pretty(&serde_json::json!({"cli_build":loppi_protocol::build_info("cli"),"report":report})).map_err(|e|e.to_string())?;
                if let Some(path) = output {
                    #[cfg(unix)]
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut file = loppi_protocol::platform::OpenOptions::new()
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
                eprintln!("loppi: {error}");
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
                eprintln!("loppi: supply both range boundaries, or neither for all history");
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
                eprintln!("loppi: {code}: {message}");
                std::process::exit(2);
            }
            Ok(_) => {
                eprintln!("loppi: unexpected status response");
                std::process::exit(1);
            }
            Err(error) => {
                eprintln!("loppi: {error}");
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
                eprintln!("loppi: {code}: {message}");
                std::process::exit(2)
            }
            Ok(_) => {
                eprintln!("loppi: unexpected revision response");
                std::process::exit(1)
            }
            Err(error) => {
                eprintln!("loppi: {error}");
                std::process::exit(1)
            }
        }
    };
    let request = match cli.command {
        #[cfg(windows)]
        Command::PrepareInstall => unreachable!("local command handled above"),
        Command::Readme
        | Command::Workflow(_)
        | Command::Context(_)
        | Command::Integrations(_)
        | Command::Mcp { .. }
        | Command::BuildInfo
        | Command::Diagnose { .. } => {
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
            days,
        } => {
            let state = match send(Request::Status) {
                Ok(Response::Status { state, .. }) => state,
                other => {
                    eprintln!("loppi: cannot read recording schedule: {other:?}");
                    std::process::exit(1);
                }
            };
            Request::SetRecordingSchedule {
                enabled: matches!(enabled, Switch::On),
                start_minute: start.unwrap_or(state.recording_schedule.start_minute),
                stop_minute: stop.unwrap_or(state.recording_schedule.stop_minute),
                weekday_mask: (!days.is_empty())
                    .then(|| days.iter().fold(0, |mask, day| mask | (1 << *day as u8))),
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
                            eprintln!("loppi: could not read credential from terminal");
                            std::process::exit(1);
                        }
                    }
                } else {
                    let mut value = String::new();
                    if std::io::stdin().read_to_string(&mut value).is_err() {
                        eprintln!("loppi: could not read credential from standard input");
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
        Command::Understanding {
            action: UnderstandingCommand::Rebuild { event_id },
        } => Request::RebuildUnderstanding { event_id },
        Command::Vocabulary { action } => Request::Vocabulary {
            action: match action {
                VocabularyCommand::List => loppi_protocol::vocabulary::Action::List,
                VocabularyCommand::Set {
                    id,
                    name,
                    kind,
                    aliases,
                    enabled,
                } => loppi_protocol::vocabulary::Action::Upsert {
                    id,
                    name,
                    kind,
                    aliases,
                    enabled,
                },
                VocabularyCommand::Remove { id } => {
                    loppi_protocol::vocabulary::Action::Remove { id }
                }
                VocabularyCommand::Configure {
                    llm,
                    local_asr,
                    cloud_asr,
                } => loppi_protocol::vocabulary::Action::Configure {
                    llm,
                    local_asr,
                    cloud_asr,
                },
            },
        },
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
            EventCommand::Complete {
                event_id,
                revision,
                from_ms,
                to_ms,
            } => std::process::exit(workflow_cli::execute(
                loppi_protocol::workflow::Action::Complete {
                    event_id,
                    expected_revision: revision,
                    started_ms: from_ms,
                    ended_ms: to_ms,
                },
                cli.request_id,
            )),
            EventCommand::ConfirmRange {
                from,
                to,
                sources,
                title,
            } => std::process::exit(workflow_cli::execute(
                loppi_protocol::workflow::Action::ConfirmRange {
                    started_ms: from,
                    ended_ms: to,
                    sources: sources.into(),
                    title,
                },
                cli.request_id,
            )),
            EventCommand::Tags {
                event_id,
                revision,
                tag,
                apply,
            } => std::process::exit(workflow_cli::execute(
                loppi_protocol::workflow::Action::Tags {
                    event_id,
                    expected_revision: revision,
                    tag_ids: tag,
                    plan: apply,
                },
                cli.request_id,
            )),

            EventCommand::Reorganize {
                started_ms,
                ended_ms,
                title,
                sources,
            } => Request::ReorganizeEvents {
                started_ms,
                ended_ms,
                title,
                sources: sources.into(),
            },
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
                    eprintln!("loppi: could not read corrected text from standard input");
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
        Ok(Response::Workflow { result, .. }) => {
            println!("{result}");
            std::process::exit(if result["ok"] == true { 0 } else { 5 });
        }
        Ok(Response::Context { result, .. }) => {
            println!("{result}");
            std::process::exit(context_cli::exit_code(&result));
        }
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
            | Response::CalendarAdjacent { .. }
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
            eprintln!("loppi: binary data response is unavailable from this command");
            std::process::exit(1);
        }
        Ok(Response::Logs { entries, .. }) => {
            for entry in entries {
                println!("{}", serde_json::to_string(&entry).unwrap());
            }
        }
        Ok(Response::Lifecycle { .. }) => {
            eprintln!("loppi: unexpected lifecycle response");
            std::process::exit(2);
        }
        Ok(Response::Error { code, message, .. }) => {
            eprintln!("loppi: {code}: {message}");
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("loppi: {error}");
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
            "loppi: --retry-preview-token requires --request-id and cannot be used with --preview-only"
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
            "loppi: retrying cleanup request {id} with preview token {token}",
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
                    eprintln!("loppi: {code}: {message}");
                    std::process::exit(2);
                }
                Ok(_) => {
                    eprintln!("loppi: unexpected cleanup preview response");
                    std::process::exit(1);
                }
                Err(error) => {
                    eprintln!("loppi: {error}");
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
        #[cfg(windows)]
        if !loppi_protocol::platform::confirm_cleanup() {
            eprintln!("loppi: cleanup cancelled");
            return;
        }
        #[cfg(unix)]
        {
            // The separate native sheet is intentional, including CLI callers.
            // Without a graphical login session, osascript fails and no request is sent.
            let outcome=loppi_protocol::platform::command("/usr/bin/osascript")
            .args(["-e","display dialog \"将永久清除选中范围的录音、文字、笔记、截图和相关声纹样例。已经导出或发送给服务商的副本无法从这里撤回。\" with title \"清除全部数据\" buttons {\"取消\", \"清除全部数据\"} default button \"取消\" cancel button \"取消\" with icon caution"])
            .output();
            match outcome {
                Ok(result)
                    if result.status.success()
                        && String::from_utf8_lossy(&result.stdout)
                            .contains("button returned:清除全部数据") => {}
                Ok(result) if result.status.success() => {
                    eprintln!("loppi: cleanup cancelled");
                    return;
                }
                _ => {
                    eprintln!(
                        "loppi: a graphical macOS session is required to confirm 清除全部数据"
                    );
                    std::process::exit(2);
                }
            }
        }
    } else {
        if !std::io::stdin().is_terminal() {
            eprintln!("loppi: an interactive terminal is required to confirm 清除原始音频");
            std::process::exit(2);
        }
        print!("确认清除原始音频？输入 清除 继续: ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() || answer.trim() != "清除" {
            eprintln!("loppi: cleanup cancelled");
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
            eprintln!("loppi: {code}: {message}");
            std::process::exit(2);
        }
        Ok(_) => {
            eprintln!("loppi: unexpected cleanup response");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("loppi: {error}");
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

#[cfg(test)]
mod agent_help_tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn read_help_matches_schema_and_write_help_distinguishes_revisions() {
        let mut command = Cli::command();
        command.build();
        let read = command
            .find_subcommand("context")
            .unwrap()
            .find_subcommand("read")
            .unwrap();
        let dedupe = read
            .get_arguments()
            .find(|a| a.get_id() == "dedupe")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        let schema = loppi_protocol::context::input_schema("read").unwrap();
        assert!(dedupe.contains(&format!(
            "默认 {}",
            schema["properties"]["dedupe"]["default"]
        )));
        assert!(dedupe.contains("独有文字"));
        let rename = command
            .find_subcommand("person")
            .unwrap()
            .find_subcommand("rename")
            .unwrap();
        let revision = rename
            .get_arguments()
            .find(|a| a.get_id() == "if_revision")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        assert!(revision.contains("对象 revision"));
        let request = rename
            .get_arguments()
            .find(|a| a.get_id() == "request_id")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        assert!(request.contains("最终负载"));
        for (name, range) in [("probe", "1–20"), ("record-test", "1–60")] {
            let seconds = command
                .find_subcommand(name)
                .unwrap()
                .get_arguments()
                .find(|a| a.get_id() == "seconds")
                .unwrap()
                .get_help()
                .unwrap()
                .to_string();
            assert!(seconds.contains(range));
        }
    }
    #[test]
    fn every_public_command_and_argument_explains_its_use() {
        fn check(command: &clap::Command, path: String) {
            if command.is_hide_set() {
                return;
            }
            assert!(
                command.get_about().is_some() || command.get_long_about().is_some(),
                "missing command help: {path}"
            );
            for arg in command.get_arguments().filter(|arg| !arg.is_hide_set()) {
                assert!(
                    arg.get_help().is_some() || arg.get_long_help().is_some(),
                    "missing argument help: {path} {}",
                    arg.get_id()
                );
            }
            for child in command.get_subcommands() {
                check(child, format!("{path} {}", child.get_name()));
            }
        }
        let mut command = Cli::command();
        command.build();
        command.clone().debug_assert();
        check(&command, "loppi".into());
    }
}
