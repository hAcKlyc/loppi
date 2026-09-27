# 开发工作流技能

从 MyAgents 仓库的 `.claude/skills/` 原样复制，独立纳入 hear 的版本管理；不是指向原仓库的链接，不随源仓库自动更新。

| 技能 | 职责 |
|---|---|
| [prd-discuss](prd-discuss/SKILL.md) | 讨论需求、维护决策台账、收敛产品与架构边界 |
| [prd-writer](prd-writer/SKILL.md) | 核实事实并撰写 PRD |
| [start-dev](start-dev/SKILL.md) | 从就绪需求到实现、验证、审查和提交 |
| [cross-review-code](cross-review-code/SKILL.md) | 协调三个审查视角并裁决修复 |
| [review-requirements](review-requirements/SKILL.md) | 需求符合性与验收证据审查 |
| [review-adversarial](review-adversarial/SKILL.md) | 故障路径、并发、资源与生命周期审查 |
| [review-architecture](review-architecture/SKILL.md) | 最终架构、职责归属与复杂度审查 |

调用关系：`prd-discuss → prd-writer → start-dev → cross-review-code`；审查协调器先执行 requirements / adversarial，再执行 architecture。

本次仅复制技能，未启动开发或审查流程。正文保留了 MyAgents 的名称、架构文档路径、构建命令和示例；这些不是 hear 已存在的实现或工具链。正式使用前，应按 hear 的实际项目指令与文件核对并适配，不能为满足源项目示例而引入其架构。hear 当前 PRD 仍为 draft，复制技能不改变其就绪状态。
