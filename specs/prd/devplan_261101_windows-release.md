---
type: devplan
status: in-progress
created: 2026-11-01
updated: 2026-11-01
scope: "完成 loppi Windows 支持、真实 Windows 安装验收，并将同版本产物发布至 v0.1.18。"
---

# Windows 完整支持与 0.1.18 发行

## 用户授权与目标

用户准备休息，明确授权完成所有必要工作、解决 Windows 问题、实际打包并发布到既有 v0.1.18 Release，并明确授权通过其 MyAgents 网络联络 Windows 上负责 loppi 的 Agent。本任务持续执行到原子验收；不将编译通过冒充可运行。源基线为 main `1e36cf56c5c49b9cc4ba9d9305131e7e83782ca9`，现有 v0.1.18 标签/两个 macOS 包构建源码为 `4969899afd8d2e9d4dc632d568ef33a843cc56bc`。

## 执行台账

### 开发契约

- 必赢场景：在用户 Windows 11 Pro x64 设备上，从实际安装程序完成安装、启动、首屏/设置、麦克风和电脑声音持续采集、离线模型准备/转写、时间线/事件/现场笔记截图、回放、托盘/悬浮入口、CLI/Agent接口、暂停恢复/正常退出与升级，再产出有校验值和真实身份的 Windows 安装包。现有 macOS 行为与原 ICON/设计系统保持。
- 原子交付：Windows 本机与 Mac 组合态验证、独立三视角审查、干净源码版本0.1.18构建、Git源码同步、最终 v0.1.18 Release 的标签/构建信息/产物一致。用户本次授权同版本增加Windows；若必须更新现有标签，先保存原标签/附件校验事实，全部新产物验证后才移动到最终源码；不把不同源码二进制伪称同一构建。旧版下载在最终新产物就绪前保持可用。
- 必须保证：完整GUI/CLI/Core/worker和离线模型、所需DLL/许可随安装包；本机单owner/受限IPC、持久数据/幂等/时间坐标、真实采集状态/缺口、监督与取消、凭据安全存储、有限资源和正常退出不退化。支持基线为本机可真实验收的Windows11 x64，不声称已验证Windows ARM64或更早Windows版本；最低系统要求必须在安装/运行和文档一致。
- 状态来源：Core继续是唯一业务/SQLite/录制/任务owner；GUI保留既有路由元数据。Windows仅提供原生事实/平台操作，不新增业务数据库、独立配置owner或网络管理接口。协议与模型权重尽量不变；不为测试绕过既有用户意图/权限/资源规则。
- 复用：现有Request/Response、lifecycle/owner锁/监督、source_process与record媒体日志、CPAL麦克风、worker帧协议、model_pack与models.lock、cloud provider与凭据引用、资源Governor、schedule、Controls/SetupGroups/Onboarding、EntryRuntime/播放owner、现有build/package验证入口。
- 已授权平台机制：必要的cfg平台边界；Windows受限本机IPC/对端身份、进程与IO/原子文件操作、WASAPI系统声（排除本产品回放）、Windows安全凭据、截图/OCR/电源资源/日历/播放/托盘/悬浮/登录启动；Windows包脚本、目标与原生库锁、必要的平台依赖与安装器。它们都归现有生命周期owner，不新增业务双路径、临时迁移flag、未授权云服务或协议代理。
- 明确不做：Linux/移动端、跨设备音频同步、旧hear数据迁移、新产品IA或品牌重画、变更仓库可见性/开源许可证、购买签名证书、关闭安全软件/绕过真实审批、上传历史录音/真实笔记/密钥。没有Windows代码签名身份时必须如实标注，不伪称Authenticode验证。
- 安全：外部MyAgents只走用户指定公开CLI契约；令牌仅注入调用进程环境，绝不进入仓库、委托prompt、日志或发布物。跨设备Agent/Session ID来自成功发现/接纳响应；状态与文本主动读取，不将接纳/idle当成功，不因结果不确定自动重发。

### 分工与共享工作区

- 主协作者（Mac）：契约/协调、前端平台文案与真实样板、Mac回归、跨平台集成与独立审查、源码/版本/标签与Release最终一致性。
- Windows loppi Agent：只读审计先行，随后以明确文件所有权承担Windows原生/后端/工具链/打包与实机验收；不得覆盖已有改动。代码经独立分支协作，主协作者整合后最终构建必须使用同一源码提交。正式发布动作由主协作者统一授权，避免同时替换Release。
- 发现设备：WINDOWS-TEST-HOST，Windows11 Pro x64 Build26300，工作区 `C:/Users/USER\Documents\project\loppi`；初始main干净，与Mac基线一致。公开Agent selector见本机 `/tmp/loppi-windows-session-start.json`；正式证据只记录必要非敏感身份。
- Mac 当前开发分支：`codex/windows-support`；初始工作树干净。所有修改均须明确归属，禁止同时写同一文件。

### 验收与结束条件

