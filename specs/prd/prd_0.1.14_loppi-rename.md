---
type: prd
status: implemented
created: 2026-10-31
updated: 2026-10-31
scope: "将产品展示、源码、发行产物、系统身份、存储、凭据和 Agent 接入一次统一为全新的 loppi；不保留 hear 兼容层或自动迁移，不扩展产品功能，不代替开源许可或发布决策。"
issue: "产品正式更名为 loppi；用户已接受方案并授权完整开发、验证和 review。"
review: "实现、最终统一验证、三视角独立 review、真实签名包系统验收及干净源码 Universal2 发行验证全部通过；无待决策或未完成必需项。"
---

# hear → loppi 更名 PRD

## 执行须知（给空 session 的你）

用户已明确接受本 PRD 并要求完整开发、验证和 review。产品名为 **`loppi`**，只有用户自己使用，要求“一次性不留债务的直接改成新的”“不需要考虑兼容性问题”。因此按全新系统身份、存储、凭据和接口设计，不建设 hear 兼容或自动迁移能力；不主动修改旧数据或替换本机已安装应用，不对外发布。

开始实施前先读 [整体架构](../ARCHITECTURE.md)、[Agent Context 契约](../tech_docs/agent-context.md)、[离线资源与 Mac 打包](../tech_docs/offline-macos-packaging.md)及第 4 节列出的代码入口。开发核对基线为 `f2f38b0`，工作树版本 `0.1.14`、内部 IPC 协议 `24`；已有文档中的协议版本和工具数量存在滞后，以当前代码为准。文件名中的版本用于归档，不预定更名版的发行版本。

第 3 节 D1–D4 已按用户指示收敛，用户已接受完整 PRD 并要求开发。全部更名、文档和验证是一次原子交付；工程批次只用于实现、验证、review 和 commit。第 7 节是完成门槛；实际证据和未通过项记入执行台账。代码引用使用真实文件与符号，不依赖行号。

## 1. 背景、目标与已确认事项

产品帮助用户在电脑可用期间持续留下声音和主动截图，将记录整理成可定位的事件、文字、人物和证据，再由用户或外部 Agent 找回。它降低的是“现在要不要记、之后到哪里找”的成本。更名服务于产品正式对外传播，以及全球开源社区和中文用户的识别与使用。

用户原话：“那就打算正式更名了就叫 loppi”；随后要求整理仓库改动并保存 PRD，之后明确接受整份方案并要求开发。以下为已接受的交付范围：

- 产品名采用小写 `loppi`，中文说明直接使用该名称，不另造中文产品名。
- 当前兔子形象可以继续承载品牌，本次不要求重新设计图标、角色或视觉系统。
- 目标是完成产品更名；记录、整理、回放、清理及 Agent 取证的既有语义沿用。
- 正式发布、开源是背景意向；本 PRD 不选择许可证、不授权公开仓库或上传发行产物。

补充决策依据：用户说明“只有我自己在用”，随后明确“忽略历史问题，就我自己在用，一次性不留债务的直接改成新的。不需要考虑兼容性问题”。原方案中为旧用户保留身份、存储或接口的建议据此撤销。

成功结果：从安装、首次设置、日常使用到 Agent 接入均使用一致的 `loppi`。切换后使用全新数据目录和配置，重新完成必要授权、Provider 配置及 Agent 接入；不承诺原 hear 历史自动出现在新应用中。旧文件不主动删除。

## 2. 一次性交付范围

| 范围 | 应交付的结果 |
| --- | --- |
| 产品与交互 | 应用名、窗口、权限用途说明、首次设置、兔子入口、辅助访问文本、错误提示、设置和导出默认文件名统一。 |
| 源码与工程 | npm/Cargo 名称、crate 路径、Rust 引用、原生桥接符号、内部事件、项目环境变量、资源引用和锁文件同步。 |
| macOS 发行 | GUI、CLI、Core、worker、登录项 helper、应用包和 DMG 的布局、定位、签名及校验一致。 |
| 系统与本地状态 | 新 Bundle ID、新登录项、新数据目录与数据库、新 GUI 偏好位置及新 Keychain service；按首次安装配置。 |
| Agent 与外部接口 | CLI、MCP、随包 Skill、配置生成、自检、安装说明全部切换到新命名。 |
| 文档与验证工具 | 更新当前使用入口、架构和开发说明，以及实际运行的测试、打包、诊断、资源采样和报告脚本。 |

