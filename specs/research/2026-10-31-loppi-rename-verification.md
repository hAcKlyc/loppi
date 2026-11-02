# loppi 整仓更名：验证与审查记录

日期：2026-10-31。需求：[更名 PRD](../prd/prd_0.1.14_loppi-rename.md)。分支 `codex/rename-loppi`，基线 `f2f38b0402a1f613d9ad3d578615bff6f35911ae`。

## 结论与范围

整仓更名、三视角独立审查、真实系统验收及干净源码最终发行包全部通过，PRD 已完成。新产品只使用 loppi 身份、目录、凭据、工具、变量及导出格式，不保留 hear 的兼容、迁移或回退。移除历史生命周期协议 17–19 适配，保留同目录唯一 Core、正常构建交接、降级保护和完整退出。

仓库远端仍为 `hAcKlyc/hear`，现有 GitHub 链接保留真实地址。历史记录仅修复移动源码的链接，保留原事实。模型词表、下载清单、哈希和第三方许可未改变。本机项目目录名称未变。

## 自动化验证

| 项目 | 结果与覆盖 |
| --- | --- |
| 统一验证 | `python3 scripts/verify.py` 全部 21 个检查通过，覆盖类型、lint、Rust check、420 项 Rust 测试、前端/原生/Python 回归、四组件 debug 身份、隔离 Core/诊断/测试 owner 烟测。21 项既有 opt-in 测试涉及私有数据、真实环境或付费请求，未运行且不计为通过。退出修复后静态与 11 项桌面测试通过；最终全套复验 `/tmp/loppi-final-verify.json` 再次全部通过（420 passed、21 opt-in ignored）。 |
| Agent 接入 | `python3 scripts/smoke_agent_context.py` 通过七工具发现、清单核对、CLI/MCP 结果一致、existing-only、并发读取和安装保护；查询不改变采集状态。 |
| 生命周期 | `python3 scripts/test_upgrade_lifecycle.py` 通过当前构建并发初始化、完整退出/无后台不重启、30 秒有界忙锁，以及 capture/normal/query 交接和拒绝覆盖较新构建。 |
| Universal2 | 退出修复后重新构建四组件；ARM/Intel slices、最低 OS、构建身份和随包模型通过。当前 ARM 宿主的离线准备、重试、移动至 Unicode 路径烟测通过。 |
| 活动资源归档 | ZIP 内部全部更名：81 个成员，72 个二进制媒体与原仓库资产一致，16 个内部预览引用有效；自下载链接指向交付 ZIP。SVG 与嵌入媒体经 reviewer 对照，无图像内容变化。 |
| 第三方完整性 | 模型/native manifests、词表及许可证与基线逐字节一致；HTML 的 base64 媒体未变。Cargo/npm 锁文件只有本产品包名及相关排序变化。 |

## 签名候选与真实系统验收

本轮在当前 ARM Mac、macOS 26 验证；没有重启或注销整台电脑。候选位于 `build/verification/loppi-rename/loppi.app`，版本 0.1.14、协议 24、四组件身份一致。退出修复后的候选公证 Accepted：`d2c46b00-0c5e-46e9-aebb-303fa44bb61a`；签名、公证装订、Gatekeeper、只读挂载、内容比对和挂载包模型烟测通过。候选由工作树构建，`dirty=true`，不当作干净源码发行包。

| 场景 | 实际结果 |
| --- | --- |
| 首次设置与界面 | 显示 loppi / 准备 loppi；新库首次为空、随包模型就绪，设置、时间线、兔子入口和 Agent 页面正常。配置中的实际 CLI 路径、七工具、`loppi-context` 及导出默认名 `loppi-2026-10-31` 一致。隐藏主窗口保留兔子入口。 |
| 新系统权限 | 用户明确允许并完成本人 Touch ID；系统设置的麦克风、完整“录屏与系统录音”和“仅系统录音”均出现 loppi 且开启。完整录屏列表最初缺少应用，补入的是本轮签名候选，无需再次更名或修改旧授权。 |
| 麦克风与双路 Start/Pause | 麦克风 3 秒收到 144384 帧、48 kHz、零丢包；Tap 20 秒实际录制 960512 帧、5 块、零丢包。双路 Start 后 recording=true，Pause 后均 false，暂停意图保存。 |
| Tap 自播放排除 | 外部合成语音正对照 peak=0.630897；实际 GUI 回放期间 193024 帧 peak=0、零丢包。重启 GUI 后重复实际播放，仍为 193024 帧 peak=0。 |
| ScreenCaptureKit | 独立真实录制 962880 帧、5 块、零丢包；保留音频解码 peak=0.614328。持续 SCK 源中回放、终止并重开 GUI、再次回放：24 块共 5447040 帧全部为数字静音，未自录。测试后恢复 Tap 标记及麦克风开启偏好，录制保持暂停。 |
| 截图与导出 | 真实截图保存成功，导出含 1 个音频 WAV、1 张 2940×1912 PNG；marker 为 `.loppi-export-id`，manifest 格式 `loppi-export-0.1`，归档大小 1567744 字节。截图内容仅留本机，报告不复制画面/OCR。 |
| 新凭据 | 经正式 CLI 配置入口在新 service 配置已有 Provider，最小合成文本/音频连通成功。重建后的签名 Core `--check-credentials` 显示 llm/asr 均 readable、ok=true，检查无网络请求/无交互/无秘密输出。先前临时 CLI 造条目的 -25293 属 fixture ACL，临时条目已删除；实际配置路径通过。 |
| 登录项 | 实际启用 `app.loppi.core`，由 launchd 启动唯一 Core；初始交接有两次重试，随后稳定，UI 正常。保留注册时 native Cmd-Q 关闭 GUI/Core 并移除 socket，服务不自动重启；再启动已注册服务，新 Core 保留暂停且两路不录制。没有用完整 OS 重启冒充证据。测试后 UI 关闭登录项，launchctl 确认服务已注销。 |
| 完整退出 | 修复并重建后，在实际 GUI 回放和周期刷新期间 native Cmd-Q：GUI/Core/source/worker 全部退出、socket 消失。登录项开启与关闭两种状态均通过。最终验收应用已退出，没有新登录服务或后台残留。 |

