---
status: implemented
updated: 2026-10-21
---
# 粗转就绪、三分钟已知静默连接与并行活动续接

> 后续变更（2026-10-22）：本文保留当时的交付契约与测试事实。现行文字发布已删除原声音频覆盖/纯静默拒绝规则，按用户确认优先保留完整文字；当前行为见 [精转发布修复计划](devplan_261022_asr-publication.md)与 [算法文档](../tech_docs/algorithms.md)。

用户批准上一轮建议 1、2、3：粗转完整后整理；减少精转物理碎片；独立并行活动跨窗口各自续接。用户建议三分钟作为静默切块阈值。本次将其落实为同一精转范围内不超过 180 秒的已确认无人声空档可以连接，并保留等长静默；无文字不等于无人声，不改 VAD 或原声留存规则。

## 执行台账

### 开发契约
- 必赢场景：成熟小时候选等待其已有本地待办完成，晚到粗转汇合后再调用 LLM；两路独立两小时活动都保持原 ID；20 秒至 3 分钟有完整无人声证据的空档不拆精转请求，未知、清理或采集缺口仍拆。
- 原子交付：三个策略改变、兼容、测试和文档一次完成；沿用 Core 事件与 ASR owner。
- 必须保证：终态失败粗转不无限阻塞 LLM；手动整理保留主动强制入口；新到证据仍可修订，不许自动重跑不变历史。音频时间不压缩、纯静默不接受虚构文字、人工边界/清理/版本优先、旧 ASR 输入和远端 ID 不改写。
- 复用：candidate_windows、coarse_jobs、audio_blocks、take_task_fair、fit_context、event_candidate_links、asr::plan/Input、media_gaps、clear_ranges。
- 允许机制：有界前序事件数组替代单一前序上下文；既有本地待办就绪查询；新 ASR planner 策略版本。无新表、服务、队列、持久调度状态。
- 不做：最终摘要、会议前缀精转、资源 governor 重构、粗转声纹、自动历史重整、VAD 留存调整。SSD 300 不随切块同时更换；其准确率对照属于后续实验。
- 费用与边界：填充静默计入最多 20 分钟的请求长度及计量，不宣称减少请求等于减少费用。原有严格词时间/音频覆盖校验保留；缺有效词时间的异常长句仍可能失败。
- 安装与运行：本次开发不自动重启录制或替换安装版，用户此前的打包运行已完成。

### 验收与结束条件
- mandatory：core check/build、受影响事件/粗转/ASR/清理回归；模拟晚到粗转验证调用任务减少与终态失败出口；3 分钟阈值两侧、来源隔离、完整/部分静默覆盖、捕获/清理缺口、真实 Ogg 时长和映射、旧输入不变；并行续接/单路结束或换活动/人工锁与非法 ID。
- mandatory：合成文本调用当前配置模型验证双源跨小时独立续接、双源同场会议、活动转换；仅合成材料和隔离内存库，禁止发送真实转录、写生产库或改变生产队列。
- required reviews：requirements + adversarial，修复后 architecture，遵循 cross-review-code。
- advisory：真实语音字符错误率/说话人错误率、长期能耗与费用，不能由合成文本或请求数推断提升。
- 结束：单批次修复与检查、三路审查通过，作用域提交及最终静态门禁后标 implemented。不为观察项扩张范围。

### 开发批次与行动清单
- Batch 1：既有事件处理链路的就绪与上下文完整性
  - [x] 就绪调度与晚到粗转回归
  - [x] 已知静默连接与物理输入/时间映射回归
  - [x] 多前序续接与真实合成模型验证
  - [x] 文档、check/build、回归、三路审查和提交

### 当前批次 Review 基线
- Base：fb8c35de043f1e59a05e0028537ce5e1d73d9f1c
- 范围：events.rs、events/asr.rs 与相关测试、必要的 coarse/main 调度、algorithms.md、本计划。
- 共享工作树其他全部修改不属于本任务，不覆盖、不提交。

### 待用户决策
无。三分钟是已确认静默连接阈值，未知或采集缺口不扩阈值。

