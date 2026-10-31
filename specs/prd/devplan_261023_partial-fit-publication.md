---
status: implemented
created: 2026-10-23
updated: 2026-10-23
---
# 整理结果按字段、按事件保留，局部失败不扩大

用户明确要求弱化校验、拒绝过度防御：一个字段异常只作废该字段，单条异常不能回滚其他可用结果。今天两个十五分钟计算窗被覆盖/未整理下标校验拒绝，又阻塞数小时事件的后续精转。本次在既有事件发布、精转调度和时间线投影内解决，删除多余校验，不新增恢复框架。

## 执行台账

### 开发契约
- 必赢场景：坏摘要/标题/续接字段不影响有效时间及内容；坏事件与好事件混合时好事件照常提交；未整理下标错误、遗漏观察不再拒绝整窗；未命名素材保留原声和粗转。已存在的有效事件不能因为部分坏响应被删除。
- 字段规则：无效摘要置空；无效标题沿用已有标题，新建用“普通会话”；无效续接标记作废；无效时间/来源或同源冲突只跳过受影响事件。没有有效事件的合法数组也完成本次整理，素材留在未整理投影，不自动重复付费。不能解析的整个 JSON / 缺失 events 数组仍走既有有界重试。
- 精转：终态失败的整理窗口不再阻塞既有有效事件；仍待处理的计算/本地任务保留正常等待，人工边界、实际原声、清理与已提交请求复用不变。错误不因隐藏粗转而失去入口。
- 原子交付：Core 发布/调度、必要 UI、测试和文档同批完成。真实旧响应可含 unorganized，但该字段不再裁决发布。无数据迁移、额外付费补跑或自动历史恢复；本次不替换运行安装包。
- 必须保证：Core 唯一写入；任务版本/清理墓碑/人工修改优先；真实时间与来源；已有有效内容、稳定候选 slot、跨窗同一事件与已付费 ASR 引用不被坏字段破坏。数据库写入失败仍回滚事务，不把存储损坏当模型字段异常吞掉。
- 不保证：模型语义、任意历史错误自动修复、真实录音必然正确命名、全库其它处理器行为统一重写。ASR 供应商句段解析不是本次整理字段校验的改动范围。
- 状态来源：candidate_windows/cloud_tasks 是计算状态；named_events/event_candidate_links 是已发布结果；asr_inputs/event_asr_refs/state 是精转事实；原始素材独立保存。UI 只读 Core 派生状态。
- 复用：publish_fit_with_input、fit_nonoverlap、summary_for_scope、task_current、timeline、asr::settling_state/refresh/plan、现有云队列与手动 retry。
- 新增架构机制：无新增持久状态、owner、协议版本、重试器；直接缩小校验失败的作用范围。
- 适用架构：specs/ARCHITECTURE.md、specs/tech_docs/algorithms.md；本仓库无 AGENTS.md/CLAUDE.md。

### 验收与结束条件
- mandatory：生产发布函数的字段异常/混合事件/slot 稳定/旧结果保留/续接/清理回归；终态整理失败不阻塞精转及付费结果复用；失败素材隐藏过滤回归；workspace 静态/测试/构建和隔离 Core/CLI 烟测。
- mandatory：今天冻结库上以合成异常响应回放实际发布和调度，验证两处窗口可独立收敛、后续精转可规划；原响应没有保存，不声称重放了真实模型响应。不访问云 API、不启动冻结库服务、不写活跃库。
- 前端过滤变化使用现有 React 渲染/交互与浏览器夹具验证；不修改原生采集，无新增 OS 验收项。长期真实模型语义为 advisory。
- 改发布/调度重跑 Rust 回归与冻结回放；改 UI 重跑前端静态/相关测试/build。最终组合态门禁与 requirements/adversarial/architecture 三路独立审查通过后提交。发布安装不在本次范围。

### 开发批次与行动清单
- Batch 1：同一局部错误扩大根因。
  - [x] 字段/事件独立接纳，移除覆盖和 unorganized 全窗拒绝。
  - [x] 终态整理失败与精转解耦，失败素材入口可见。
  - [x] 合成回归、冻结回放、整体门禁与文档。
  - [x] 三路审查、修复、提交与交付。

### 当前批次 Review 基线
- Base: 97e97f9a66ad3086e32a73352481a85704b37c9c。
- Branch: fix/partial-fit-publication。
- 预期范围：events.rs、events/activity_tests.rs、events/asr.rs/tests.rs、diagnostic_tests.rs；TimelineView.tsx 与相关测试；架构/算法/诊断文档及本计划。

### 待用户决策
无。用户已明确选择优先保留可用结果、弱化校验；局部实现不再要求确认。

