---
status: in-progress
updated: 2026-11-02
---

# 首次准备页细节调整

将顶部「准备 loppi」中的产品名替换为现有 SVG 字标，统一前三项成功状态为文字右侧的绿色对号，三步骤标题字号加大一级。云服务每行按「事件分析 · DeepSeek / 语音识别 · 火山引擎 → 获取密钥 → 输入框」排列，直接打开对应密钥控制台，删除原来的获取密钥折叠入口及其说明。「稍后」移动到「完成设置」左侧，两者字号一致。另外将左侧栏顶部 logo 水平居中。使用 dev 分支，项目版本从 0.1.19 升至 0.1.20。

## 执行台账

### 开发契约
- 必赢场景：用户在首次准备页识别品牌与三个步骤，直接进入各服务商密钥页面，填入后沿既有验证流程完成设置；也可从右下角选择稍后。
- 原子交付：五项准备页 UI 调整、侧栏 logo 居中与 0.1.20 版本同步一次完成；不改变既有配置格式和持久数据。
- 必须保证：DeepSeek 跳转 https://platform.deepseek.com/api_keys；火山引擎跳转 https://console.volcengine.com/speech/new/setting/apikeys?projectName=default；原生链接打开继续走固定服务商白名单；原有完成门禁、保存行为、失败提示和云处理披露保留。
- 明确不做 / 不保证：不合并 main 或发布 GitHub、不新增录音权限或云验证机制、不使用真实密钥/付费服务。用户追加授权提交当前仓库所有未提交内容，并从干净提交构建本机 ARM64 release 包。
- 本次负责的问题：五项呈现需求、侧栏 logo 居中、两个固定目标地址、版本一致性；现有其它平台/录音行为不在范围。
- 状态来源与生命周期：模型、声音与队列状态仍来自 props/Core；CloudKeyInput 继续拥有临时草稿与验证中状态；本次不新增状态。
- 复用的既有抽象：assets/brand/wordmark.svg、ModelPreparation、SourcePreparation、CloudKeyInput、Button、open_setup_help、windows_host::open_url、color-complete-text 等语义 token。
- 新增架构机制：无。
- 适用架构不变量：Core 继续为业务唯一写入者；前端仅呈现已有事实；固定 URL 由原生命令选择，平台打开由现有实现承担；组件不新增私有色值或尺度。

### 验收与结束条件
- 验收标准：五项需求逐条成立，左侧栏顶部 logo 在标准与收窄侧栏水平居中；1120×760、760×540 及大窗布局无重叠/水平溢出；键盘可操作获取密钥、稍后与完成设置；未配置/已配置/错误状态仍可用；项目七处版本与五个 Rust 包锁定版本一致为 0.1.20。
- 事实依据：真实 React 组件合成数据浏览器截图/交互，固定 URL 命令源码与 Rust 检查，前端类型/lint/build、受影响 Node 测试与版本解析核验。
- 改动与检查对应：TSX/CSS 改动重跑静态、前端 build、受影响交互/截图；Rust URL 变更重跑 workspace check 与 onboarding 单测；版本只变更需元数据/锁文件一致性验证。无需因文档收尾重跑构建。
- 结束条件：上述验证与三路独立 cross-review 完成，无范围内阻塞后提交；不因主观观感或范围外已有问题追加工程。
- 真机边界：此更改仅替换既有原生打开命令的固定常量，不改变 OS 调用或权限机制；浏览器验证证明 UI 发出正确 provider，原生路由与构建证明固定 URL。全安装/录音验收属范围外。最终主观观感复核 advisory。

### 开发批次与行动清单
- Batch 1（唯一批次）：准备页、侧栏字标对齐、固定链接与版本号，共同验证后提交。
  - [x] 实现五项 UI 与链接调整。
  - [x] 同步 0.1.20 版本与更新记录。
  - [x] 静态、构建、针对性测试与浏览器检查。
  - [x] requirements / adversarial / architecture 三路审查并处理范围内问题。
  - [ ] 按用户追加授权提交当前仓库全部未提交内容；从干净 dev 提交签名、公证 ARM64 release 包。

### 当前批次 Review 基线
- 分支：dev（已有分支复用；切换冲突文件逐字节备份恢复）。
- Batch base：dd01561f3e7829429b308f229ee4240493f0c755。
- 预期范围：App.css、OnboardingView.tsx、onboarding.css、SetupGroups.tsx、onboarding.rs、test_onboarding.html、版本文件与本台账。
- 共享工作区基线：/tmp/loppi-onboarding-baseline 保存本次触及文件的原始工作区快照。版本文件与 CHANGELOG 原有未提交修改不属于本次。

### 待用户决策
无。

### 进展日志
- 2026-11-02：读取用户五项需求、start-dev、CONTRIBUTING 与相关设计/架构规则；复用现有 dev，保留所有原有内容。记录工作区基线后开始单批次实现。
- 2026-11-02：完成 UI、固定链接与版本同步；静态门禁通过，11 项受影响 Node 测试与 2 项 onboarding Rust 单测通过。浏览器 macOS/Windows 呈现各 6 项回归通过；1120×760、760×540、1920×1080 均无水平或行内溢出。键盘密钥入口、打开失败提示、凭据失败提示与稍后导航通过合成数据检查。完整 ARM64 开发构建进行中。
- 2026-11-02：需求与异常路径审查 PASS，架构审查 PASS，ARM64 开发包构建与隔离模型 smoke 通过。提交前用户追加侧栏顶部 logo 居中，纳入当前单批次契约；复用既有 .brand flex 容器，将窄窗已有的居中规则提升到基础样式，追加布局证据与定向复核后统一提交。
- 2026-11-02：用户明确追加“仓库没提交的代码都提交”与 ARM Mac release 安装包要求。提交范围扩大到整个当前工作区，保留其它 PRD 原有验收状态；补充现有 AppOpened/GUI activation 代码的独立集成审查与全量离线 verify。release 从干净 dev 提交构建，复用既有签名、公证、资源与安装包校验，不合并 main、不推送或发布 GitHub。
- 2026-11-02：追加集成的三路审查全部 PASS；全量 21 项离线门禁通过，0.1.20 同 epoch Core/GUI driver 的 activation smoke 通过。准备提交全部已授权文件。许可清单计数/哈希落后的既有非阻塞资料问题已记录在 evidence/review.md；所有新增依赖通知已有独立 updater 文件随包保留。当前仅剩提交后干净源码的 ARM64 release 签名、公证及交付收尾。