不包括 hear 的旧命令/环境变量别名、旧协议及归档格式适配、旧数据/偏好/凭据自动迁移、双目录识别或双写、hear 历史版本恢复与降级支持；也不包括功能扩展、Windows/Linux 适配、新的同步或网络服务、模型升级、重做兔子形象、重写历史证据、完整 ASR 选型重测。仓库托管名称、域名、组织账号、商标和开源许可证是外部事项；本文只记录配套入口，不执行这些操作。

## 3. 命名规则与已收敛决策

### 3.1 统一映射

以下是已交付的命名；所有当前入口一次切换，不保留 hear 运行时回退。第三方内容和历史证据按第 6 节处理。

| 对象 | 当前 | 目标或处理规则 |
| --- | --- | --- |
| 展示名、npm 包名 | `hear` / `Hear` | `loppi`；句首、标题也保持小写。 |
| 应用与发行文件 | `hear.app`、`hear_<version>_<platform>.dmg` | `loppi.app`、`loppi_<version>_<platform>.dmg`。 |
| CLI / Core / worker 可执行文件 | `hear`、`hear-core`、`hear-worker` | `loppi`、`loppi-core`、`loppi-worker`。 |
| 四个 crate 目录与包名 | `hear-{protocol,core,cli,worker}` | `loppi-{protocol,core,cli,worker}`；Rust 引用同步为 `loppi_*`。 |
| 桌面包与库 | `hear-desktop`、`hear_desktop_lib` | `loppi-desktop`、`loppi_desktop_lib`。 |
| helper 及其构建源文件 | `HearCore.app`、`HearCore-Info.plist`、`HearCore.entitlements` | `LoppiCore.app` 及对应的 `LoppiCore-*` / `LoppiCore.entitlements`；权限能力不因改名扩大。 |
| 原生桥接、内部事件 | `hear_*`、`Hear*`、`hear-*` 事件 | 对应 `loppi_*`、`Loppi*`、`loppi-*`；声明、实现、发送、订阅及测试同步。 |
| 项目环境变量 | `HEAR_*` | `LOPPI_*`，不读取旧前缀；第三方变量沿用原名。 |
| MCP / Skill | server `hear`、工具 `hear_*`、`hear-context` | server `loppi`、工具 `loppi_*`、`loppi-context`；配置和 Skill 路径占位符同步。 |
| 对外结果版本字符串 | `hear.context.v1` | `loppi.context.v1`，不保留旧字符串；不借此改变字段语义。 |
| 默认日志与新导出 | `hear.jsonl`、`hear.01.jsonl` 等；`hear-export-0.1`、`.hear-export-*` | 对应 `loppi` 前缀；只支持新应用自身的日志轮转、归档与未完成导出恢复。 |
| 系统与存储身份 | Bundle ID、数据目录、数据库名、Keychain service | 按下表全部换新。 |

MCP 当前共有七项工具：`status`、`list_context`、`search_context`、`get_event`、`read_context`、`digest`、`list_people`，均按统一前缀更名。`client_tools` 清单核对、工具描述、schema、示例和 Skill 必须对应同一套实际工具。

### 3.2 四项决策

用户已选择全部换新、不考虑 hear 兼容；以下具体命名沿用现有 `app.<产品>.<组件>` 结构，作为当前方案。

| 编号 | 决策 | 接受的代价与排除项 |
| --- | --- | --- |
| D1 系统身份 | 主应用为 `app.loppi.desktop`，helper 为 `app.loppi.core`；包、登录项、音频排除规则一起切换。 | 按新应用完成系统授权与登录项启用；不继承或迁移旧权限，不伪装成旧签名身份。现有 Developer ID 证书不因品牌改名而强制更换。 |
| D2 持久化 | 默认业务目录 `~/Library/Application Support/loppi/`，数据库 `loppi.sqlite`；GUI 使用新 Bundle ID 对应配置目录，显式覆盖数据目录时存于 `LOPPI_DATA_DIR/gui/`。 | 新库、新偏好按首次使用初始化；不扫描、读取、迁移、合并或回退至 hear 目录。旧文件留在原处，本次不主动删除。 |
| D3 凭据 | Keychain service 为 `app.loppi.cloud`，凭据 account/provider 的业务结构沿用。 | 在新应用设置中重新配置 Provider；不读取、复制、重命名或删除 `app.hear.cloud` 下的旧条目。 |
| D4 外部接口 | 仅提供 `loppi` CLI、`loppi_*` MCP 工具、`loppi-context` Skill、`loppi.context.v1` 和 `LOPPI_*`。 | Agent 配置和个人脚本一次重配；不提供旧命令别名、旧工具名、旧结果版本或旧环境变量 fallback。 |

