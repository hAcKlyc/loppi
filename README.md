# hear

持续情境记录器。持续留下声音，通过初步识别、片段筛选与会话归组，形成包含音频、转写、截图、重点和评论的情境记录。

## 当前状态

0.1 macOS 客户端正在开发验收。签名版已跑通几分钟真实声音的录制、粗转、事件、精转、人物、纠错、笔记、导出和清理预览；覆盖升级与原有数据读取已有初验。签名 DMG 已通过 Apple 公证、票据装订和 Gatekeeper 包验证，可用于测试分发。多小时稳定性、资源测量及全新 Mac 安装验收仍待完成，当前构建不能当作正式发行版。

| 文档 | 职责 |
|---|---|
| [PRD](specs/prd/prd_0.1.md) | 定位、用户路径、产品行为、数据语义、范围与待确认决策 |
| [整体架构](specs/ARCHITECTURE.md) | 工作树中的进程、模块与数据边界；区分实现与未通过的验收 |
| [核心算法](specs/tech_docs/README.md) | 声音处理工作流、事件拟合与全局声纹匹配 |
| [本地ASR评测与选型](specs/tech_docs/local-asr-benchmark.md) | 当前X-ASR具体权重、20组测试集、五模型效果/成本与升级依据 |
| [技术 RFC](specs/prd/tech_rfc_0.1.md) | 进程与职责、采集和处理流程、数据生命周期、策略频率、性能与验证 |
| [设计系统](specs/DESIGN.md) | 视觉原则、组件规则与语义样式 token |
| [开发计划](plan.md) | 批次、验收证据与未完成的发布门槛 |
| [方案自审](specs/prd/REVIEW.md) | 本次审查发现、已做修正、仍需决策或实测的事项 |
| [界面交互 Playground](playground/hear-0.1.html) | 基于 0.1 PRD 的单文件 HTML 交互试验稿；直接在浏览器中打开 |

## 设计主线

**持续采集 → 语音检测与本地粗转 → LLM 事件拟合 → 需要时整段精转 → 时间线找回。**

保留下来的人声音频与已有粗转文字默认持续保存，直到用户主动清理或达到全局空间边界而暂停录音。用户主动截图、Mark 和评论关联到发生时刻。产品自身完成记录、整理、找回和回放，并在 macOS 首版提供完整命令行能力。

`hear` 是项目代号，尚未作商标或域名可用性确认。0.1 以 macOS `.dmg` 直接分发，不通过 Mac App Store。源码现阶段保持闭源；未来是否开源及采用 Apache-2.0、AGPL-3.0 或其他许可证尚未决定。复用 MyAgents 代码和第三方模型前，须分别核查相应代码权利与许可；当前仓库不复制第三方实现、模型权重或真实录音资料。

## macOS 发行构建

在已安装 Rust、Node.js/npm、Xcode Command Line Tools，并有 Developer ID Application 证书的 Mac 上，从仓库根目录执行：

```sh
npm ci # 首次构建或依赖更新后执行
./build_release.sh
```

默认生成 **Universal2**：GUI、CLI、Core 和 worker 都同时包含 ARM 与 Intel 代码；X-ASR、VAD 和声纹模型只在 Core 的资源目录保留一份，约 **204.64 MB（195.16 MiB）**。安装后自动检查和加载本地组件，无需额外下载。模型在构建时按固定清单获取并校验，运行时只读，不复制到用户数据目录。

安装两个 Rust target：`rustup target add aarch64-apple-darwin x86_64-apple-darwin`。发行须从干净且已提交的源码构建，依次完成编译、资源与启动检查、签名、DMG、公证和装订。产物在 `build/releases/<版本>/macos-universal2/`，包含 DMG、`release-info.json` 和 `SHA256SUMS`。通过 `APPLE_SIGNING_IDENTITY` 指定证书，未指定时选择本机 Developer ID Application 证书。

仅构建内部候选或指定架构：

```sh
node scripts/build.mjs build --target universal-apple-darwin
node scripts/build.mjs build --target aarch64-apple-darwin
node scripts/build.mjs build --target x86_64-apple-darwin
./build_release.sh --target universal-apple-darwin
```

App 位于 `target/<target>/release/bundle/macos/hear.app`。单架构候选在异架构宿主上只做编译和结构检查，不声称通过原生启动验证。脚本不会更新 `/Applications/hear.app`。完整布局、升级与验证边界见[离线资源与 Mac 打包说明](specs/tech_docs/offline-macos-packaging.md)。

