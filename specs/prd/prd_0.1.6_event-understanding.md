---
status: in-progress
created: 2026-10-24
updated: 2026-10-24
scope: 窗口内生成并续接事件摘要及结构化理解，完成内部存储、引用与失效链路
issue: null
research: specs/research/hear-event-understanding-adaptation-assessment.md
review: passed (requirements, adversarial, architecture)
---

# 事件理解的窗口续接与存储

## 执行须知

本文件是独立可执行的原子交付契约。先读 `specs/ARCHITECTURE.md`、`specs/tech_docs/algorithms.md` 和 `diagnostics-and-verification.md`。实现 owner 是 `crates/hear-core/src/events.rs`，纠错和清理分别由 `knowledge.rs`、`cleanup.rs` 发起。仓库无 CLAUDE.md。先核对真实代码，不沿用研究阶段旧版 `unorganized` 输出要求。

用户已明确要求“按这个范围写一份 PRD……然后直接开始正式的开发到完成”，因此本次成稿承接已收敛需求，记录 ready-for-development 审核通过后直接进入 in-progress，无二次确认步骤。整个交付包含实现、测试、独立三视角审查与提交；不包含安装、签名发行或修改正在使用的录音数据库。

## 背景与目标

hear 自动记录全天音频。事件摘要首先帮助用户认出“说的是哪件事”，需要保留具体对象、例子、方案与分歧。用户确认：沿用现在的窗口分析，把上一轮摘要和结构化结果带进去更新；摘要超过几行在界面省略，数据层不限制；新增字段先做好存储与链路，不单独展示。

必赢场景：同一次讨论跨越多个计算窗口，后窗仍更新同一个事件，保留前窗有效细节与引用，并纳入新的进展；人工修改或清理后，过时的理解不再参与续接或被迟到响应恢复。

## 原子范围与反向边界

- 复用现有约 15 分钟计算窗口、前序自动事件选择、候选关联 slot 和同一 LLM 请求。模型负责语义合并、去重和更新，Core 负责归属、引用和生命周期合法性。
- 沿用 `summary` 列，新增可空 `named_events.understanding_json`。同一事务更新事件、摘要、结构化结果与 revision。
- 只新增 topics、key_points、mentioned_objects；不加入场景技能、行动项通知、全局实体库、全文事件重分析、独立任务队列或状态机。
- 前端沿用 `cardExcerpt` 和 `.event-card p` 的两行省略。无需新增协议字段、详情面板、引用跳转、搜索或导出功能。
- 不自动补算历史，不修改候选输入哈希策略版本；已完成且输入不变的历史任务不因升级付费重跑。新任务与本来需要更新的任务使用新格式。
- 粗转不能保证语义正确；引用证明可追溯到输入，不代表算法验证了事实真伪、人物身份或逐词对应。

## 输出契约

原事件时间、title、sources、merge_with_event_id 不变。新增 `understanding` 为 null 或下列对象：

```json
{
  "topics": ["项目发布排期", "测试范围"],
  "key_points": [
    {"text": "讨论倾向先验证导出，再决定是否按周五发布；发布日期尚未确认。", "evidence_indices": [0, 1]}
  ],
  "mentioned_objects": [
    {"name": "导出功能", "type": "feature", "context": "本轮发布前需要验证的功能。", "entity_id": null, "evidence_indices": [1]}
  ]
}
```

- `summary` 是完整可读的一段话：先用两三句定位活动及主线，再适度展开关键细节。建议约 500 字以内，仅为生成指导；不因超过 240 或 500 字截断、拒收或置空。保留已有空白归一化，空白、错误类型、NUL 仍视为不可用。
- summary 负责认出事情和理解讨论如何展开；key_points 保存值得单独引用的独立事实、观点、结论及其限定条件，不把摘要拆句复制，也不为凑数补条目。建议、设想、分歧和已确定事项要区分。
- topics 是开放主题标签；mentioned_objects 只取讨论中有辨识意义的对象，type 是开放字符串，context 说明本次语境。entity_id 本次一律存 null；不得接受模型猜测的全局绑定。
- 三个数组都必需；空数组表示本次可用材料没有值得提取的对应条目。字段缺失、类型错误、null 或整体不可解析时，仅把 understanding 置空，不丢弃有效事件或摘要。单条文本空白/NUL、引用无效的要点/对象丢弃该条。主题与完全相同条目去重；语义去重交给模型。
- 完整理解是最新快照，不是追加日志。合法输出替换旧快照；缺失/损坏输出置空，不能将旧结构伪装成更新后的结果。摘要与结构各自可用性独立。