- mandatory：两个OS适用静态/单测/前端构建；共享IPC/owner与子进程失败/超时/取消/退出、原子文件与凭据边界；Windows真实双路采集/本地转写/回放排除/截图OCR/暂停恢复；真实安装与覆盖升级/卸载行为（保留用户资料）、托盘/悬浮/启动入口、模型只读/Unicode路径/缺失损坏；源与安装4组件身份、架构/依赖/资源哈希与最终安装包验证。
- mandatory：支持窗口与Windows平台文案/快捷键/标题栏适配，原ICON指纹；Mac回归不污染用户当前录制/资料/偏好；三视角审查PASS后提交；最终干净源码的Windows安装包（及若更新同版源码则对应Mac包）发布到既有v0.1.18，上传后远端SHA/大小/说明核对。
- advisory：其它Windows设备/最低版本以外矩阵、ARM64 Windows、真实付费云API与未获专门授权的密钥传输、跨夜/多天资源观测。相关事实诚实记录，不将其伪称已验证。
- 影响对应：平台/共享Rust变更重跑受影响的Mac+Windows检查；UI变更重跑相关浏览器/窗口与真实平台；打包变更重做实际包；发布元数据不机械重建未变代码。实现、审查与发布证据均写磁盘，全部mandatory关闭才implemented。

### 行动清单

- [x] 双端审计、平台边界与文件分工确认。
- [x] Windows原生/共享Rust/GUI与CLI源码实现及确认修复（正式包/真机门槛独立待验）。
- [x] Windows打包、资源/身份/依赖与真实默认/Unicode安装、录制中升级和卸载保留资料。
- [x] Mac/前端组合回归与平台规范/文档更新。
- [x] 独立需求/故障与最终架构审查、修复、源码提交。
- [x] 同一干净30a源码的三端正式构建、Mac签名公证和各自产物身份/资源核对。
- [x] 解锁后的真实桌面截图/OCR/HWND排除恢复、物理OS入口验收及确认缺陷修复。
- [ ] 包含追加视觉/标题栏/实测修复的新统一源码三端正式包、身份与实际安装复核。
- [ ] main同步、v0.1.18标签/三端附件与远端摘要核对、全部强制验收闭环。

### 当前批次 Review 基线

- Base：`1e36cf56c5c49b9cc4ba9d9305131e7e83782ca9`。
- 预期范围：协议/核心/CLI/worker/Tauri平台边界、Windows原生模块与平台依赖、前端必要平台呈现、构建/验证/文档。具体scope在审查前按实际差异冻结。

### 进展

- 2026-11-01：已读用户指定external-myagents-cli公开契约，安全注入环境并成功发现/接纳Windows loppi审计Session；未传播访问令牌。远端Codex权限模式no-restrictions，工具链Node24.13/npm11.6/Rust1.96MSVC。初审无既有完整Windows后端，进一步审计进行中。

- Windows权威Session selector（只作公开CLI寻址，无凭据）：`ma-session:1:98b5376a-4750-49e6-ae82-c5de39b7afef:c5e7b546-0aaa-5908-99be-44432bf52711:5e6eceaa-2f55-45dd-a074-433c5857e364:Njk3OGE1M2EtZTA2YS00NGNlLWIyNDAtMWM2YmE2NzdlNTgx`。
- 2026-11-01：Windows审计完成，Win11 x64设备/音频端点/SDK/WebView2/NSIS与GitHub权限可用，无签名证书、无既有Windows实现；受限磁盘14.8GiB已纳入构建约束。已授权remote codex/windows-native独占Rust/Tauri/config/后端及构建脚本，root独占共享前端、设计导出与主文档。WindowsCore/worker已静态编译，链接/采集实测仍待完成。root完成Windows权限/状态/凭据文案、托盘名、系统字体及平台标题栏间距；9项定向/除待刷新设计资源外全部Node、双平台18场景呈现分支通过。全部本机原生与最终发布门槛仍未完成。

- Windows存储边界已明确：Core大体积设备资料为`%LOCALAPPDATA%\app.loppi.core`，安装程序为`%LOCALAPPDATA%\loppi`，GUI路由配置归Tauri应用标识目录；不以测试旧路径引入迁移/fallback，默认升级/卸载保留Core资料。

- 2026-11-01 04:12 CST：远端报告Windows全工作区Rust测试通过（Core392通过/21既有忽略），真实双源10秒采集无丢包、X-ASR合成英文语音识别、原生GUI播放及进程回放排除、合成Credential Manager写读删已完成；截图/OCR与安装验收继续，最终报告/源码尚未同步，仍不据口头进展关闭发布门槛。旧v0.1.18标签OID与两个Mac包/报告已备份到ignored build/backups/v0.1.18-before-windows，Release immutable=false；现有下载保持。

- 2026-11-01 04:17 CST：Windows Agent 发现WTS控制台锁定、前台窗口0、Win32及Pillow桌面截图黑屏；WebView内部截屏正常但不作为桌面验收。已向用户异步提示方便时解锁，Agent继续其它实现/安装/审查，并验证锁屏明确失败、合成PNG OCR及截图子进程监督。真实桌面截图/窗口排除维持mandatory pending，绝不绕过锁屏或据此宣告完成。

- 2026-11-01 04:25 CST：原生privacy契约明确Windows独立`system_audio`事实，截图状态不能控制WASAPI准备；root共用派生helper并补锁屏/缺失音频事实、三类截图错误提示。84项Node回归（导出5项仍待ICO合并）通过。Windows报告新增19项平台边界测试通过（对端PID/命名空间/限额超时取消/DACL/Unicode/文件锁）。root已通过公开Session文本向Windows交付无凭据前端精确补丁，授权仅apply，不转移所有权或混入原生候选提交。

