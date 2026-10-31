# Review Contract
mode repair；base e9463dc04504a33731b4d47c0343c942b514a12c。
需求：specs/prd/devplan_261024_summary-budget.md，用户已接受1000字左右有预算增量编辑，每窗一次调用、数据不截断；同12例high三遍+跨窗纠正实测。
In-scope：crates/hear-core/src/events.rs、events/summary.rs、events/fit_prompt.txt、events/understanding_tests.rs；scripts/evaluate_event_prompt.py；specs/ARCHITECTURE.md、tech_docs/algorithms.md、上述devplan及本research公共资料。
Out：9个既有版本/UI修改、所有其它研究untracked、活跃库和安装包；既有模型语义不保证全零错误。
保证：update原证据规则不变；compress非空旧句替换、确实缩短、只用目标旧理解证据，不要求当前证据；未知mode/坏引用/下标/重复消费不删旧句。无mode旧提案按update。旧摘要与keypoints仍唯一持久源，无新表/IPC/队列/额外调用/重试。长度请求内派生，最终字符串不截断，超长保存但实测标超标。压缩语义无损由内容实测核验，代码只验证结构/引用，不以字符串字符数推定语义。
允许：私有LLM输出增加mode与输入预算/长度；复用现有pure合并、事务、迟到及scope guard。生产low/60不改，实验high/120。不得因未完成实测阶段额外增加机制。
验收：335 Core passed/15ignored；workspacecheck/build随后；结构审计新旧兼容、36真实63窗与6合成18窗无隐式重试，全部分母/失败保留；预注册新定位锚点并继续报告旧46项覆盖。真实目标每个summary≤1100且关键细节/纠正有效；预定证据后补，不能以测试代替模型表现。
Required lenses：fresh requirements+adversarial只读并行，修复后fresh architecture。不改文件、不spawn、不付费。只本轮引入/显著暴露/用户明确拥有的根因可阻塞。