## 引用契约

1. 每次请求构造临时 `evidence` 目录（从零编号）。当前 observations 中有非空文字的粗转句段进入目录，observation 附 `evidence_index`；无文字人声不能作为文字结论证据。
2. 前序/已关联事件结构化结果已有的引用也进入目录。相同句段按稳定 ID 去重；相同 ID 的来源、原始起止必须一致。继承引用在入参中重新映射为本次 evidence_indices，不能直接沿用上轮数字。旧原文不全量重送，只携带旧理解、引用元数据及现有有限 tail。
3. 目录项包含 `segment_id`、`source`、`start_ms`、`end_ms`，时间是原粗转句段绝对毫秒。跨窗句段的引用指向完整原句，observation 与事件可写时间仍裁在当前窗口；不能把句内某个词的时刻当作已知事实。
4. 入库将每项 evidence_indices 转换为 `evidence_refs` 数组，内容是目录中的稳定引用；禁止持久化请求下标。结构对象附 `schema_version: 1`。不另建引用事实表，不复制原文。
5. 只允许引用本次当前 observation，或当前实际续接/更新目标原有的引用，禁止借用另一并行事件的引用。每条引用必须为非负整数、在目录内、有非空粗转原文、与最终事件来源相符且时间相交；非空要点/对象至少有一个引用。某条含未知/越界/跨事件引用则整条丢弃，避免删掉坏引用后保留无依据的断言。
6. 继承引用读取时核对粗转行是否仍存在，来源和原始时间是否一致；不一致的旧理解不送入模型。发布前重新构造并比较完整实际输入，含当前观察、引用目录、旧摘要/结构与 revision。输入已变时本次摘要及结构均不发布，沿用现有事件范围接纳规则。若只是上下文修订，而实际观察及锚点、最终事件范围/来源未变，则保留数据库中当前仍有效的快照，不能用空值覆盖；范围或原始依据变化时清空。

## 窗口续接与存储

`llm_input` 继续通过 `fit_context` 选择前序及 linked 事件。结构、摘要和 revision 一起读取；前序 tail 仍沿用现有上限。模型接收前轮完整有效快照，在新窗口补充、纠正、合并重复条目；旧要点未被推翻仍可保留旧引用。新事件不能继承另一事件结构。

`publish_fit_with_input` 复用任务版本、清理墓碑、来源、非重叠及人工锁校验。结构化更新本身可以递增事件 revision，但不能重新创建 ASR 任务；只有原有范围/来源规则触发精转。完全相同结果为 no-op。被遗漏或非法的事件输出仍不是删除指令。

旧事件没有结构化结果时：若全部事件范围在当前候选内，可依据本窗口生成；若延续的事件有窗口外历史且旧结构为空，本轮结构保持 null，不将局部理解冒充全事件。摘要沿用相同的完整性规则，分别判断旧摘要是否存在。这也适用于失效后的结构；不增加自动全事件恢复任务。

持久化为一个 nullable TEXT JSON 列，迁移使用 `storage::migrate`，重复启动幂等；旧记录默认 null。JSON schema_version 是内部存储格式，不提升 GUI IPC 协议号。Core 是唯一写入者，SQLite 快照是唯一事实源；无新缓存。

## 失效与并发规则