对外分发前需要完成 Apple 公证。`build_release.sh` 自动读取根目录 `.env` 中的以下配置（已有环境变量优先），不会把 `.env` 或私钥文件放进安装包：

```dotenv
APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
APPLE_API_KEY=YOUR_KEY_ID
APPLE_API_ISSUER=YOUR_ISSUER_ID
APPLE_API_KEY_PATH=/absolute/path/to/AuthKey_YOUR_KEY_ID.p8
```

`APPLE_API_KEY` 是 App Store Connect API Key 的 ID，私钥放在 `APPLE_API_KEY_PATH` 指定的文件中。团队 API Key 需要 Issuer ID，个人 API Key 不应填写 Issuer ID。私钥路径支持绝对路径、相对仓库根目录的路径和 `~/`。配置后直接运行 `./build_release.sh`，即可依次完成构建、DMG 打包、公证、装订票据和 Gatekeeper 验证。

也支持钥匙串凭据：首次在本机终端运行 `xcrun notarytool store-credentials hear-notary`，按提示输入开发者 Apple ID、App 专用密码与 Team ID，然后执行：

```sh
APPLE_NOTARY_PROFILE=hear-notary ./build_release.sh
```

`APPLE_NOTARY_PROFILE` 是钥匙串中的配置名，也可写入 `.env`；设置后优先使用钥匙串认证。缺少签名或公证配置时，发行入口在编译前退出。仅构建 App 可运行 `zsh scripts/build_macos.sh`；单独重新打包须显式传入 App 和输出 DMG 路径：`zsh scripts/package_macos.sh /absolute/hear.app /absolute/output.dmg`，并自行传入签名、公证环境变量。

长时资源观测可运行 `zsh scripts/measure_macos.sh 600 /tmp/hear-process-usage.csv /Applications/hear.app new /tmp/hear-resource-report.html`。脚本每分钟记录安装版核心、worker 与界面的实际可执行路径/PID、CPU 累计时间、物理内存足迹、常驻内存、磁盘 IO 和进程能耗计数；允许短暂重叠的 worker，并在重启或进程缺席时留下可追溯样本。报告每 15 次采样自动更新一次，同时生成同名 `.html.json` 摘要；`HEAR_REPORT_EVERY` 可调整频率。意外停止后以 `resume` 代替 `new` 续写同一个 CSV，先校验表头，不覆盖原曲线。也可用 `python3 scripts/report_macos_resources.py <一个或多个 CSV> --output /tmp/hear-resource-report.html --summary /tmp/hear-resource-report.json` 合并重叠采样。报告按 PID 分段显示内存和单核 CPU 曲线、峰值、P95、末值与超过 150 秒的采样空档；空档内的峰值未知。进程能耗计数只能用于同机趋势，不能替代整机电池续航实测。录音覆盖、可回放性与来源缺口仍须以核心状态和媒体清单单独核对。

CLI 的 `transcribe-range` 返回 `range_id` 和 `tasks`；每条任务有可用于 `cloud retry` 的真实 `id`、`source`、起止时间和当前 `state`。双路或长范围可能产生多条任务；无可用原声会返回 `range_no_audio`，不会留下空的精转范围。

精转文字优先保留完整内容与供应商时间：时间能映射到原请求且当前归属有效，就保存文字，允许偶发长静默跨度；VAD、当前原声留存和声纹质量不再导致整段文字丢失。声纹和回放各自检查真实原声。最新修复、证据与安装状态见 [精转失败调查](specs/research/2026-10-22-asr-publication-failures.md)。

已有事件可以在详情的「音频信息」中重新精转；命令行可运行 `hear event retranscribe <event-id> <revision>`。新任务按当前云端适配器处理整场可用原声，原有人工修正只在可确认的同一时段自动恢复，其余进入待核对。

## 开发验证用云端配置

本机原型从仓库根目录 `.env` 读取 `DEEPSEEK_API_KEY`（事件拟合 LLM）和 `VOLCENGINE_API_KEY`（火山引擎录音文件 ASR）；变量名示例见 [`.env.example`](.env.example)。`.env` 已被 Git 忽略，不在文档、日志或测试输出中保存密钥值。当前优先验证 DeepSeek 官方 API 与火山引擎「录音文件识别标准版HTTP」，具体接口与尚待验证的限制见[技术 RFC](specs/prd/tech_rfc_0.1.md)。正式 macOS 客户端仍由用户在设置中配置凭据并存入 Keychain，不能把开发机 `.env` 打进安装包。
