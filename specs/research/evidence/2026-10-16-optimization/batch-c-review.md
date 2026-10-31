# Batch C Review Contract

- Mode: repair。主 Agent 修改；reviewer 只读，不得派生 reviewer。
- Requirement source: `specs/prd/devplan_261016_reliability-performance.md` 与已接受研究的 F05/F06/F07/F08/F13；P0/P1 整体原子交付。本批内部稳定点不是发布完成。
- Exact baseline: `27f9c2788be40376b201b14c4b01b7a7b270b941`，target = 当前本批工作树。
- In scope: Cargo.lock；crates/loppi-core/build.rs、native/resources.m、src/{analysis,coarse,resources,cleanup,knowledge,main,retention,source_process}.rs；crates/loppi-worker/Cargo.toml、src/main.rs；specs/ARCHITECTURE.md、tech_docs/{algorithms,speaker-identity}.md；上述 devplan；本目录 batch-c-review.md、batch-c-quality.json。
- Out of scope: 已提交 A/B；无关未跟踪素材、playground、先前 research/旧 evidence；前端、播放、云公平调度与读连接属于 D。完整进程树和 GUI 峰值观测、长时/硬件/真实质量发布验收在 D/最终组合态继续，不能要求本批声称完成。
- Must guarantee: 现有交互、模型、阈值、来源/真实时间、人工优先、暂停和云外发范围不变；core DB 业务唯一权威；有声无字、未知/失败保留；清理后迟到结果不能复活；VAD 独立于粗转耗时；逐源连续状态/重放不串源；持久粗转与声纹延期可恢复；输入、消息、缓存有界；模型故障/截止时间/父进程退出能回收；压力调节不主动暂停采集或删除历史人声。
- Non goals: 新 UI、新模型、阈值调优、数据库服务化、定时重启核心、硬限制所有 OS 瞬时内存；不能把公开短样本/当前硬件模拟当成 24h/72h、macOS13/8GiB、真实标注质量通过。
- Owned defects: 本批引入/显著放大的任务、重放、保留、清理、资源准入、模型生命周期和声纹计算问题，以及上述 F 项。
- Approved mechanisms: VAD/粗转角色拆分；core DB coarse_jobs 持久任务引用已有媒体；统一有截止时间的 model RPC；单一资源 governor（OS 信号/footprint/积压/交互延迟/采集推进，峰值预留、迟滞、计算边界让位、空闲卸载）；声纹持久进程、瞬态有限重试和有界匹配缓存；沿用 cleanup/version/generation 判定，不增加业务 owner。
- Required lenses: fresh requirements + adversarial 固定快照；裁决修复与相关验证后 fresh architecture 审完整最终候选；PASS 才提交。
- Evidence: 166 core tests passed / 1 ignored scale；cargo check core/worker；8 组公开固定输入分段/时间/文字与基线相同；双源累计 335.52 秒、68 块、60 coarse jobs 的实际 release core DB/pipeline，VAD 强杀恢复输出等同不中断；10 次复用声纹向量与基线完全相同；30 轮最大输入的模型内存观测。详见 batch-c-quality.json。最终签名构建/CLI smoke/原生短链进行中，提交前补齐；质量/流水线实验在最后 minor fairness/index 修复前，影响的资源 tests 已复验，最终 release 重跑流水线。
- Applicability: task/schema/cleanup/retention 变化重跑对应 Rust；worker/model/resample 变化重跑固定输入和内存；governor/RPC 变化重跑压力/截止/释放测试；source progress 变化最终签名 native 短链；前端未动。

## Findings and adjudication

- Requirements 初审 PASS；adversarial 两项 P1 blocking，主 Agent 复现并确认，已根因修复，等待定向复核。
- A1/F07：连续有声超过20秒硬分段时，重放输出含未确认尾部却被整体丢弃，能漏10秒粗转任务；旧粗转没有新 job 水位还会重复前缀。重放结果现同样经过 core 的 coarse::enqueue 事务，复用已确认前缀裁剪和清理墓碑，不改写已完成 VAD 判定。旧版 coarse_segments 的已发布毫秒端点作为兼容水位，按原样本/真实时间映射回既有媒体；增加 session/end 索引。
- A2/F06：无法满足预算的大模型等待权阻挡可执行小模型。让位资格现同时核对释放当前空闲驻留后可满足另一模型峰值预算；不能用不可执行的任务饿死可执行任务。活动推理不被普通公平调度打断。
- 受影响验证：新增旧版/新任务重放尾部去重、不可准入等待者与可卸载驻留回归；168 core tests passed /1 ignored、cargo check/build 通过。连续有声与双源 release 故障恢复复验进行中；旧带停顿公开样本不能覆盖此次发现，新增证据单独保留。

- 定向 adversarial PASS（21 项独立 targeted tests）；requirements 发现旧水位非3整除样本起点的反向映射提前1ms，已改为向上取整起点，新增样本100000 → 确认81334 → 正向时间5000ms往返回归，coarse 8/8 PASS。该修复不改变3整除输入，先前连续音频证据继续适用。

- 最终 requirements 定向 PASS；adversarial 定向 PASS；fresh final architecture 对完整最终候选 PASS。最终170 core tests passed /1 scale ignored、cargo check/build、隔离CLI四组、签名包与deep/strict验证通过。最后生产候选 SCK/Tap 各双源8秒0 dropped（最终binary SHA见quality JSON）。本批提交门禁完成；D/最终组合态仍未完成。
