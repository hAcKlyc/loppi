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
- 发现设备：DESKTOP-H5LPK87，Windows11 Pro x64 Build26300，工作区 `C:\Users\hackl\Documents\project\loppi`；初始main干净，与Mac基线一致。公开Agent selector见本机 `/tmp/loppi-windows-session-start.json`；正式证据只记录必要非敏感身份。
- Mac 当前开发分支：`codex/windows-support`；初始工作树干净。所有修改均须明确归属，禁止同时写同一文件。

### 验收与结束条件

- mandatory：两个OS适用静态/单测/前端构建；共享IPC/owner与子进程失败/超时/取消/退出、原子文件与凭据边界；Windows真实双路采集/本地转写/回放排除/截图OCR/暂停恢复；真实安装与覆盖升级/卸载行为（保留用户资料）、托盘/悬浮/启动入口、模型只读/Unicode路径/缺失损坏；源与安装4组件身份、架构/依赖/资源哈希与最终安装包验证。
- mandatory：支持窗口与Windows平台文案/快捷键/标题栏适配，原ICON指纹；Mac回归不污染用户当前录制/资料/偏好；三视角审查PASS后提交；最终干净源码的Windows安装包（及若更新同版源码则对应Mac包）发布到既有v0.1.18，上传后远端SHA/大小/说明核对。
- advisory：其它Windows设备/最低版本以外矩阵、ARM64 Windows、真实付费云API与未获专门授权的密钥传输、跨夜/多天资源观测。相关事实诚实记录，不将其伪称已验证。
- 影响对应：平台/共享Rust变更重跑受影响的Mac+Windows检查；UI变更重跑相关浏览器/窗口与真实平台；打包变更重做实际包；发布元数据不机械重建未变代码。实现、审查与发布证据均写磁盘，全部mandatory关闭才implemented。

### 行动清单

- [x] 双端审计、平台边界与文件分工确认。
- [x] Windows原生/共享Rust/GUI与CLI源码实现及确认修复（正式包/真机门槛独立待验）。
- [ ] Windows打包、资源/身份/依赖与真实安装验收。
- [x] Mac/前端组合回归与平台规范/文档更新。
- [ ] 独立需求/故障与最终架构审查、修复、源码提交。
- [ ] 最终干净构建、标签/远端/Release附件一致、全部强制验收闭环。

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