| 操作 | 摘要和结构处理 |
| --- | --- |
| 仅改标题 | 保留；旧请求因 revision 变化不能恢复旧语义结果 |
| 人工改事件范围或来源 | 同一事务清空两者，保留现有 boundary_locked 规则 |
| active/pending_review 人工文字纠错 | 按来源和范围清空；发布时存在纠错则禁止基于粗转恢复 |
| 仅重跑精转、改人物名 | 不改变粗转依据，不自动清空或重算理解 |
| 清除原始音频 | 保留文字、摘要、结构及粗转引用 |
| 清除全部数据 | 按来源/范围清空；若实际删除整句越过选择区间，连带清空依赖该句的相邻事件；迟到结果不得恢复 |
| 删除/拆分/合并事件 | 沿用现有事件生命周期；新建事件默认没有理解，已删除事件不作为输入，不迁移其快照 |
| 原粗转引用缺失或元数据变化 | 旧结构不进入续接输入；发布快照校验拦截在途旧结果 |
| 任务失效/失败/重启 | 沿用 cloud_tasks；无额外任务、重试与自动历史补算 |

清理和纠错统一走 `invalidate_event_understanding`（由现有 invalidate_summaries 扩展并改名），避免调用方只清一个字段。SQL 直接写入的清理整句分支同样同时清空两者。

## 验收标准

1. 合成数据的连续两窗/多窗事件保持相同 event ID；旧摘要、要点及对象作为后窗入参；引用重新编号后仍还原到同一粗转 ID。独立并行事件不能串引用。
2. 新事件、重复更新、结构单独变化、非法引用/类型/空值、超过 500 字摘要都按契约保存；结构变化不新增 ASR 任务，相同结果不增加 revision。
3. 迁移可重复、旧数据可读、不自动重排历史任务；legacy 跨窗结构空值不伪造全量理解。
4. 范围/来源修改、标题编辑、人工纠错、原声清理、全部清理及整句越界清理符合表格；保存中的任务/输入变化与迟到结果无法复活旧结果。
5. Rust 核心与相邻清理/纠错回归通过；TypeScript/lint、Rust check/build 通过；隔离 Core 烟测通过。默认验证只用合成数据，不外发用户真实录音或转录。
6. 阅读现有前端并跑时间线回归证明不截断数据、仍按两行显示；不新增 UI 或协议消费路径。模型实际语言质量仅作为后续产品观察，本次不以付费真实录音重放为交付门槛。

## 执行台账

### 开发契约

- 单一 Batch 完成整份 PRD；真实兼容责任为已有 SQLite 数据和现有 GUI/CLI，使用可空列且不改变 IPC。
- 必须保证与明确不做见上述契约；本次只负责新增理解链路及其影响的摘要、失效和续接。
- 状态来源：named_events 摘要/理解为派生快照，coarse_segments 为引用权威；生成与发布均由 events owner，knowledge/cleanup 发起失效。
- 复用：llm_input、fit_context、publish_fit_with_input、task_current、summary_for_scope（现 interpretation_for_scope）、storage::migrate、edit_event_in、queue_asr、SQLite 事务。
- 新增机制仅一个可空存储列、内部格式与纯映射辅助模块；无服务、队列、持久化索引或缓存。引用目录是单次请求派生值。
- 人工修订/清理优先，候选不是事件边界；云结果不能写入未被授权的引用归属。

### 验收与结束条件

每项行为以合成单测/集成回归为依据，Core 烟测验证真实二进制与迁移启动。Rust 修改后重跑相关测试和 check/build；文档修订不重复高成本检查。无新 OS/权限/采集交互，签名安装与主观摘要效果为 advisory，不属于本次交付。所有验收和三视角 review 通过后提交并结束，不扩大范围。

### 开发批次与行动清单

- Batch 1：事件理解作为既有窗口处理的一部分原子交付。
  - [x] 核对当前实现并完成契约。
  - [x] 存储、请求引用目录、续接与发布。
  - [x] 摘要 prompt、无长度硬限制、统一失效。
  - [x] 合成回归、静态/构建/运行验证与文档同步。
  - [x] requirements / adversarial 审查、修复、最终 architecture 审查。
  - [ ] 提交与 PRD 整体验收。

