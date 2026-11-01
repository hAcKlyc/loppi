# Batch 1 Review Contract

1. Mode: repair；reviewers 只读、不改文件、不派遣。主 Agent 裁决并修复。
2. Requirement: specs/prd/devplan_261022_design-system.md 的 Batch 1；specs/DESIGN.md F1–F8 与 SpeakerPicker 非模态规则。Batch 2 审美尚未开始，不以其未实现阻塞本批。
3. Target: 当前工作树。Exact base: 0666e02。源码在 <USER_HOME>/Projects/hear，分支 codex/design-system-polish。
4. In scope: batch1-files.json 列出的 tracked + untracked 文件，及本 Review Contract / manifest。DESIGN 与旧审阅证据是同对话上一轮产出。
5. Out of scope: 其它未跟踪的 playground、素材 ZIP、旧 devplan / drafts / research；不得审成新增回归。
6. Required guarantees: 760×540 可达；模态背景隔离、初始/循环/返回焦点、最上层 Escape、写入 busy、嵌套；非模态可离开；Tabs 手动激活/ARIA；功能文字与 token 达标。保持录音/播放/分页/修订/未知请求 ID/密钥及 compact 首次设置语义。
7. Owned defects: 本批引入或放大，或 F1–F8 明确要求；未变业务算法与全应用历史问题不阻塞。
8. Allowed architecture: 展示原语、局部焦点/反馈/当前视图的预览 generation。无新业务 owner/store/protocol/retry/poll/theme；只读取消在原 SettingsView 内淘汰晚到结果。
9. Required lenses: requirements、adversarial、architecture，fresh context。前两路固定快照，主 Agent 修复验证后最终 architecture。
10. Evidence: specs/research/2026-10-22-design-system-implementation.md 与当前 evidence 文件。10 组浏览器、51 逻辑测试、lint/typecheck/build、签名 Tauri 1120/760 运行检查已通过。修改共享原语需重跑相关交互；CSS/token 改动只重验相关视觉/对比度；未改业务/Rust不扩大测试范围。全应用 VoiceOver、其它 OS/DPR 与安装发行认证不在本批承诺。

请按对应 skill 报告可达的 in-scope blocker / required evidence gap，给具体文件行与触发条件；与已有基线区分。主 Agent 会裁决，不让建议或未知假设自动升级保证等级。
