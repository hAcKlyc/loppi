# hear

持续情境记录器。持续留下声音，通过初步识别、片段筛选与会话归组，形成包含音频、转写、截图、重点和评论的情境记录。

## 当前状态

0.1 macOS 客户端正在开发验收。签名版已跑通几分钟真实声音的录制、粗转、事件、精转、人物、纠错、笔记、导出和清理预览；覆盖升级与原有数据读取已有初验。多小时稳定性、资源测量、Apple 公证及 Gatekeeper 安装仍未通过，当前构建不能当作正式发行版。

| 文档 | 职责 |
|---|---|
| [PRD](specs/prd/prd_0.1.md) | 定位、用户路径、产品行为、数据语义、范围与待确认决策 |
| [整体架构](specs/ARCHITECTURE.md) | 工作树中的进程、模块与数据边界；区分实现与未通过的验收 |
| [核心算法](specs/tech_docs/README.md) | 声音处理工作流、事件拟合与全局声纹匹配 |
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

在有 Developer ID Application 证书的 Mac 上执行 `zsh scripts/build_macos.sh`，然后执行 `APPLE_NOTARY_PROFILE=hear-notary zsh scripts/package_macos.sh`。前者构建并逐层签名 GUI、登录项、worker 与 CLI；后者制作含 `hear.app` 和「Applications」入口的磁盘映像，验证签名与内容，提交 Apple 公证、装订公证票据并检查 Gatekeeper。`APPLE_NOTARY_PROFILE` 是使用 `xcrun notarytool store-credentials` 预先保存在本机钥匙串中的配置名。没有此配置时，脚本仍生成签名 `.dmg`，但以非零状态退出并明确标记公证未完成，不能分发。

长时资源观测可运行 `zsh scripts/measure_macos.sh 600 /tmp/hear-process-usage.csv /Applications/hear.app new /tmp/hear-resource-report.html`。脚本每分钟记录安装版核心、worker 与界面的实际可执行路径/PID、CPU 累计时间、物理内存足迹、常驻内存、磁盘 IO 和进程能耗计数；允许短暂重叠的 worker，并在重启或进程缺席时留下可追溯样本。报告每 15 次采样自动更新一次，同时生成同名 `.html.json` 摘要；`HEAR_REPORT_EVERY` 可调整频率。意外停止后以 `resume` 代替 `new` 续写同一个 CSV，先校验表头，不覆盖原曲线。也可用 `python3 scripts/report_macos_resources.py <一个或多个 CSV> --output /tmp/hear-resource-report.html --summary /tmp/hear-resource-report.json` 合并重叠采样。报告按 PID 分段显示内存和单核 CPU 曲线、峰值、P95、末值与超过 150 秒的采样空档；空档内的峰值未知。进程能耗计数只能用于同机趋势，不能替代整机电池续航实测。录音覆盖、可回放性与来源缺口仍须以核心状态和媒体清单单独核对。

CLI 的 `transcribe-range` 返回 `range_id` 和 `tasks`；每条任务有可用于 `cloud retry` 的真实 `id`、`source`、起止时间和当前 `state`。双路或长范围可能产生多条任务；无可用原声会返回 `range_no_audio`，不会留下空的精转范围。

已有事件可以在详情的「音频信息」中重新精转；命令行可运行 `hear event retranscribe <event-id> <revision>`。新任务按当前云端适配器处理整场可用原声，原有人工修正只在可确认的同一时段自动恢复，其余进入待核对。

## 开发验证用云端配置

本机原型从仓库根目录 `.env` 读取 `DEEPSEEK_API_KEY`（事件拟合 LLM）和 `VOLCENGINE_API_KEY`（火山引擎录音文件 ASR）；变量名示例见 [`.env.example`](.env.example)。`.env` 已被 Git 忽略，不在文档、日志或测试输出中保存密钥值。当前优先验证 DeepSeek 官方 API 与火山引擎「录音文件识别标准版HTTP」，具体接口与尚待验证的限制见[技术 RFC](specs/prd/tech_rfc_0.1.md)。正式 macOS 客户端仍由用户在设置中配置凭据并存入 Keychain，不能把开发机 `.env` 打进安装包。
