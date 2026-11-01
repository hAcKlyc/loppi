# Windows 本机只读审计

我先做只读审计，确认仓库现状、Windows 环境和已有实现；本轮不会编辑、提交、推送或发布。先返回基础事实，再补充平台缺口与建议分工。

初步事实已确认：本机是 Windows 11 Pro x64（Build 26300），工作区为 `C:\Users\hackl\Documents\project\loppi`。`main` 工作树干净，HEAD 与 Mac 提供的 `1e36cf56…` 完全一致，origin 已指向 `git@github.com:hAcKlyc/loppi.git`；本地只有 main，已知远端分支为 main/dev。Node 24.13.0、npm 11.6.2 和 Rust 1.96.0 MSVC 已安装。

代码中已有可复用的 CPAL 麦克风模块，但核心 IPC、进程监管、权限和多处 GUI 原生调用仍直接依赖 Unix/macOS，尚未发现完整 Windows 后端。我继续核实工具链、设备、缓存及具体文件边界。

工具链基本齐全：已找到 VS 2022/2026 的 MSVC、Windows SDK 10.0.26100.0、WebView2，以及 Tauri 缓存中的 NSIS 3.11；GitHub 仓库权限包含 push/admin。现有 v0.1.18 的 5 个 macOS 附件均在，标签解引用后也是预期的 `4969899…`。

主要工程缺口已定位到平台适配层，而非转录与业务逻辑重写。本机未见 loppi/hear 进程，音频驱动状态正常；实际设备端点和交互桌面还在核实。另一个构建约束是 C 盘目前仅约 14.8 GiB 可用，项目还没有依赖、模型或构建产物缓存。

只读审计完成。**本轮未编辑代码、安装依赖、提交、推送或修改 Release；工作树仍干净。**

1. **设备与仓库**

   - Surface Laptop 3，Windows 11 Pro x64，版本 `10.0.26300`。
   - 工作区：`C:\Users\hackl\Documents\project\loppi`。
   - 当前分支 `main`，HEAD：`1e36cf56c5c49b9cc4ba9d9305131e7e83782ca9`，与 Mac 一致。
   - origin：`git@github.com:hAcKlyc/loppi.git`；本地仅 main，远端 main/dev。没有未提交工作、开放 PR，未发现既有完整 Windows 实现或 Windows 专用分支。
   - `v0.1.18` 是附注标签，标签对象为 `b093afb…`，**解引用提交为预期的 `4969899afd8d2e9d4dc632d568ef33a843cc56bc`**。
   - Release 已核实有两份 DMG、两份 release-info 和一份 SHA256SUMS，共 5 个附件，均未触碰。

2. **已有实现与 Windows 缺口**

   | 范围 | 审计结论 |
   |---|---|
   | 麦克风 | 已有 CPAL 0.18.2，可复用采集、队列及错误处理。但启动权限函数、禁止空闲休眠函数仍是 macOS 原生调用。 |
   | 音频时间轴 | `record.rs` 使用 `CLOCK_UPTIME_RAW`；Windows CPAL 时间戳使用 QPC。必须统一时钟域，不能只让代码编译通过。 |
   | 系统声音 | 当前只有 ScreenCaptureKit/Core Audio Tap。需 Windows WASAPI 后端，并保留应用自身回放排除能力。 |
   | IPC | protocol/core 直接使用 Unix socket，并依赖 `getpeereid`、`LOCAL_PEERPID`。Windows 需带访问控制和对端身份验证的本机传输。 |
   | 文件与进程 | `fs2` 锁可复用；但 Unix 权限、`O_NOFOLLOW`、非阻塞 fd、父进程检查、kqueue、登录会话识别、可执行路径均需适配。 |
   | 凭据 | `cloud.rs` 直接使用 macOS Keychain，且 `security-framework` 是无条件依赖。需平台凭据接口，Windows 后端建议 Credential Manager。 |
   | 截图/OCR | 现为 macOS 原生实现。Windows 需截图、入口窗口排除、调用方窗口归属验证及 OCR 后端。现有 `u32` 窗口标识也需明确 Windows 表示契约。 |
   | 播放 | 前端已有完整播放命令与事件契约；后端直接调用 Objective-C 播放器，需 Windows 原生渲染实现并保留 epoch/取消语义。 |
   | 托盘/悬浮入口 | Tauri 窗口与兔子前端可复用；定位、焦点、窗口层级等大量调用 `ns_window()` 和 AppKit，需要 Windows 适配。 |
   | 开机启动 | 当前是 macOS LoginItem/ServiceManagement。Windows 尚无实现，需保留用户开关、显式退出及按需查询不开麦语义。 |
   | 资源与电源 | 资源准入策略可复用；内存、进程占用、电源、睡眠恢复数据来自原生 macOS 函数。录制日程的时区计算也依赖 Foundation。 |
   | 离线模型 | 模型清单、哈希校验、worker 协议可复用；模型路径已有 `resources/models` 后备布局。构建配置目前只支持 Mac 原生库。 |
   | 安装包 | 尚无 Windows 打包流程、ICO、Windows 资源布局或安装验收。现有 `scripts/build.mjs` 明确拒绝非 Darwin，CI 也只有 macOS。 |

   数据库、事件处理、ASR/LLM 队列、人物、保留清理、绝大多数 React 界面应直接复用，不需要重做。