### 进展日志
- 2026-10-23：核实运行版 0.1.5 与基线一致，工作树干净；私有冻结证据 /var/folders/lt/tx2xdhgx3g97md2696nm0fww0000gn/T/hear-audit-20261023-fmqfvjp9。建立本契约，开始单批实现。

- 自验证：`python3 scripts/verify.py --suite all --report /tmp/hear-partial-fit-verification.json` 全通过：318 Core、11 desktop、9 protocol 测试，53 前端、3 资产、6 Python/native policy、静态检查、构建、组件身份、隔离 Core 与诊断烟测。12 个 opt-in Rust 测试默认跳过，其中本次冻结回放另行执行。
- 冻结回放 PASS：两处失败窗口均以合成异常响应独立发布；83,189 条原声块、10,626 条粗转、14,651 条精转及 1,112 个已有付费请求身份/提交次数保留。先验证失败邻窗不再阻塞前一会议，再发布和规划，引用 1,158→1,159；不代表真实模型响应恢复，也未发云请求。
- 浏览器 PASS：`scripts/test_partial_fit.html`，隐藏粗转时仍有失败素材卡片及详情入口，整理异常与精转完成并列，切换过滤可查看粗转。截图保存在本机 /tmp/hear-partial-fit-browser-evidence/partial-fit.png。

### 本批 Review Contract
- Mode: repair；requirement source 为本文及用户“弱化校验、字段异常仅作废字段、局部失败不扩大、拒绝过度防御”的明确指令。
- Target/base: 当前完整工作树相对 97e97f9a66ad3086e32a73352481a85704b37c9c。
- In-scope: crates/loppi-core/src/events.rs、events/activity_tests.rs、events/asr.rs、events/asr/tests.rs、diagnostic_tests.rs；src/TimelineView.tsx；scripts/test_timeline_view.mjs、test_partial_fit.html；specs/ARCHITECTURE.md、tech_docs/algorithms.md、tech_docs/diagnostics-and-verification.md、本计划。
- Out-of-scope: 私有证据、活跃数据库和安装包、共享缓存/构建输出；没有其他 writer 改动。
- 保证/非目标/本次负责问题/允许机制：严格按上方开发契约。不把被用户明确取消的完整覆盖、整批拒绝、固定数量、无效条目清除旧结果重新升级为保证；没有新增状态、owner、队列、协议或恢复器。仅本次引入/显著暴露或本需求明确拥有的问题阻塞。
- Required lenses: requirements、adversarial、最终 architecture，fresh-context、只读。
- 检查：全套静态/测试/构建/隔离烟测、冻结库合成回放、浏览器已通过；修改按验收中的影响关系重验。原生采集、真实云端语义和发布安装不在本批。

### Phase 1 独立审查与裁决
- Requirements：PASS。字段独立接纳、删除整批覆盖/下标/数量拒绝、slot/旧内容保留、任务/清理/人工边界、精转解阻与付费复用、失败素材入口和全部验收证据均通过；无阻塞或必需证据缺口。
- Adversarial：PASS。未发现本批引入或显著暴露的可达错误传播/状态/生命周期问题；独立重跑混合字段及 slot 保留、存储回滚、终态调度、ASR 输入复用四项回归通过。无新增可证实风险。
- 主 Agent 裁决：两路均无阻塞，无代码修复需求。原验证结果继续有效；交给最终 architecture 审查完整候选，不追加无关防御。

### 最终架构审查与整体门禁
- Architecture：PASS。确认逐字段/条目接纳在既有 Core 发布事务；删除了整批强反序列化、覆盖/下标/数量/重复续接拒绝、遗漏 slot 删除路径；精转调度与 UI 复用既有事实，无新增机制或版本迁移义务。
- 三路审查全部完成，无阻塞、无必需证据缺口、无代码修复。最终候选代码与已通过的 14 组门禁、冻结回放及浏览器证据一致。
- PRD 级复核：本批所有范围一次交付；原始数据、清理墓碑、人工优先、付费 ASR 引用和待处理任务等待均按契约验证。无中间态兼容壳、未完成 mandatory 或待决策项。发布安装和真实模型任意语义不属本批承诺。

### 交付
- 实现提交：5ad5076（fix: preserve usable event results when model fields are invalid），分支 fix/partial-fit-publication。
- 全部 mandatory 范围、最终组合态门禁、三路审查通过，状态 implemented。唯一后续发布动作是安装候选版本；本次没有修改活跃数据或替换 /Applications/hear.app，也没有调用付费恢复任务。
- 已知边界：有效 events 数组完成的是一次尽力整理，未覆盖素材仍为未整理；不能保证模型语义正确。整份无效 JSON/events 容器和真实数据库错误仍可导致任务失败。原失败响应未保存，私有回放只证明规则与调度，不宣称真实结果已恢复。