- 2026-11-01：经远端三路审查的原生候选`34bfec2f93e42ca80c8671bcb2c2a8a313b5ae71`已ff集成，87文件与root零重叠。生产源码所有权已交接root，Windows继续安装测试不并行改同文件。Mac首次检查因更名旧绝对路径缓存失败；仅清理tauri/tauri-plugin-dialog构建缓存后，`verify.py --suite all`整体验证退出0，含静态/全工作区单测、89项Node、Python、原生策略、四组件身份及隔离Core/诊断/测试owner。原生产Mac应用未重启或读写其资料。完整正式资源及ICO导出5项检查通过。
- 集成自审收敛Windows构建入口：补包版本/组件role/epoch核对，安装器只选择本次版本，避免旧NSIS残留进入报告；PowerShell转发参数并预检Node。它们属于本次包身份/脚本契约，最终Windows构建与负向入口检查仍待执行。

- 2026-11-01：组合两路review确认Windows ICO误用旧Hear标记、v4托盘遗漏键盘上下文消息，两项均已根治并获两位定向代码PASS。Windows实际18项几何/消息与GUI编译通过；物理任务栏操作仍待解锁。ICO原生跨CPU48px浮点采样不同，改用固定整数像素中心采样与批准RGBA黄金指纹；所有PNG/ICNS原字节不改，正式包只用Git中的ICO。
- PowerShell预检的多Node数组/启动异常误报成功已通过实机发现并修复：选第一应用、原生exit状态、显式catch非零；两个入口的参数拒绝、缺Node、无效exe均返回1，警告后exit0仍为0。Node90项及Mac宿主的Win32呈现模拟90项均通过；后者不替代Windows真实门禁。项目`.gitattributes`统一源码LF，不放宽格式规则、不修改用户全局配置。
- Windows开发候选默认安装、实际默认资料根粗转、录制中覆盖安装及默认卸载保留资料通过；Unicode模型/安装/升级通过。一次卸载慢退出现场已自然结束，随后有日志复现正常退出，原原因未证实，不能冒称已归因。另独立复现GUI早启动时安装准备返回成功但GUI仍活跃，属新的发布阻塞。
- 当前临时独占lease：Windows Agent仅修`crates/loppi-protocol/src/platform/win.rs`的安装退出函数，root期间不写该文件；使用已有进程身份与正常退出消息，修完提供小补丁/实测，禁止自行提交。其它生产源仍由root集成。
- Tauri CLI2.12官方匹配模板的Hook后仍用Restart Manager强制关闭文件持有者；root在NSIS hook中覆盖为只读占用验证，失败非零中止，silent错误框设默认；真实NSIS编译/负向保活测试待Windows返回。该修复关闭本次“不强杀”的真实源风险，不增加业务owner。

- 2026-11-01 06:30 CST：Windows单文件退出lease已完成交回，root核对9229字节补丁SHA a3b68497…f31并应用。协议19项及真实GUI两次早启动/一次入口就绪退出均通过（1.24/0.91/1.49秒，GUI退出0，Core/worker结束）；合成无入口15.36秒/最终静止5.10秒均非零且保活，跨安装目录隔离通过。该修复只复用既有进程身份/正常退出消息，不强杀。
- 整数采样ICO七尺寸RGBA与完整ICO SHA478b1c5d…53603跨CPU完全一致，原PNG/ICNS指纹不变，实际GUI/rc构建通过。两个PS包装脚本的全部正负向已通过。NSIS真实小安装器验证两处只读RmGetList、空路径/关闭后放行、占用时非零且进程存活，无错误框挂起、无Shutdown/Kill；完整正式安装器仍须重建。原NSIS Hook退出失败仍保留现场调查，不混称GUI竞态已关闭该问题。

- 2026-11-01：最终需求/故障定向复核及fresh architecture全部源码PASS，135文件冻结哈希一致，三者均独立核对批准ICO无像素差异。root本轮fmt/diff/typecheck/lint通过。Windows紧凑原始报告11853字节SHA bea14082…814已公开CLI回传、解压核对并落盘；十项旧Hook对照全部0，原dirty失败仍如实记录“未复现/原因未知”，不无限扩张机制，最终新包重复原序列验收。单文件lease已交回，root现全量源码owner。
- 本次提交仅是经三路审查的内部干净构建checkpoint，非原子交付完成。接下来用同一SHA实际构建三端包；全部mandatory关前不implemented、不改main/tag或Release附件。原Mac签名包/原标签备份保留。


### 最终离线门禁的测试工具修复

