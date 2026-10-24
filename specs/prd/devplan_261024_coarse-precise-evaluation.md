---
status: in-progress
created: 2026-10-24
updated: 2026-10-24
---
# 粗转/精转三次配对实验

需求源：用户要求固定模型high、同prompt、同批12真实case，每组各三遍，量化+case分析。完整方法在specs/research/2026-10-24-coarse-precise-evaluation/PLAN.md。此前“调到稳定”的门槛不作为本实验结束条件；本次须交付完整72次实验及可信统计，可以结论不通过。

## 开发契约
- 仅测试/离线工具：复用real_understanding_replay、cloud::chat、llm_input、publish_fit_with_input、asr::input/assemble/map及同ASR供应商。
- 新增测试专用固定窗口fixture入口；必要时补c07缺失精转，源音频只读、独立ASR请求、结果写private；不回写活跃库。精转适配只在隔离fixture，不改变生产事实源。
- 生产架构/协议/默认模型/界面不变，不增重试/验证模型调用。keychain凭据留Core。
- 拥有本次测试入口增量、新paired脚本、新研究目录；前轮尚未提交的工具与cloud cfg(test)是共享基础，审查需兼容但不冒认本次新增。9个原有version/UI改动与其它研究排除。
- 基线729e1ba6fc737416e9ef091e9a9bf6dc1f337190；修改前保存拥有文件快照，以区分上一轮增量。

## 验收与结束条件
- 12×2×3完整执行（失败原样计入，不擦除），固定prompt/high/窗口与source hash；完整quant+case报告可复核。
- 语义评分逐事实/逐case带依据，明确不确定项和缺失；不要求实验产品质量全通过。
- Rust变更定向测试、check/build；Python自检及源数据覆盖/重复/引用与窗口一致核验；requirements/adversarial与最终architecture完成。

## 执行台账
- [x] 冻结样本两种来源/标尺/窗口，补齐缺失精转。
- [x] 测试入口和runner自验证，运行72次。
- [x] 语义逐例评分、统计、案例与限制。
- [x] 独立审查、报告、仅拥有变更提交。

当前待用户决策：无。用户已明确不换模型，统一high，三遍对照。

2026-10-24：前置requirements/adversarial完成，c03条件gold澄清、缺失统计分母修复且独立故障注入通过；最终工具architecture PASS。329 tests/check/build通过。72次正式回放已启动，不重试、不修改输入或prompt。

2026-10-24：72/72完整回放、126/126窗口调用返回；64/72严格结构通过，评分与输出全部保留。最终 requirements/adversarial 定向审查及独立统计重算 PASS，最终工具 architecture PASS。REPORT/metrics/私有逐例输出已完成，9个无关文件基线hash未变；Python编译与拥有diff检查通过。研究验收完成，当前候选prompt未稳定通过，不切换生产输入/模型。待本批提交后记录hash并关闭台账。