新旧目录的锁互不相通，因此正式安装切换时应先结束本机 hear 进程并停用旧登录项，再启用 loppi；这是当前唯一使用者的一次安装操作，不建设自动接管旧版或永久跨目录协调能力。

## 4. 仓库事实与职责入口

本表用于定位责任边界，不是逐文件施工顺序。执行时应重新搜索当前工作树，避免依赖已经移动的路径。

| 领域 | 主要入口 | 核实到的事实与修改责任 |
| --- | --- | --- |
| 工程与包元数据 | 根 `Cargo.toml`、`package.json` 及锁文件；四个 `crates/hear-*`；`src-tauri/Cargo.toml`、`src-tauri/src/main.rs` | workspace、包名、路径依赖、库引用和可执行文件互相关联；锁文件须由相应工具生成或校验。 |
| GUI 与原生界面 | `index.html`、`src/App.tsx`、`OnboardingView.tsx`、`SetupGroups.tsx`、`setup.ts`、`api.ts`、`cloud-task-status.ts`、`TimelineView.tsx`、`WorkspaceViews.tsx`、`AgentAccess.tsx`、`FloatingWidget.tsx`、`native-audio.ts`；`src-tauri/src/`、`src-tauri/native/` | 名称分散在显示文案、辅助访问标签、导出文件名、原生提示、Tauri 事件及权限说明中。 |
| 包布局与签名 | `src-tauri/tauri.conf.json`、`src-tauri/Info.plist`、`src-tauri/native/HearCore-Info.plist`；`scripts/build.mjs`、`scripts/package_macos.sh`、根 `build_release.sh`、`scripts/build_macos.sh` | helper、CLI 和四组件路径在配置、构建、签名、DMG 挂载验证中重复出现；构建时生成的覆盖配置也必须一致。 |
| 生命周期与目录 | `crates/hear-protocol/src/lib.rs::{data_dir,core_binary_path}`、`lifecycle.rs`；`crates/hear-core/src/main.rs::{run,acquire_owner_lock}` | 默认根目录为 `hear`，数据库为 `hear.sqlite`；`core.lock`、`lifecycle.lock`、`core.sock` 均依附该目录。已有优雅交接和构建身份检查。 |
| GUI 偏好 | `src-tauri/src/lib.rs::run` | `HEAR_DATA_DIR` 指定时位于其 `gui/`，否则使用 Tauri `app_config_dir()`，保存 `onboarding.json`、`entry-preferences.json`；Bundle ID 变化会影响默认配置位置。 |
| 登录项与系统声 | `src-tauri/native/agent.m`；`crates/hear-core/native/{audio_exclusions.h,system_audio.m,system_audio_tap.m}` | SMAppService 使用 `app.hear.core`；自播放排除依赖 GUI/Core 身份。旧登录项不会因改常量自动注销。 |
| 凭据 | `crates/hear-core/src/cloud.rs::{SERVICE,credential,check_credentials}`；`events.rs` 中凭据引用 | service 为 `app.hear.cloud`；数据库保存 provider 与 account 引用，实际秘密在 Keychain。只改 service 会使原凭据不可见。 |
| Agent 契约与安装 | `crates/hear-protocol/src/{context.rs,agent_assets.rs}`；`crates/hear-cli/src/{main.rs,mcp.rs,context_cli.rs}`；`resources/agent/hear-context/` | 协议库拥有共享查询契约和六份随包 Skill 文件；CLI/MCP 是适配层。Skill 安装拒绝覆盖不同内容的副本。 |
| 导出、日志与观测 | `crates/hear-core/src/{archive.rs,unilogs.rs}`；`scripts/{sample_macos_resources.py,report_macos_resources.py,measure_macos.sh}` | 旧导出恢复识别 `.hear-export-id` 和 `.hear-export-*.pending`；日志轮转及采样按名称匹配，遗漏会造成恢复或观测失效。 |
| 验证与 CI | `scripts/verify.py`、`verify_build_identity.py`、`test_build_identity.py`、`test_upgrade_lifecycle.py`、`smoke_agent_context.py` 及相关前端/原生测试；`.github/workflows/verify.yml` | 验证入口含旧包路径和测试隔离变量；CI 主要调用统一验证入口，无名称引用的文件不必为更名修改。 |
| 品牌资源与说明 | `assets/hear-icon-source.png`、`design/rabbit/`、`playground/`、`README.md`、`specs/`、`.agents/skills/README.md`、`.gitignore` | 活跃资源名、页面标题、下载名和链接一起更新；图标像素无需重画。原型、历史文档、证据需按第 6 节区分。 |