- 事实：73f53d4干净Windows已通过typecheck/lint/Rust check及全部Rust/Node/rabbit；Python discovery暴露`restore_debug_session.py`、`test_upgrade_lifecycle.py`顶层导入fcntl失败，以及Mac采样合成路径被Windows Path改写导致归属断言失败。
- 裁决：required evidence gap，属于已承诺两端开发门禁；不是产品采集/存储代码缺陷。最小修复仅将Mac操作专用fcntl导入归到执行该操作的函数，把Mac虚构路径明确写PurePosixPath；不跳过原有纯函数断言、不放宽验证规则、不移植Mac诊断工具到Windows。
- 修复范围：scripts/restore_debug_session.py、scripts/test_upgrade_lifecycle.py、scripts/test_resource_sampler.py及本台账/审查记录；base73f53d4cd4988875e00e05e271041de01b86e5e8。现有业务/原生检查不失效；定向重跑两端全部Python发现及模块导入。两路定向复核与原final architecture定向复核后提交新checkpoint，正式三平台包统一使用新SHA。
- 73f53d4的Mac两薄包均已签名、公证、装订、Gatekeeper和身份/模型验证通过，作为中间构建证据保留；不将它们混入最终不同源码的三端Release。因统一来源契约，新测试源码提交后重新构建全部正式包。

- 三处工具修复经需求/故障/架构定向复核全部PASS，AST确认原Mac操作和13项样例断言保持；Mac9/9通过，真实Windows9项运行中1项原有Mac native adapter跳过，其余8项通过，无新增skip。报告已落盘；新内部checkpoint仅改变开发测试工具与事实记录，不改变产品源码，正式包继续按统一新SHA重建。


### 最终安装器产品图标补齐

- 0fa6e0e真实完整Windows发行构建退出0，四组件一致、dirty=false、epoch1793487528；388630959字节，SHA87533908…0bfc。19PE闭包、GUI七帧图标和完整NSIS只读检查通过，实际安装验收继续。Tauri会恢复上次测试安装目录，fixture不能假定返回默认目录；缺失/损坏模型start应非零，拒绝是正确产品行为。
- 实际资源检查发现安装器外层仍是默认NSIS图标。裁决为本次产品ICON发行资源的required gap；Tauri CLI2.12本地权威schema明确`NsisConfig.installerIcon/uninstallerIcon`。仅在tauri.windows.conf.json将两项明确指向现有批准icons/icon.ico，不改变ICON像素、业务逻辑或安装生命周期。
- 修复范围仅该Windows配置及本台账/审查记录，base0fa6e0e377ef64d86db6b44199044b367733507a。定向核对schema、路径/源ICO指纹与安装/卸载两种资源；两位及架构定向复核后新checkpoint，三端包仍统一新SHA。0fa的功能性音频/IPC/转写等证据继续适用；源未变不机械重跑，最终新安装器须实际安装/升级/卸载与资源核对。

- 图标两项配置获需求/故障/原final架构定向PASS。真实NSIS3.11小型编译和写出卸载器均0，两个EXE的RT_GROUP_ICON/RT_ICON七尺寸PNG逐帧与批准ICO一致；无注册表写入、未改真实安装。源码路径/schema/兼容性门禁已关，正式新包资源与安装身份待重建验证。
- 0fa模型负向实测：缺失model_bundle_missing、损坏model_checksum；start均退出2并提示model_not_ready，prepare-install退出0/forced=false，原模型校验值不变。失败的先前fixture期望不再混称产品缺陷。

### 最终包与当前外部阻塞

- 二进制源码固定为`30a04cc7879ee69f191b53c744da7483cd48c0e0`，已推送`codex/windows-support`。两份Mac薄包均签名、公证、装订、Gatekeeper通过；root独立核对8组件身份、批准ICNS和DMG校验。Mac旧应用当前录音未被重启。
- Windows实际发行入口0，四组件同source/epoch1793489277/dirty=false，最终包388800577字节、SHA ef3b8f66…49d4c533、unsigned。真实默认安装/52资源/19PE/GUI+安装+卸载器七帧图标、双路采集/持久粗转/CLI、录制中覆盖→新转写→录制中卸载及资料保留通过；最后干净默认重装。GUI退出0；Core已停并释锁，单独退出码未捕获，不补写0。
- 18份完整脱敏原始JSON通过公开CLI回传，56654字节SHA0941141d…966f0d，root已验传输/源身份/图标PNG字节/模型锁/前后新采集及出口。完整verify属于0fa的10项、426Rust/89Node/8Python通过及原有忽略/跳过；30a仅安装图标配置变化，不改贴执行版本。最终包与证据详见[发行准备状态](../research/evidence/2026-11-01-windows/release-readiness.md)。
- Windows默认应用已保留，合成Core数据归档、凭据回收、startup关闭，无测试产品进程。用户/工作区前缀脱敏；未上传真实用户内容、密钥或进行付费云调用，未绕过锁屏/安全策略/被拒清理。
- 当前仅真实锁屏阻挡三类mandatory本机验收；已向用户异步提示解锁，Windows Agent完成所有可独立事项并待命。该外部条件不引入产品/架构岔路，台账保持in-progress。旧Release及标签/main保持；发布授权本轮已明确，无需再次审批，解锁验收通过后继续合并与发布。
- 本收尾记录仅承载事实，不更改30a产品源码；此后的文档提交号不是包内source，已验证包不为文档提交重建。
- 2026-10-31 23:57:38 UTC最后只读复核仍锁定（Session1/WTS flags0/Default）；远端main仍1e36cf5、v0.1.18标签OID仍b093afb，旧5附件摘要与备份一致。全部可独立工作已完成，当前等用户解锁；状态和证据以本次文档提交留存，不宣告完整交付。

### 用户追加：时间轨道满高呈现