3. **工具链、权限及缓存**

   - Node `24.13.0`，npm `11.6.2`。
   - Rust/Cargo `1.96.0`，已安装 `x86_64-pc-windows-msvc`。
   - VS 2022 Community/Build Tools、VS 2026 Build Tools 均在；找到 x64 MSVC 编译器和链接器，实际读取版本成功。
   - Windows SDK `10.0.26100.0`，包含 signtool；CMake `4.4.3`、Ninja、Python `3.14.2` 可用。
   - WebView2 Runtime 已安装。
   - Tauri 缓存中已有 **NSIS 3.11**；未在 PATH、常规目录或 Tauri 缓存发现 WiX。
   - GitHub 已登录 hAcKlyc，SSH 读取正常，仓库权限包含 push/admin；未验证写操作。
   - 用户和系统个人证书库中**未发现 Code Signing EKU 证书**，当前不能承诺 Windows 签名发布。
   - 当前项目没有 `node_modules`、`target`、`build`；常规位置未发现 sherpa/模型缓存。Cargo 已缓存 CPAL、Tauri 和多个 windows crate，但未见 sherpa-onnx。
   - 上游 sherpa-onnx **2.0.0 已提供 Windows x64 发布库**，可固定 SHA256 后复用，无须升级模型引擎。
   - C 盘剩余约 **14.8 GiB**，后续构建需控制中间产物规模。

4. **GUI、设备与测试条件**

   - 当前可打开交互桌面 `Default`，单屏，当前 API 返回尺寸 `1664×1109`。
   - 可通过本机 Python/Win32 操作隔离测试窗口、截图及自动化；当前工具列表没有专用桌面控制工具。
   - 实际枚举到一个活动 Realtek 麦克风阵列和一个活动扬声器端点，驱动状态正常；麦克风隐私总开关为 Allow。
   - 没有 loppi/hear/core/worker 进程，也未发现当前用户麦克风使用登记中的活动标记。**这不能证明其他软件绝对没有录制。**
   - 已读取到本机 OCR 语言 `zh-Hans-CN`。
   - 本轮没有录音、播放、截取桌面内容或安装测试；没有读取历史录音、笔记或凭据值。

5. **需要纳入统一契约的两个细节**

   - **自身回放排除**：建议采用进程树排除的系统声音采集。微软示例支持排除指定进程及子进程，要求 Build 20348 或以上；建议本次明确交付 **Windows 11 x64**，避免将尚未验证的 Windows 10 兼容性写入承诺。[微软官方示例](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/)
   - **OCR 与安装形式**：虽然本机能枚举 Windows OCR 语言，但微软文档明确将桌面 OCR 的受支持范围限定为具有包身份的应用。因此，**NSIS/MSI 安装后直接调用 WinRT OCR 不能仅凭本机偶然可运行就算交付完成**；需选择包身份方案，或随包提供独立离线 OCR。[微软 OCR 文档](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr?view=winrt-26100)

   IPC 建议使用限制到当前用户/登录会话的命名管道，并验证对端进程。默认命名管道 ACL 并不等同于现有 Unix socket 的安全边界，需显式配置。[微软管道访问控制文档](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)

6. **建议文件分工**

   - **Windows 本机负责**：Rust 运行时平台适配、Windows 原生模块、相关 Cargo manifests/Cargo.lock、Windows Tauri 配置、安装构建脚本和本机验收证据。重点涉及 protocol、core 的启动/采集/进程/资源/凭据模块，以及 Tauri 的 entry/playback/onboarding 后端。
   - **Mac 主协作者负责**：统一产品契约、平台文案与共享前端、README/架构文档、现有 Mac 构建脚本及 Mac 回归、审查和 Release 协调。
   - `src-tauri/src/lib.rs`、`entry.rs`、`config/build-targets.json` 属于高冲突文件，建议每个文件明确单一编辑者；我可持有前两者，Windows 构建使用独立入口，尽量减少 Mac 构建改动。
   - 验收应覆盖隔离数据下的双路录制、时间轴、回放排除、截图/OCR、离线推理、退出与重启、安装卸载。完成这些之前不把“编译成功”等同于 Windows 可用。

当前仅完成审计，下一轮可按主协作者确定的契约和文件所有权进入实现。