## 5. 产品行为、升级与架构契约

### 5.1 首次安装与正常运行

新安装包的外部入口使用 `loppi`。从随包路径定位 CLI、Core、worker 和模型，应用移动后通过现有集成入口重新生成配置，不硬编码 `/Applications/loppi.app` 为唯一可运行位置。模型继续只在 helper 资源内保存一份。

Core 继续是业务数据库、采集监督、媒体、队列与清理的唯一 owner；GUI/CLI 通过现有本机 IPC 工作。Agent Context 继续只查询已运行 Core，不因查询自动开麦、启动云任务或创建后台。MCP 继续使用 stdio，不新增端口、服务进程、内容缓存或第二份数据库。

### 5.2 全新初始化与正常生命周期

只解析新业务目录、新数据库和新 GUI 配置位置；`LOPPI_DATA_DIR` 显式覆盖时，Core、GUI 和测试子进程必须使用同一目录约定。不因发现 hear 数据而切换路径，也不为旧数据库创建迁移标记、备份队列或恢复分支。

新应用按既有首次设置与用户启用规则开始工作，旧版的历史记录、暂停状态、定时配置和偏好不自动带入。权限或配置尚未就绪时展示真实阻断原因；存储初始化失败时明确报错，不回退到旧目录。

正常运行仍复用 `lifecycle` 的同目录互斥、构建身份检查、优雅交接与完整退出。Core 继续正确封存尾块、释放子进程和 socket，保留对较新构建的降级保护。删除只用于 hear 历史版本的兼容分支与对应测试；不能借此删掉 loppi 自身更新和退出所需的生命周期能力。

### 5.3 系统身份、权限与凭据

D1 的新身份必须同步到主包、helper plist、SMAppService、helper 查找路径和系统声排除名单。真实签名包验证新登录项注册、启用状态和重启后的唯一后台；应用代码不负责发现或注销旧版登录项。

麦克风、屏幕录制/系统声权限按新应用实际状态引导，不承诺继承旧授权。系统声录制期间回放、GUI 重启和两种音频采集实现都必须排除 loppi 自身的播放声音，避免自录回路。

D3 只在 `app.loppi.cloud` 保存和读取新配置的秘密；新数据库保存对应引用。用新签名 Core 的 `check_credentials` 验证已配置凭据可读；秘密仅存 Keychain，不另存明文文件、命令参数、日志或证据。未配置或读取失败时明确显示状态，不读取旧 service，也不静默重复提交云任务。供应商 ID、接口和第三方环境变量不随品牌改名。

### 5.4 Agent 接入与外部接口

新生成的 Codex 配置、通用 MCP 配置、复制命令、Skill 和帮助文本使用实际 `loppi` 路径及同一工具清单。保留配置生成与显式安装的职责：不后台扫描并改写用户的 Agent 配置，不覆盖自定义 Skill，不覆盖无关命令。

D4 一次切换 CLI 命令、MCP server/工具名、结果版本、Skill 及路径占位符。旧配置和旧命令不继续工作，接入说明只提供新入口；个人 Agent 配置和脚本由使用者重新配置。