- 用户已解锁Windows，原Agent继续30a原生截图/OCR/窗口排除与真实入口验收；已有整体发布授权不变。同时用户提供Mac首页截图，指出红色记录段只占轨道上部、要求修复精致度。两项合并到本次0.1.18完整交付，不取消Windows目标或先发布旧视觉包。
- Batch base：f1b1db136485c0ff2104025c6fa7361f1bc2b520，除本台账补充外原工作树干净。事实根因：唯一tokens将轨道定义24px、记录段定义18px，组件top0，必然留6px全时段灰底。
- 必须保证：红色录制段与绿色已留存/已整理段共用轨道高度与上下边界，取消这条无含义的全宽空带；只依真实已有区间填色，仍裁剪今天/未来，不涂满空声源或未来。实际缺口保持原局部纹理、覆盖顺序和提示；当前时刻线、标签、键盘焦点、点击定位及录音/内容事实保持。
- 最小实现归属：现有map-event-height引用map-track-height，轨道高度仍24px，不新增数值/owner/状态/协议/依赖或修改Core。更新核心DESIGN及派生token/规范/真实组件预览；原ICON、字标、色彩不变。
- 验收：真实App浏览器记录/历史/暂停/空/跨日、1120/760及减少动态；实际DOM量测所有记录段与轨道top/bottom/height一致、缺口仍可辨、时刻和键盘/定位契约保持。做必要静态/资产/相关回归与新小范围独立审查，不机械重跑未改原生/业务检查。
- 完成：新干净源码统一生成三端最终包，保留所有适用的30a原生验收；新UI与包身份单列验证，原Tag/Release/main在全部门槛完成前继续保持。当前Mac正式录音不用于测试，先在隔离预览验证修正。

### 解锁实测确认：Windows DPI截图裁切

- 已观察到实际GUI→Core→GDI成功持久化、OCR完成，外进程借用GUI HWND排除被正确拒绝，同GUI PID隐藏/恢复及像素排除通过。150%桌面物理2496×1664，但Core进程DPI awareness0导致GetSystemMetrics虚拟化，PNG仅1664×1109并裁掉右侧合成标题；GUI/helper awareness2。此为真实截图完整性blocking，不以OCR完成冒称全屏验收。
- 最小修复放在既有Core Windows OS桥的截图调用内：当前线程临时Per-Monitor-V2上下文，metrics/DC/BitBlt使用同一物理坐标，RAII在全部返回路径恢复原上下文。设置失败走已有失败返回，不改变进程全局DPI、系统缩放、manifest、HWND归属/锁屏/超时/恢复边界或业务owner。
- 临时唯一写入lease交Windows Agent：仅`crates/loppi-core/native/windows.cpp`中`loppi_capture_png`及直接所需小型作用域guard；root期间不写该文件。Agent无提交/推送/发布权，交回精确小补丁及受影响实测后root接回。其它源尤其前端仍由root独占。
- 受影响验证：实际DPI-unaware调用者在当前150%桌面的PNG物理尺寸与全屏四角/右侧合成内容、Core实际持久化OCR、同PID排除恢复及无关原入口测试；编译和线程DPI上下文恢复的正常/失败路径。Windows原生裁切修复与用户轨道修正一起进入最终小范围审查/新统一源码包；Mac业务未改不机械重跑旧业务全套。

### 用户追加：Windows 融合标题栏与入口实测修复

- 用户要求Windows标题栏融合界面，允许自定义标题栏。检查Tauri与现有HTML dialog后选择更小的原生同色方案：保留系统装饰，通过Windows11 DWM将主窗口caption背景/文字绑定现有color-canvas/color-text，取消突兀边框。系统继续负责拖动、双击、最小化/最大化/还原、关闭及Snap；关闭仍触发既有CloseRequested隐藏，后台录制和资料不变。Mac原生交通灯及布局不改，产品ICON与字标不改。不新增业务owner、GUI命令、权限或持久状态。
- 准备中、首次设置和详情沿用同一原生主窗口标题栏，不额外插入HTML控件、不覆盖正文和模态层。颜色从唯一tokens在构建时解析，错误token阻止构建，不维护第二份色值。当前高对比度模式保留系统样式；原生API错误显式返回启动错误。实际验证同色外观、拖动/双击/按钮/边缘缩放与关闭后台语义。该选择避免WebView顶层模态遮挡自绘窗口按钮。
- 解锁后实测还确认：WebView子窗口未触发父HWND拖动，Windows像素位置被共享Mac UUID/归一化校验拒绝；实际托盘Quit在菜单窗口残留时阻塞。均属原Windows入口mandatory，本批定向根因修复并重测，不能以旧静态几何检查代替物理输入。
- 临时扩大Windows Agent唯一写入lease：在既有Core截图文件之外，仅`src-tauri/native/windows.cpp`托盘菜单/悬浮拖动与位置、`src-tauri/src/entry.rs`平台位置校验、`src/FloatingWidget.tsx`Windows兔子拖动事件。root不同时写这四文件；Agent提供精确补丁及实测，不commit/push。root独占标题栏/配置/能力/样板/规范。复用EntryRuntime现有位置持久化与原生事件，不新增重试/业务协议/兼容配置。
- 验收：实际鼠标拖动保存且重启恢复、点击仍标记、菜单鼠标和键盘Open/Quit正常结束、--login真实展示；Windows原生融合标题栏在实际GUI测试拖动、双击最大化还原、最小化、关闭隐藏仍录制、重新打开、边缘缩放与首次设置可达。受影响静态/单测/前端构建、三路独立审查及同源三包完成后统一发布。
- 定向实测裁决：右侧展开菜单拖到另一侧时，保存锚点800/700正确，但释放后的可见中心仍704/700，重新悬停才回正。根因是OS移动循环中已折叠WebView，释放HWND仍保留抓取时的局部偏移。归现有原生拖动owner：计算释放锚点后，立即复用既有collapsed布局，再发送kind5结束/保存；不加状态或恢复轮询。补验双向拖动释放当帧位置、保存和重启一致；此前其它通过证据仍适用。

