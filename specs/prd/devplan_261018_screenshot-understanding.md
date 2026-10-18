---
status: implemented
updated: 2026-10-18
---
# 新截图自动图片理解

需求来源：用户要求已配置 LLM 时直接读图，输出图片摘要和完整 Markdown 转录；未配置 Key 用原本地 OCR。最终明确「历史不管，新的自动。保持简单」。

## 执行台账
### 开发契约
- 必赢场景：新截图立即保留原图和本地 OCR；有 LLM 配置时在现有队列后台直接读图，发布中文摘要及忠实 Markdown 转录。没有 Key 保留旧逻辑。
- 原子交付：核心任务、持久字段/迁移、供应商图片请求、界面和文档一起完成；已有数据库及历史 Moment JSON 需兼容。
- 必须保证：仅 capture 保存入口创建任务，历史不回填；队列暂停/恢复/重试/统计复用；原图与 OCR 在云失败时仍可用；清理不能被迟到结果复活；摘要与转录原子发布并可搜索；看不清不编造。结果及时刷新。
- 明确不做：历史重新识别入口/自动回填、新供应商、新开关、自定义重试系统、保证识别每个小字准确。
- 本次负责：上述新增路径引入的持久化、生命周期、安全呈现及旧数据兼容问题。
- 状态来源：moments 保存本地 OCR 与云结果；cloud_tasks 为唯一云处理状态；cloud_queues/credential refs 决定保存时是否入队；UI 只读投影，无第二套任务状态。
- 复用：moments::save_capture/image/list，events::take_task_fair/task_current/task_failed_with_retry/recover_tasks，cloud::chat 计量/响应解析，usage::Recorder，cleanup 的 outbound gate 与全部数据清理，storage::migrate。
- 新增机制：cloud_tasks 增加 screenshot 目标类型（复用队列）；moments 增加摘要、Markdown 字段；MomentView 派生任务状态。无新 owner/store/retry。
- 不变量：网络在 Store 锁外；原图读写受既有清理屏障约束；正文/凭据不进日志；只发送用户主动截图。
### 验收与结束条件
- 新增截图在配置有/无 Key 时分别有/无图片任务；旧截图不会因配置或重启入队。
- 验证队列暂停、恢复、失败/重试、清理迟到结果、搜索更新、迁移保留已有任务和 ASR 引用。
- 验证供应商实际图片内容块、结构化结果校验；UI 摘要/Markdown/空转录/本地 OCR/等待和失败状态。
- 静态：typecheck、lint、cargo check；针对性 Rust 测试及前端浏览器烟测；前后端构建。模拟供应商验证完整处理链路，不使用用户私密截图作外部测试；真实供应商质量属于 advisory。
- 按影响重跑，不扩展无关测试。全部 required cross-review PASS 后提交。
### 开发批次与行动清单
- Batch 1（一个端到端批次）
  - [x] 核心策略、迁移、图片适配和测试
  - [x] 摘要/Markdown 呈现、队列和配置文案、文档
  - [x] 验证、cross-review、提交
### 当前批次 Review 基线
- Batch base: f7567399cf2d4387b8902a509ea40deed213092e
- 范围：moments/events/cloud/main/protocol、MomentList/api/DetailDrawer/WorkspaceViews/App.css、相关测试与架构设计文档及本台账。
### 待用户决策
无。历史忽略，只自动处理新截图；其余复用现有机制。
### 进展日志
- 2026-10-18：核实官方 deepseek-flash 支持 image_url/base64；直接读图而非 OCR 润色。前述讨论收敛，进入单批次开发。

- 2026-10-18：9 个截图相关 Rust 测试通过（含本地 HTTP 供应商模拟→双输出发布/计量、持久队列恢复、暂停/重试、历史不回填、清理和迁移外键/视图保留）。UI 浏览器 fixture PASS，覆盖 Markdown 结构与复制、活动 HTML/远程资源禁用、等待到完成自动刷新、空转录及 240px 窄栏。
- npm audit 的 source-map-js 1.2.1 是基线已有开发依赖告警；本次未变更该版本。前端 bundle 有 500KB 大小提示，不影响构建；不借此扩大范围。

- 2026-10-18：完整核心测试 247 PASS、3 个既有重型测试 ignored；协议 8 PASS；云任务状态 2 PASS；typecheck/lint/frontend build/cargo check PASS。完整 macOS app 构建并深度签名验证 PASS。签名产物隔离启动旧 schema 烟测 PASS，保留历史 OCR、未创建历史云任务。
- Cross-review：requirements PASS；adversarial PASS（独立复跑 9 个截图测试）；architecture 发现新增 Markdown 样式未使用语义 token，裁决为本次违反既有设计规范的阻塞问题，改用既有 token；定向复核 PASS。CSS 修复后 typecheck/lint/build/浏览器完整 fixture PASS，核心证据不受影响。未新增状态或扩大范围。
- 最终边界复核：无历史回填/手动重识别入口/新 provider/新 queue；持久旧数据兼容有实际升级测试。云供应商精确识别效果为 advisory，未上传任何真实用户截图做验证。

- 2026-10-18：Batch 1 提交 f036e0a。最终 CSS token 修复后的 macOS app 再构建/签名检查 PASS。单批次整体门禁已完成，无额外完整 review 必要。
- 已重启签名 GUI 与后台核心，恢复原本录制意图；原生界面确认“正在记录，麦克风·电脑声音”，设置显示“自动整理事件与新截图”及发送说明。未触碰历史截图。实际云端图片质量属于用户可选体验观察，不是未完成实现项。
- 验收方式（advisory）：新截一张有文字的图片，等摘要出现后展开完整转录并复制 Markdown；旧截图仍显示本地识别文字。无 Key 分支和暂停/重试已在隔离测试覆盖，不要求用户删除现有 Key 来验收。