项目环境变量仅识别 `LOPPI_*`。测试、验证脚本及其子进程同步使用新隔离开关，正常验证不得访问真实数据或网络。禁止因旧 `HEAR_TEST_*` 已被忽略而让部分测试失去隔离。

内部 IPC 协议号、业务数据库 schema 和对外格式版本分别管理；不因改名机械递增或重置。更名不扩展查询语义，不保留 hear 专用协议适配。

### 5.5 导出、日志和恢复

新导出的默认文件名、manifest 格式名和内部 marker 统一为 loppi；新作业继续具备原有的完成、报错、清理和中断恢复能力。不接管 hear 的 pending 作业，不新增旧归档读取或导入功能。

日志写入、轮转与诊断读取全部使用新命名。资源采样、进程识别和报告覆盖更名后的 GUI、Core、worker、采集子进程与 WebView；不为 hear 历史日志和采样记录增加适配，新报告不得因漏掉新进程而虚报资源下降。

### 5.6 架构约束与允许的新增机制

复用现有 `lifecycle`、数据路径解析、Core 启动恢复、凭据模块及集成安装入口。GUI 不增加业务库写入权；不新增迁移服务、跨旧目录锁、旧名探测、双写、兼容队列或重试器。本次不增加长期持久状态、通信协议或职责主体；旧版本专用分支应随相关入口清理，不以隐藏开关保留。

## 6. 替换边界与文档更新