### 解锁后组合态源码验收

- 所有原生写入lease已交回，root统一持有生产源。精确6769-byte补丁SHA c7563733fb3f11ee2e91f232a36761986352a4dad67055f8f93214abbc6b70e5与后续root最小修正均已实际编译。根治DPI裁边、WebView拖动启动/坐标校验、菜单退出阻塞、拖动后悬停抑制未解除与松手视觉偏移；不增加业务owner。
- 双平台各18场景+减少动态、各8时刻、24px满高/局部缺口、底部点击/Enter、34项Node回归通过；Mac GUI12项、主题解析2项、静态/前端构建和Mac GUI编译通过。Windows实际GUI12项、主题2项、C++消息/几何及新增异步退出/悬停断言、React交互、静态均通过。
- Native真实150%物理2496×1664截图四角、失败/成功DPI恢复、Core持久OCR、GUI PID排除/隐藏恢复、外进程拒绝通过。实际托盘鼠标/键盘Open/Quit无需Escape；双向拖动立即显示中心与保存一致、单击仅一标记、移出重入恢复、--login新进程恢复通过。原生caption/画布像素同为250/248/242；真实拖动/双击最大化还原/最小化/边缘缩放/关闭隐藏仍双路录制、首次设置可达通过。
- 原始证据以Git证据专用提交8ca9966710a05eae8fd9ca71e0567cc0f55c3a6c精确传输，root逐字节核对图片和JSON。JSON工作区CRLF35534字节被Git规范为LF34268字节，两种原SHA均独立重建匹配；只差行尾，不改原始数据。七个生产文件哈希中六个完全一致，entry.rs仅rustfmt闭包换行，精确反向换行也匹配Native指纹。解释见unlocked-transport.json及unlocked-review.md。
- 保留真实例外：一轮键盘录制中Quit的worker exit1，GUI/Core/采集子进程0且全部子进程结束，未人工强杀；单个worker原因未捕获。既有OwnedChild取消允许kill/wait，契约不要求被取消模型子进程全0；独立评审裁决为非阻塞观察，不改原始pass:false、不捏造原因。Native说明kind7顺序有文字误写，实际源为先kind1离开再kind7解除，原始数据保留、解释纠正。
- 本轮fresh requirements/adversarial及fresh最终architecture、两处定向生命周期修正复核均PASS，原生证据关闭源码验收缺口。接下来提交内部干净checkpoint、同源三包、最终安装/身份与Release核对；全部完成前仍in-progress，main/tag/旧下载保持。

### 用户追加：首页收口与打开即恢复记录

- 9559源码三包均已构建；Mac已按用户新指令实际安装并重启，原ICON/设置/双源记录已核对保留。用户随即验收四处缺陷，要求继续修正：未整理绿色描边、空态文案、打开软件自动记录、短红段过度圆角。9559包转为历史候选，Windows已完成的新包验收仍保留原归属，不发布/移动main/tag。
- Batch base：9559b84a5566b8144769910458b6586bae5843b8。root为全部源唯一writer；Native无写入lease，待新SHA再做受影响运行验证和正式包。
- UI契约：已留存未整理改为平涂浅薄荷绿，无常驻描边/内阴影；与已整理保持同色系不同深浅，录制仍暖红。只复用现有色组，通过语义token表达未整理留存色；轨道/红绿段/图例使用统一2px小圆角、同高24px，不改时长/来源/缺口/点击与键盘语义。正常列表为空统一为“暂无有效事件”，加载/失败仍不伪称空态。新增真实未整理历史、短录制区间样板，不能只看命名事件与长区间。
- 自动记录契约：已配置用户显式打开GUI时，经现有AppOpened将query-only Core提升正常运行并恢复保存的记录意图，不应因未启用定时计划停在“尚未开始”。继续尊重enabled_intent/主动暂停、声源选择、定时窗口、授权/模型/资源边界；首次未选择开始的用户不被偷偷开麦。CLI/Agent查询仍不提升采集许可；关闭主窗保持后台，真正退出仍正常停止。修复放Core现有AppOpened分支，不新增GUI自动点击、配置、重试、协议或状态owner。
- 事实根因：AppOpened已有first-activation guard，但clear_quit/allow_capture仅放在recording_schedule.enabled分支；无定时的已启用用户永远停在query采集许可。最小修正把恢复许可放到定时结算之后，仍只做首次GUI激活；不重写用户意图或源选择。
- 验收：静态/受影响Node、Core回归与隔离真实IPC验证query不采集、GUI首次激活恢复、退出后重新打开恢复、主动暂停及源选择保留；使用禁用声源的隔离库验证许可，避免测试录制真实资料。新签名Mac包和Windows新包分别实际打开验证原已启用记录无需手动继续、暂停不被改；图中原始绿/短红/空态真实组件视觉核对。必要独立三路review、新干净源码统一三包、更新本地和原子发布结束；不机械重跑未受影响的DPI/图标/安装算法全套。
- 用户继续要求审视红色色阶和两类卡片的hover/页面背景关系。正常本区域常态3种暖红、含hover4种，Logo锈红独立。原白卡hover与页面同色、录制hover偏深且标签保留独立浅底，裁决调整现有语义角色：白卡hover浅暖灰#F4F0E8、录制hover柔和#FCEAE0、录制标签底透明；默认暖白/浅暖红和页面底色不变。新增两组实际hover前景/背景对比度检查，真实默认/hover并置验收；不增加新状态或交互机制。