### 进展日志
- 2026-10-21：核实原 3 秒常量同时用于切块、供应商分句与发布覆盖检查；不能整体扩大。新连接上限单独放在 ASR planner，原分句和虚构文字校验继续保留。
- 2026-10-21：实现成熟候选待本地工作齐备后再创建 LLM 任务；候选自身保留 pending 阻止相邻后继抢跑。包含未进入 observations 的未分析窗尾，终态失败不等待，显式立即处理可强制。已有自动 pending 任务在新增本地工作时暂缓领取；粗转有成熟候选等待时增加最老份额，仍保留实时份额与双来源交替。
- 2026-10-21：新 ASR 策略 `volc-event-silence-v3` 连接完整同源已知静默覆盖、不超过 180 秒的空档，按真实长度编码；未知/部分/异源/采集/清理空档继续分开。保留 10ms 时钟容差、20 分钟/10MB 上限、旧输入/远端请求。实际编码 190 秒音频（两端各 5 秒，中间 180 秒）并逐帧解码验证时长、静默、后段映射与虚构文字拒绝。
- 2026-10-21：`previous_events` 按 source_mask 各取一个最新前序，最多三项，每项尾部继续有界。允许并行活动分别续接；重复使用同一旧 ID 拒绝；人工锁仍保护原边界。更新模型输入/prompt 与合成场景，`deepseek-flash` 10 场景 × 2 次 = 20/20 PASS，32 次调用、95,169 reported tokens；新增两小时双源独立活动两次均保持两个完整 ID。全程只发送合成文本，生产库仅读配置。
- 2026-10-21：core 全量回归 299 PASS / 8 opt-in ignored，check/build PASS。后续仅补第三窗口单路结束的定向回归，不改变生产或 prompt。无实际语音对照，不宣称文字/说话人准确率提升；请求数减少与费用变化应在真实使用中观察。

- 2026-10-21：requirements/adversarial 均发现同一 P1：无云任务的等待候选在积压超过两小时、最后本地失败且无新证据时，可能滑出扫描范围。裁决为本次引入、违反失败出口保证的阻塞项。复用 candidate_windows 将 pending 且没有当前版本云任务的最早起点纳入扫描；不新增 owner/状态。新增四小时积压后粗转失败、空结果完成、显式强制及 VAD 尾部失败回归，失败场景修前红、修后绿；93 项事件回归通过，2 项 opt-in 忽略。该修复不改变 prompt/模型输入语义，既有合成模型证据保持有效。

- 2026-10-21：修复后 check/build PASS；requirements/adversarial 定向复核均 PASS，原 P1 关闭；全新 architecture reviewer 审查最终完整差异 PASS，无剩余阻塞或 mandatory 证据缺口。最终 scoped diff whitespace check PASS。

- 2026-10-21：实现提交 `a9a046d44b1dfe927e2995f2bd53ccc4dfc973a6`；重新逐条核对整份契约与非目标，最终 HEAD `cargo check -p hear-core` PASS。单批次完整审查已覆盖最终代码，无临时机制或未决 mandatory 项。共享工作树其它修改保留。安装版未替换、录制未重启；真实语音质量、能耗与费用继续作为 advisory 观察，不冒称已验证提升。

### Review Contract
- mode: repair；required lenses: requirements、adversarial、architecture。
- base: `fb8c35de043f1e59a05e0028537ce5e1d73d9f1c`；requirement source: 本计划与本轮用户批准的 1/2/3，三分钟仅用于已知静默连接。
- in-scope: `crates/loppi-core/src/events.rs`、`crates/loppi-core/src/events/activity_tests.rs`、`crates/loppi-core/src/events/asr.rs`、`crates/loppi-core/src/events/asr/tests.rs`、`crates/loppi-core/src/main.rs`、`specs/tech_docs/algorithms.md`、本计划（含 untracked）。
- out-of-scope: 其它共享工作树文件，包括 README、发布脚本与其它文档。不得覆盖或提交。
- 必须保证、非目标、允许机制：上文契约；不增加表/服务/队列/持久调度状态。阻塞项限本次引入/放大或验收明确负责的问题，不扩大到全量识别质量、已发布的无关历史问题。
- evidence: 上述 check/build、299 core 回归、20/20 合成模型、实际 Ogg 编解码。prompt/输入语义变更重跑相应模型场景；候选/规划变更重跑受影响回归和 check/build；文档/测试修订仅重跑相应检查。