| 类别 | 处理规则 |
| --- | --- |
| 当前产品代码、活跃原型、随包 Skill、构建/验证脚本 | 更新实际品牌、路径和引用；跨语言符号及事件成对核对。相关测试验证行为，不能只修改断言让旧路径继续通过。 |
| 当前 README、架构、使用和开发指南 | 修正为新入口，保留文档职责；同步纠正相关的旧协议号、六工具等失实描述。README 中“代号 hear”等叙述应更新；说明全新配置和不迁移旧数据；开源状态只能反映已明确的事实。 |
| 历史 PRD、研究、发布记录及原始证据 | 不批量改写正文、旧命令、产物名、哈希或提交引用。入口文档说明旧称；若移动活跃源文件导致链接失效，维护链接或提供明确的新位置，不改变证据含义。本文自身保留旧名用于解释更名范围。 |
| 第三方文本和模型 | 不修改许可证正文、作者、供应商名称/接口、模型 ID、下载 URL、哈希和词表；词表中的 `HEAR`、`HEARD`、`HEART` 是模型 token，不是品牌残留。 |
| 生成目录与本机状态 | `target/`、`build/`、`dist/`、依赖缓存通过重建更新；不批量改写本机历史数据、报告、安装包或第三方凭据。公证配置示例名使用 `loppi-notary`；`APPLE_*` 等标准配置项沿用，现有签名证书和私钥不因改名重建。 |
| 工作流与无关改动 | `.agents/skills/` 中复用的 MyAgents 工作流无需借本次整体改写；`.claude/skills` 链接保持有效。已有无关草稿与工作树改动不纳入更名施工。 |
| 托管与传播入口 | [当前仓库](https://github.com/hAcKlyc/hear)的 README、clone、发行下载和 CI 链接须配合最终托管名称更新；实际远端仓库/账号/域名调整另按授权执行，不提前写入未生效地址。 |

搜索覆盖 `hear`、`Hear`、`HEAR` 以及带旧名的文件路径，以“当前产品和工具链只使用新命名、无旧名兼容分支、历史证据真实”为验收标准；历史记录、第三方原文和模型 token 不要求字符层面零命中。

## 7. 验收与发布完成定义

以下检查由实施者执行并记录，review 者核对证据与覆盖边界；跳过、交叉编译或模拟结果不能写成真机通过。任一必需项失败，都不能宣布更名完成或发布。

| 验收场景 | 通过标准 |
| --- | --- |
| 全仓静态与回归 | `python3 scripts/verify.py` 对应的静态、Rust、前端、Python、原生与烟测门槛通过；路径、包名、FFI 链接、Tauri 事件、Skill 内嵌资源均有效。残留按第 6 节分类，无无意保留的当前入口。 |
| 新安装完整路径 | 首次设置、授权、录制/暂停、时间线、回放、截图、导出、设置和兔子入口正常；窗口及系统中的产品展示名称统一，不依赖开发目录或旧构建缓存。 |
| 新存储与配置 | 默认目录、数据库和 GUI 偏好均使用新位置；显式 `LOPPI_DATA_DIR` 时所有进程一致。首次初始化、重复启动和初始化失败行为正确；不读取或修改旧目录，不建立迁移状态。 |
| 进程与登录恢复 | loppi 自身的录制、暂停、关闭窗口、完整退出、重复启动与登录项恢复正常；同一数据目录只有一个 Core owner。不存在为 hear 保留的进程接管或跨目录协调分支。 |
| 权限与自播放排除 | 真实签名包验证新应用的授权入口；ScreenCaptureKit 与 Core Audio Tap 路径均验证新身份匹配、自播放排除及 GUI 重启后的行为。无对应机器证据时明确未通过，不以编译代替。 |
| 云凭据 | 在新 service 配置后，新签名 Core 的只读检查显示可读，无网络请求、无秘密输出；未配置、保存失败及拒绝访问均准确呈现，不查询旧 service。 |
| CLI / MCP / Skill | `loppi` 帮助与集成自检正确；MCP 初始化、七工具发现、清单核对、查询结果和 Skill 指令一致；Core 离线时保持 existing-only。旧名不再作为支持入口，测试隔离完整采用新变量。 |
| 归档、日志与观测 | 新归档、pending 作业恢复、新日志读取与轮转正常；采样报告识别实际更名进程。没有新增旧格式适配。 |
| 发行包 | 四组件版本、源码与构建身份一致；既有 ARM、Intel、Universal2 构建范围和最低 macOS 不缩水。模型只打包一份且哈希不变；离线准备、移动安装位置、签名、公证、装订、Gatekeeper、DMG 挂载内容及最终校验和全部通过。原生运行结论只覆盖实际测试的平台。 |
| 文档与切换说明 | 当前文档和可复制入口有效，清楚说明新应用需重新授权、配置 Provider 和 Agent；旧数据不自动迁移。无未经决定的开源许可证、发布状态或远端地址。 |

发布完成要求：实施范围全部交付，以上必需项有可复核证据，阻塞问题关闭，所有当前入口统一且没有 hear 专用兼容分支。生成已验证发行包与对外上传/安装是不同动作；执行外部发布和替换用户安装版时仍遵守当时授权。

## 8. 本次文档审查结论

已按用户“只有自己使用、全部换新、不考虑兼容”的补充要求重审全文，撤销旧版身份保留、数据与凭据迁移、旧接口别名、旧导出恢复等原有责任；验收同步改为全新初始化和新应用完整能力。正常生命周期、唯一 Core owner、隐私许可、凭据保护和测试隔离继续保留，这些是 loppi 自身正确运行的条件。

用户已接受本文并要求实施，原讨论台账的决策与原话已完整吸收，执行进展统一记录如下。

## 执行台账

### 开发契约
- 用户授权：「OK，就按这份 PRD 开始开发吧，把整个仓库完整更名为 loppi，不保留旧兼容逻辑，做完验证和 review。」
- 必赢场景：源码、GUI、四组件、系统身份、存储、凭据和 Agent 入口统一为 loppi，使用新库与新配置；七项只读 Agent 工具正常工作。
- 原子交付：单一开发批次，全部更名、文档、验证与独立 review 一起完成；没有 hear 外部兼容责任。
- 必须保证：新命名贯通构建、签名和运行定位；Core 唯一 owner、真实权限与自播放排除、完整退出、测试隔离、模型哈希及第三方文本完整。
- 明确不做：旧名别名、旧身份/数据/凭据/格式迁移或回退；旧资料删除、替换用户安装版、远端仓库更名、对外上传与许可证选择。
- 状态来源：数据路径由协议库解析，业务状态由 Core 拥有，GUI 偏好按新配置目录保存，秘密由新 Keychain service 保存。
- 复用：`data_dir`、`core_binary_path`、`lifecycle::{ensure_current,quit_gui_core}`、`agent_assets`、`context`、现有 build/verify 入口。
- 新增架构机制：无。删除 hear 历史协议 17–19 的生命周期适配，保留新产品自身的稳定交接路径。

### 验收与结束条件
- 执行统一静态/回归/烟测、Agent 与生命周期专项、Universal2 四组件构建和包身份检查；相关修改后只重跑受影响的检查。
- 签名、公证、真实 GUI/权限/登录项与两种系统声路径是第 7 节的发布硬门槛，缺少证据时如实标为未完成，不降格为建议。
- requirements / adversarial 独立审查、修复及定向验证后，再进行最终 architecture 独立审查。
- 仅全部必做证据闭合且 review PASS 才提交完整交付；外部操作不包含在本次授权内。

### 开发批次与行动清单
- [x] Batch 1：机械更名与活动资源路径更新；清理历史专用兼容；同步当前文档。
- [x] 全仓静态、回归、IPC / Agent / 生命周期与包构建验证。
- [x] 签名候选与真实系统能力验收：权限、双路采集/暂停、两种系统声音自播放排除、GUI 重启、截图/导出、凭据、登录项及完整退出。
- [x] 干净源码最终发行包、四组件源码身份与公证/挂载校验。
- [x] 三视角独立 review、修复、受影响复验与证据闭合复核。
- [x] 最终源码身份、校验和及交付状态收尾记录。

### 当前批次 Review 基线
- Batch base：`f2f38b0402a1f613d9ad3d578615bff6f35911ae`。
- 分支：`codex/rename-loppi`，工作目录沿用当前仓库。
- 范围：当前产品、工具链、资源和活跃文档及本 PRD；历史 specs/research/evidence 保留原事实。
- 初始工作区：仅本需求 PRD 和讨论台账未跟踪，无其他代码改动。

### 待用户决策
无。

### 进展日志
- 2026-10-31：用户接受全新 loppi 身份、无兼容/迁移方案，readiness 通过后进入单批次开发；原讨论台账已吸收。
- 2026-10-31：完成产品、四 crate、helper、存储/凭据、Agent、原生桥接、资源、工具链和当前文档统一；保留历史事实、第三方字节和真实远端地址，删除历史协议 17–19 适配。
- 2026-10-31：统一验证最终组合态 21 checks 全部通过，420 Rust passed、21 既有 opt-in ignored；Agent 七工具、正常生命周期、Universal2/模型/移动路径专项通过。
- 2026-10-31：requirements/adversarial 发现 ZIP 内部旧名和文档协议号失实，修复后原 reviewer 定向复核 PASS。真实退出暴露基线 query 锁/主线程窗口调用死锁，在 UI 发布前释放锁；原 adversarial/architecture 复核 PASS，重建签名包回放/刷新重叠时 native Cmd-Q 通过。最终三视角和全部证据闭合复核 PASS，无新增架构机制。
- 2026-10-31：用户授权并完成本人 Touch ID；新签名包权限、双路采集/暂停、Tap/SCK 正对照和 GUI 重启自播放排除、时间线/回放/截图/新格式导出、签名 Core 凭据正向读取及真实登录项注册/服务恢复/完整退出通过。没有重启整台电脑，Intel/macOS 13 仅完成交叉编译及部署目标检查。
- 2026-10-31：实现提交 `5e74a47fee6b9b746c0253e4b9df16ce85d2636c`。从独立干净工作树生成正式 0.1.14 Universal2；四组件源码相同、dirty=false，公证 Accepted `b38a8c4d-c847-4590-89b1-a3a7adde3896`，装订/Gatekeeper/挂载内容/模型烟测通过。完整元数据和校验和见[验证与审查记录](../research/2026-10-31-loppi-rename-verification.md)。
- 2026-10-31：最终 DMG 已保存至 `build/releases/0.1.14/macos-universal2/`，SHA256 `e1c762094b8e72babcfdf34da16886d4cc986782a6a302fd6ab6c0fa4d29f5e9`。PRD 原子交付门禁全部通过，状态 implemented；收尾提交只记录状态和证据，不改变已验证发行源码。
- 2026-10-31：按约定关闭并注销新登录项，验收 GUI/Core 全部退出；录制与云队列保持暂停，实际媒体留本机。没有替换旧安装、迁移/删除旧资料或凭据、改现有 Agent 配置、push 或公开发布。另一会话未跟踪设计草稿 `specs/prd/_drafts/loppi-design-identity.md` 保留且未提交。发行产物已复制回原目录；应用保护的发行工作树保留，详情见验证记录。