- 2026-11-01：首页最终候选及用户追加 hover 关系已完成源码修正，需求/故障/最终架构和定向复核全部 PASS；最终 token/导出/manifest/6 项设计检查、实际 pointer hover 与对比度通过。Mac 隔离真实 IPC 的 owned-process 版本独立重跑通过。Windows 旧尝试尚有 SQLite I/O 失败，最新测试改为正常 Quit 并等待实际持有 Core 进程退出 0 后再写隔离库，未因此修改产品存储代码；最新脚本已投递，真实结果仍 pending。
- 当前外部条件：MyAgents App 运行、外部调用开启，但公开 status/version/get 均返回 MYAGENTS_PORT_REQUIRED；未探测内部端口/隐藏 API，也未把陈旧响应当新结果。已异步请用户正常重开 MyAgents。Windows 原区域设置最新激活验证仍为提交前 mandatory；新同源包、自动记录/暂停真实安装验收、本地更新及 main/tag/Release 收尾继续 pending。详见 ../research/evidence/2026-11-01-homepage-final/review.md；当前不宣告完成或发布。

- 用户随后明确要求快速构建并本地重开。已使用现有 build_dev 单 ARM 入口完成最新工作树本地预览（0.1.18/source9559+dirty=true/epoch1793511058），四组件身份/模型与原ICNS核对通过；应用以原同一Developer ID重签并完整校验。正常Cmd-Q、等待原应用全部退出后替换/Applications/loppi.app，旧程序留在ignored备份，数据/偏好未改。实际打开后无需手动继续即双声源recording，enabled/pause/source四项与更新前一致；主窗口已显示，截图及完整本地证明仅存/tmp，避免提交用户内容。本地预览未公证/未发布，不冒充最终clean同源三包，也不关闭Windows激活及正式发布门槛。


### 用户追加：重点竖线与标记后的整段染红

- 需求：notch 标记重点后不能让历史两轨整段变红；重点用与声音时间对齐的细竖线呈现，移除概览中独占的“现场记录”行，保留重点/笔记/截图真实时刻、详情和保存事实。延续本地快速预览更新；完整 Windows/正式发布门槛不变。
- 精确根因：新标记可先于最新媒体封块，events::timeline 将未被素材覆盖的 anchor 按相邻命名事件分组，然后错误吞并该命名空隙内全部未整理素材并 OR 两个声源；后续 mark_recording_items 把这个错误宽范围标为 live。新媒体覆盖 anchor 后下一次读取不再走该分支，颜色恢复。不是重点动作改变录音状态或新增红色。
- 最小修复 owner：Core 既有 timeline 投影仅在所有相邻素材间的真实空隙分组 moment_only，不吞并或扩大任何素材区间/声源；无 schema、持久状态、协议、重试或 owner 增加。liveCard 忽略 moment_only 以免尾部新重点压掉仍在录制的素材卡。UI 用现有 day-map overlay 和语义色画精确细线/小旗，沿用 onSelect(item,anchor) 与原详情，不增加新行或窗口。
- 保护范围：不改重点写入/幂等/录音意图/来源/封块、已保留原声、用户资料、命名事件和精转任务；录音红段与既有颜色不变。未覆盖点按真实时刻独立投影，后续媒体到达可归入原素材；无音频时不伪造音频轨道。
- 验收：合成双源历史+当前尾部+新重点真实 Core 回归，新增点前后原所有媒体范围/来源及 live 来源完全相同，封块到达后正常吸收；静态/设计导出/相关 Node，真实 React 标记前后及跨日/两端/键盘点击/不遮挡轨道；三视角独立定向 review 后快速 ARM 本地构建、签名/身份核对、正常替换重开。不用真实用户库测试写入，安装后核对用户原标记仍在。
- 本小范围候选的前置快照保存在 /tmp/loppi-moment-before；其它未提交首页/自动启动修正保留，未并行分配源写入。禁止把旧安装预览、旧三包或 Windows 未完成证据冒称本修复已验收。

- 用户在该批继续明确色彩语义：事件轴上的进行中色块与对应第一条卡片代表同一内容，应共用相同柔和浅暖红；深锈红用于此刻指示（右上录制状态与当前时间指针）。因此追加最小映射调整：两处默认/hover都用现有color-recording-surface及hover token；卡片状态文字/点退为中性，轴侧进行中节点也退中性；右上状态点与文字共用深红。无需新增色值、状态或协议，覆盖此前本批“红段颜色不变”的限制；其它录制/数据保证保持。

