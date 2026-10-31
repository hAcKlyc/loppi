# Review contract
Mode repair；base 77269a73e93483c7f96427eb213faef5cd6efa55。
需求与验收：specs/prd/devplan_261024_fit-publication-summary.md；最新用户明确修重复字段/非法时间静默丢失、摘要跨窗遗忘，不切精转，固定模型high同12例各三遍。历史partial-fit对非法时间尽力保存的规则已被本轮明确覆盖，其它字段质量不扩大为全schema强拒绝。

In-scope：crates/loppi-core/src/cloud.rs、events.rs、events/summary.rs、events/fit_prompt.txt、events/understanding_tests.rs、events/understanding_replay_tests.rs、events/activity_tests.rs；scripts/evaluate_event_prompt.py、scripts/compare_transcript_inputs.py；specs/ARCHITECTURE.md、tech_docs/algorithms.md、上述devplan及本研究目录公共文档。实现是固定快照；正式36次回放和报告随后补足，仅私有实验文件可生成。
Out-of-scope：9个先存版本/UI改动、其它研究/设计untracked、活跃库与安装包；此前全部模型语义问题不升级成当前全零错误保证。

必须保证：原始content递归查重复键后才Value；非法时间在事务写入前整窗拒绝，不complete，不丢旧事件；复用既有有限重试。摘要句子增量默认保留、显式修订有本窗证据、错误修订不删旧句；新句可引用当前/本目标旧要点的证据；字符串列仍唯一owner，句号编号仅派生；null历史、迟到、人工修改/清理保护保持。单窗口单调用，不切模型/精转、不加样本特例。

允许机制：修改现有私有LLM契约（summary数组/summary_sentences），无新数据库状态/队列/owner/第二调用/恢复协议。旧整段string只允许在整个活动属于当前窗时更新；窗外历史保持旧值。生产low/60不改；实验high/120与既有基线可比。

证据：Core333 tests/15ignored已通过；126真实旧响应离线解析/时间核验进行；workspace check/build随后；36回放不隐式重试、结构失败/安全拒绝与完整产出分开统计；最终逐例评分及report需定向复核。不要为未完成的预定实测阶段制造代码机制，标记证据待补即可。
Required lenses：requirements/adversarial fresh-context只读并行，裁决修复后fresh architecture；不改文件、不spawn其它reviewer、不发付费调用。仅本轮引入/显著暴露及用户明确要求的根因可阻塞。