### 当前批次 Review 基线

- Batch base：18cba03。
- 预期拥有文件：本 PRD；events.rs、events/understanding.rs 与其测试；cleanup.rs、knowledge.rs；algorithms.md、ARCHITECTURE.md 中相关说明。
- 工作树已有版本号、Cargo/package 锁、src/App.css、src/Controls.tsx 改动均不属于本任务；其它 untracked 资料不纳入提交。私有真实示例不提交。

### 待用户决策

无。

### 进展日志

- 2026-10-24：用户授权成稿后直接开发；ready-for-development 审核通过，建立台账并进入 in-progress。

- 2026-10-24：实现 nullable JSON 迁移、单次引用目录与稳定引用、三窗续接、原子发布与统一失效；保持候选策略版本与现有 UI/协议。
- 2026-10-24：Core 全量回归 325 passed / 12 ignored；补充持久库重开后，8 项 understanding 定向测试全部通过。TypeScript、lint 与 11 项时间线测试通过；已有两行 CSS 省略无需修改。工作区 Rust check/build 和隔离 Core 烟测执行中。

- 2026-10-24：第一轮 requirements/adversarial 均只发现同一 P2：仅改标题后迟到响应清空有效快照。裁决为契约内 blocker（结构为新增影响，摘要亦在本 PRD 责任内），在原发布入口修复：输入过时但观察/锚点和最终范围/来源未变时保留当前有效快照；范围/证据变化仍清空，人工纠错仍优先。覆盖标题编辑及更新后的快照先到、旧响应后到两种顺序。
- 2026-10-24：修复后 Core 全量 326 passed / 12 ignored，workspace check/build 通过；原静态前端与时间线证据仍适用，正在完成定向复核与最终 architecture 审查。

- 2026-10-24：requirements 与 adversarial 定向复核均 PASS；全新 architecture 审查 PASS，无额外机制或范围扩张。隔离 Core/CLI 烟测通过（查询/修订/日志、日志存储失败、FIFO 路径、登录 owner 交接），TypeScript/lint 最终再验通过。全部本次验收已满足，无 mandatory 项遗留；尚未安装新版，真实模型措辞质量留作产品观察。

### 最终验收证据

| 要求 | 证据 | 结论 |
| --- | --- | --- |
| 续接、引用归属、稳定映射 | understanding_three_windows_reindex_old_evidence_and_keep_event_identity / understanding_cannot_borrow_inherited_refs_from_other_parallel_event | PASS |
| 长摘要、结构更新、非法输出、无额外精转 | understanding_roundtrip_long_summary_metadata_only_update_and_noop / understanding_rejects_invalid_claims_but_preserves_event_and_summary | PASS |
| 迁移、重开、不补算 | understanding_legacy_migration_and_incomplete_continuation / understanding_survives_reopening_without_new_jobs | PASS |
| 编辑、纠错、迟到保护 | understanding_invalidates_on_scope_change_and_late_input_changes / understanding_corrected_scope_cannot_regenerate_from_coarse；knowledge 纠错回归 | PASS |
| 全部/原声/整句跨范围清理 | cleanup 原有回归新增结构清空与保留断言 | PASS |
| 静态、构建、运行 | npm typecheck/lint；cargo check/build --workspace --locked；cargo test -p hear-core --locked：326 passed / 12 ignored；scripts/smoke_core.py | PASS |
| 前端范围 | scripts/test_timeline_view.mjs：11 passed；现有 cardExcerpt 全文、CSS 两行省略；无前端改动 | PASS |
| 三视角 | requirements / adversarial 定向复核 PASS；最终 architecture PASS | PASS |

12 项 ignored 为既有显式模型/私有/规模测试，不属于本 PRD 门槛；没有外发真实转录或修改活跃数据。提交前校验其它 9 个 tracked 改动的内容哈希与开发前完全一致，未纳入本次暂存。