真实采集期间云端两项队列保持暂停，实际屏幕和录音未送往云端；最小 Provider 连通只使用固定合成内容。新业务数据、偏好及凭据留在新命名空间，便于后续使用。没有复制、迁移或删除旧资料/旧凭据，没有替换 `/Applications/hear.app`、修改现有 Agent 配置或上传 GitHub 发行。

## 独立 review 与修复

- Requirements：活动 ZIP 内部旧名、技术文档当前协议号失实，两项接受并修复，原 reviewer 定向复核 PASS。
- Adversarial：独立确认同一 ZIP 问题；修复后 PASS；检查生命周期、身份/排除规则、定位、凭据及测试隔离。
- Architecture：最终更名版本 PASS，无新增 store、owner、队列、协议、迁移或兼容机制。
- 真实验收发现基线已有的退出死锁：`EntryRuntime::refresh` 持 IPC query 锁调用 `window.is_visible()`，等待主线程；主线程 Exit 的 `stop()` 又等待该锁。它违反本 PRD 完整退出保证，因此纳入本次最小修复：完成 IPC/snapshot 后、发布 UI 前释放锁并检查 stopped。既有 sequence 检查处理并发发布。没有增加状态、线程或恢复机制。
- 原 adversarial 与 architecture reviewer 对退出修复定向审查均 PASS；两路进一步检查重建签名包的退出、登录服务恢复和清理证据，确认原缺口已关闭、最终架构 PASS。requirements 进一步核对全部权限/采集/回放/凭据/登录/导出证据及最终全套复验，确认实现和签名候选验收 PASS。三路 required lens 均完成，无代码或真机阻塞；仅剩提交后的干净源码发行记录。

## 证据重建与边界

自动验证命令见上表；签名发行入口为 `node scripts/build.mjs release --target universal-apple-darwin`，不得用脏源码候选冒充最终发行。临时结构化记录位于 `/tmp/loppi-final-verify.json`、`/tmp/loppi-{native-export,sck-exclusion,exit-fix-tap-restart,exit-fix-credentials,exit-fix-native-quit,login-native-quit,login-recovery,login-disabled,acceptance-final-state}.json`。这些临时文件不是长期接口，本报告保留必要数值与结论。最终发行元数据和四组件身份文件与 DMG 一起保存在发行目录。

Intel 和 macOS 13 验证范围是交叉编译、slices 与部署目标检查，未声称在这些机器上完成真机采集。外部仓库/域名/许可证、安装替换和公开发布均不属于本次执行。

## 发行收尾

实现提交：`5e74a47fee6b9b746c0253e4b9df16ce85d2636c`。从该提交的独立干净工作树执行正式 release；未移动、提交或遮蔽另一会话的未跟踪设计草稿。全部发行文件已复制回原仓库。构建工作树 `<USER_HOME>/.codex/worktrees/loppi-release/hear` 由应用视为主检出，归档操作因此未执行；保留该工作树，不强制移除受保护检出。此限制不影响源码、产物或验收结果。

| 最终产物事实 | 值 |
| --- | --- |
| 版本 / 目标 | 0.1.14 / Universal2（arm64 + x86_64），最低 macOS 13 |
| 四组件 | GUI、CLI、Core、worker 均为上述源码提交，`dirty=false`，协议 24，构建 epoch `1793447023` |
| DMG | `build/releases/0.1.14/macos-universal2/loppi_0.1.14_macos-universal2.dmg`，202150536 字节 |
| SHA256 | `e1c762094b8e72babcfdf34da16886d4cc986782a6a302fd6ab6c0fa4d29f5e9` |
| Apple 公证 | Accepted，`b38a8c4d-c847-4590-89b1-a3a7adde3896` |
| 最终检查 | Developer ID 签名、装订/票据、DMG 与 app Gatekeeper、挂载逐文件比对、单份模型哈希及挂载包离线模型准备全部通过 |
| 凭据复验 | 干净源码签名 Core 两项均 readable，无网络请求或交互 |
| 元数据 | 同目录 `release-info.json`、`component-identities.json`、`SHA256SUMS` |

收尾提交只更新 PRD 完成状态及本报告；它不改变发行源码或二进制。新登录项已关闭/注销，验收 GUI/Core 已退出；录制、云处理保持暂停。没有安装替换、Git push 或外部公开发布动作。
