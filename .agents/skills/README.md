# 开发工作流技能

源自 MyAgents 的开发工作流文档，已由版权持有人 Ethan L 授权按 Apache-2.0 分发，并适配 loppi 的构建、验证、文档与进程边界。它们独立纳入本仓库，不依赖安装 MyAgents，也不随来源仓库自动更新。

| 技能 | 职责 |
|---|---|
| [prd-discuss](prd-discuss/SKILL.md) | 讨论需求、维护决策台账、收敛产品与架构边界 |
| [prd-writer](prd-writer/SKILL.md) | 核实事实并撰写 PRD |
| [start-dev](start-dev/SKILL.md) | 从就绪需求到实现、验证、审查和提交 |
| [cross-review-code](cross-review-code/SKILL.md) | 协调三个审查视角并裁决修复 |
| [review-requirements](review-requirements/SKILL.md) | 需求符合性与验收证据审查 |
| [review-adversarial](review-adversarial/SKILL.md) | 故障路径、并发、资源与生命周期审查 |
| [review-architecture](review-architecture/SKILL.md) | 最终架构、职责归属与复杂度审查 |
| [merge-release](merge-release/SKILL.md) | dev 验收后合入 main，由 Actions 自动构建三个平台、签名并发布完整更新资源 |

调用关系：`prd-discuss → prd-writer → start-dev → cross-review-code → dev 验收 → merge-release`；审查协调器先执行 requirements / adversarial，再执行 architecture；正式发行由 main 上的 Release Actions 执行。

构建与测试的实际入口见 [CONTRIBUTING.md](../../CONTRIBUTING.md)和[开发指南](../../docs/development.md)。`.claude/skills` 是指向本目录的仓库内相对链接。技能属于可选协作工具，不要求每位贡献者使用相同 Agent 或完整流程；项目许可见 [LICENSE](../../LICENSE)。