- 重点投影修复/两轨细线/统一进行中色全部经需求、故障及fresh最终架构PASS，Core149通过/9既有忽略、20Node及6资产、静态、真实browser边界/普通点击/键盘/实际default-hover色核对通过。已按持续本地预览授权完成新ARM包（epoch1793513398，dirty=true），使用同一Developer ID签名、原ICNS与四组件身份核对；正常退出替换重开，无手动继续，原双源recording/四项意图一致。原3条moment全部保留，实际点击两条13:53重点按秒定位正确，概览无第三行；主窗口已返回。正式clean同源三包/Windows原区域设置激活/main/tag/Release尚未完成，不能以本地预览冒称发布。

- 用户继续验收浅暖红过浅：轴段与卡片须一起稍加深，仍温和且完全同色；现有hover同步加深。此次仅调整同一色组的共用表面token/导出与实际表面文字对比度检查，不改录音、重点、布局或交互。状态浅底保持既有口径，新增同色组实体填充基础色承接较深事件表面，避免改变其它状态文字背景。前置快照/tmp/loppi-warm-fill-before；验收为真实默认/hover两处计算色相同、现有文字对比度与设计导出/类型/格式检查，再快速ARM本地预览正常替换重开，保留原标记和录制设置。

- 加深共享暖红完成：默认#F8DCCB、hover#F5D4C1，实际两处default/hover一致，右上/当前指针及状态浅底保持。中性文字对比度4.94/4.63，类型/格式和6资产通过，原需求及最终架构定向PASS。ARM当前工作树预览epoch1793514411已构建、原签名核对并正常替换重开，四组件/原ICON一致，双源recording自动恢复、意图与3条moment保留，主窗口已展示。当前仅更新本地预览；正式Windows/三包/main/tag/Release门槛仍开放。

- 用户认为加深后的整卡暖红仍不舒服，明确要求用内置生图重新探索配色。已生成同底色/中性卡片两种对照及中性方向收敛；用户随后明确去掉卡片左侧色线，卡片保持中性暖白，仅暖红「正在记录」tag，轴段保留同暖红色系。最新提案与完整提示词保存在research/2026-11-01-home-color-exploration，标为视觉提案；本轮未修改正式Token/组件或替换本机，图中坐标与像素不作为实现规范。具体颜色仍处视觉确认，不把生成图直接当成已定稿设计系统。


### 用户确认落地：中性卡片与正在记录标签

- 用户已要求将最终生图方向实现并重新打包、本地查看；不是继续探索。当前以/tmp/loppi-neutral-tag-before为精确前置快照，root唯一生产writer。
- 必须保证：当前/已完成条目共用暖白表面及中性hover，无整卡红底、左侧装饰色线或色环；仅现有「正在记录」标签使用暖红浅底/深红字与点。轴上进行中区间采用同色系可辨的柔和陶红；右上实际录制状态及当前指针保持深暖红。中性页面/辅助灰阶减黄，绿条适度去饱和，清楚区分保存/录制/故障。沿既有palette/semantic层统一实施，原ICON/字标轮廓和深红i方点、兔子像素保持。
- 本次仅颜色角色/现有CSS与设计系统/导出更新，无新状态、owner、协议、依赖、布局或交互。不改Core投影/启动/录音/原重点与数据；旧未提交改动保留。用户明确采纳新配色关系，旧六色“固定”值随本次批准更新，品牌炭黑/字标点不变；不可通过改小号字对比度阈值绕过检查。
- 验收：类型/格式、6项设计资产与实际文字表面对比；真实React默认与hover检查（当前与完成卡同底、无竖线/全卡染色，tag及轴段各自正确、时刻线/重点仍可达），宽/窄与Mac/Windows呈现；必要独立三视角；单ARM本地预览构建四组件/原ICON/原签名核对，正常退出旧版、等待进程结束替换并重开，原意图与重点保留、自动录制，主窗口可见。正式Windows/clean三包/main/tag/Release门槛仍开放，不冒称本地预览是正式发布。

- 中性卡片/暖红标签已落地：既有卡片白底/中性hover共用，删除录制卡特有染色/定位隔离样式；当前仅tag使用红浅底/深红文字点，轴段陶红，页面/灰阶减黄与绿条去饱和，原资产不变。27定向Node/静态/6资产、真实两类卡default/hover/no-stripe/tag、两平台各18场景+reduced、重点/光标检查PASS。独立发现并修正深色入口hover文字对比4.23到4.96，在既有dark-entry层增加一档同色系亮色、保留4.5阈值并新增三表面对照；真实FloatingWidget重点与启用保存actualhover通过，样例所有动作拒绝，无Core写入。原初包弃用，修正源重新构建。
- fresh需求/故障及P2修复定向、fresh最终架构均PASS。已实际安装重启修正ARM工作树预览epoch1793517718（0.1.18、dirty=true），原签名/四组件身份/原ICON通过；正常退出等待旧进程后替换重开，双源自动recording、原四项意图和3条moment保留，主窗口可见新版。用户截图/私有证明在/tmp。仅本地预览完成，不等同于正式Windows/clean三包/main/tag/Release交付；台账保持in-progress。
