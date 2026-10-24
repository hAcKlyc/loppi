# Review Contract：粗精转三次配对实验

Mode repair；baseline729e1ba6fc737416e9ef091e9a9bf6dc1f337190。本次具体增量以private/baseline-understanding_replay_tests.rs等工作树快照比对，上一轮cfg(test)profile和auditor基础已审查通过。

需求/验收：PLAN.md和devplan_261024_coarse-precise-evaluation.md；固定v15原文/deepseek-flash/high，12×2×3独立回放，固定窗口，量化和case分析，不要求实验模型全部通过。来源原文不修改，隐私只private，全部失败保留；语义评分须有依据且不把精转当人工真值。

范围：understanding_replay_tests.rs新增frozen_windows与ASRrepair测试入口；scripts/compare_transcript_inputs.py及必要测试；本实验目录与devplan；前轮evaluate_event_prompt.py只能必要兼容修复，单列增量。生产模块不新增行为。

排除：9个原version/UI变更，全部其它未跟踪文件；之前prompt优化是否稳定不是本次发布门槛。

允许机制：隔离测试fixture将精转表形状适配为coarse输入，保留原ID/文本/时间及来源映射；只测试固定窗口种入，复用insert_task与生产输入/发布；一个ASR修复请求、已有assemble/map/cloud协议、私有输出，不回写live。无新生产store/owner/协议/重试/第二模型调用。

Lenses：requirements检查设计/语义指标与前置gold，后续定向核对评分；adversarial检查来源匹配、去重、窗口一致、结果完整、统计分母；最终architecture检查最终工具复用和范围。

验证：Python源绑定/原始响应/SQL审计和故障注入；Rustcheck/build/tests；72次请求结果（失败计入）；逐case评分与聚合相互可查。prompt/源/窗口变化需整组重新测试，不为通过挑结果；统计修复只重算。语义评价由reviewer Agent逐条审阅、主评审裁决争议，不冒称人类标注/原声真值。
