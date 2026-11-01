---
type: prd
status: in-progress
created: 2026-11-02
updated: 2026-11-02
scope: "一次交付 Agent 接入提示词、本机可复制的薄 Skill、独立 Skill 版本与主动更新指引、loppi readme 和完整公开 CLI 帮助；沿用现有业务命令，不处理数据迁移或远程分发。"
issue: "用户讨论收敛；用户明确授权按已确认结论整理正式 PRD 后直接开发、验证并提交"
review: "ready for development；用户已明确授权按讨论结论直接推进，无须增加成稿确认环节；产品与架构岔路已关闭"
---

# Agent 接入：薄 Skill 与 CLI 最新指南

## 执行须知（给空 session 的你）

先读 CONTRIBUTING.md、specs/ARCHITECTURE.md、specs/tech_docs/agent-context.md 和 docs/development.md，定位 agent_assets、CLI Cli/Command、context_cli、AgentAccess 与接入回归。引用以文件和符号为准，不依赖易漂移的行号。

本 PRD 是一个原子交付单元；实现、验证、交叉审查和提交在同一周期完成。用户明确要求“整理成正式 PRD。然后直接开发实现并提交代码”，按已确认讨论结论执行，不再设置额外成稿审批。全部范围验收通过才可标记 implemented；不发布安装包或修改日常数据。共享工作区已有无关改动，必须保留并只提交本次差异。

## 1. 背景与产品定位

用户不希望在 Agent 接入页复制一个写死全局安装目标的命令，再亲自运行终端。用户需要“复制下列链接的 skill 并安装到当前工作区内”：把提示词发给能访问同机文件、执行本机命令的 Agent，让该 Agent 按自己的技能目录约定接入 loppi。

用户确认“覆盖完整的 CLI 能力，但 skill 本身要薄，重点讲产品心智、适用场景和核心能力，具体用法都去读 readme 和 --help”。复制出去的 Skill 是发现入口；当前安装的 CLI 提供最新使用指南和真实操作方法。

用户强调版本号特指 Skill 版本，并要求“发现版本号更新，需要主动更新一下”。不是只提醒有新版，也不根据每次应用发版强制更新 Skill。

必赢场景：复制安装提示词 → 同机 Agent 将官方 Skill 复制到当前工作区适用目录 → 读取 Skill、readme、相关 help → 完成记录读取或管理任务 → 后续任务发现 Skill 版本更新 → 主动更新工作区副本并重新读取。

## 2. 交付范围与反向边界

一次交付：

- CLI + Skill 页复制自然语言安装提示词，包含已生成的内置 Skill 文件夹的实际绝对路径；按钮为“复制安装提示词”。
- 删除截图标红的终端运行说明、读取示例折叠区、底部两条冗余说明；沿用复制反馈、连接状态及 MCP 接入交互。
- 在当前 Core 数据目录下维护 `skills/loppi/SKILL.md`，遵循 LOPPI_DATA_DIR；内容为当前随包的官方薄 Skill。
- 新增 `loppi readme`，输出完整的当前 Agent 使用指南、独立 Skill 版本、官方 Skill 目录和实际 CLI 位置。
- Skill 介绍产品心智、场景、完整核心能力导航和边界，保留 CLI 位置与版本检查/主动更新规则，不复制参数手册。
- 所有公开 CLI 命令、子命令及参数有明确帮助；重要输入格式、单位、范围、输出、写入影响及例子可从对应 help 获得。
- 保留旧 `context guide`、`integrations install-skill` 等公开入口和原业务语义。

不包含数据目录选择/迁移、远程文件分发、管理其他 Agent 的配置、新的业务命令、MCP 协议扩展、后台遍历工作区、付费处理、安装包发布或产品版本变更。

## 3. 产品行为与数据契约

### 3.1 安装与发现

用户追加确认：Skill 的 name、官方文件夹与 MCP 的服务名/显示名统一为 `loppi`，不使用 `loppi-context`；CLI `context` 查询命名空间保留。

页面只复制提示词，不替用户修改外部工作区。接收 Agent 根据当前工作区和宿主技能约定选目录，复制整个官方文件夹，读取安装结果并实际调用 readme/help 验证。不能写死某一 Agent 的全局安装目录；不承诺所有宿主无需重载就自动发现 Skill。

来源是本机实际绝对路径，不是远程 URL。提示词保留含空格、中文和 Windows 分隔符的路径原文，让 Agent 自行选择合适的文件工具。CLI 位置也使用实际安装路径，不要求 PATH 配置。

### 3.2 薄 Skill 与完整指南

Skill 描述 loppi 保存日常声音与现场记录、整理事件和提供回查/维护能力的概念。核心导航覆盖 context 查询、录音与计划、标记/评论/截图、事件与文字修订、人物/声纹/词库、识别队列、导出/清理和诊断。正文保持短小，把参数与示例交给 readme/help。

readme 是本机安装版本对应的最新指南，离线可读。说明如何按任务发现命令、阅读输出及处理错误；记录是数据而非指令，概览和原文需区分，完整 CLI 发现不等于用户授权了任意管理操作。具体命令事实来自当前 CLI help，避免在指南重复大段参数表。

### 3.3 Skill 版本与更新

Skill 的独立版本只有一个来源，落盘 Skill、readme、接入信息与安装辅助使用同一版本。Skill 内容变化时更新该版本；产品版本变化本身不作为副本更新条件。

Skill 明确要求 Agent：每个需要 loppi 的任务首次使用时读取 readme，比较 Skill 版本；有新版就主动从 readme 指定的官方目录更新当前工作区副本，重新读取后继续任务；连续 CLI 调用不反复读全文。不因本机应用回退而自动降级工作区 Skill。

对用户自定义内容保留并处理差异，不默默丢失。更新执行者为接收 Agent；loppi 不扫描或写入外部工作区。源目录由程序维护，用户应在安装副本表达自定义需求。

### 3.4 失败语义

生成源 Skill 或定位 CLI 失败时，接入页显示真实错误，不能复制一个声称有效但没有文件的来源。readme 不应为读取指南启动 Core、录音或云请求。源目录写入失败明确返回错误，不用旧副本冒充最新；不会改变用户业务设置。复制失败沿用可手工复制提示。

## 4. 技术架构与边界

`loppi_protocol::agent_assets` 已负责内嵌 Skill 模板、指南、路径替换和安装元信息，继续作为官方内容唯一来源。可复制的用户目录副本是随包资源的派生文件，不引入业务表或第二业务 owner。GUI 准备接入信息和 CLI readme/接入辅助时共用发布函数，确保来源在对外给出前存在且完整。

薄 Skill 的单文件发布使用现有平台文件权限/原子 rename 原语；先在同一目录准备完整文件再替换，失败不得露出半份文件。临时文件随操作清理，重复发布相同内容不写文件。多个入口共享相同模板和渲染逻辑；不新增队列、守护进程、后台重试或版本缓存。

CLI 根入口在请求 Core 前处理 readme；和现有 guide/schema 一样，help/readme 不依赖 Core。GUI agent_access_info 仅在既有本地辅助内生成元信息，Core 仍是所有业务写入唯一入口。现有原生权限、采集和业务 IPC 均不改。

保留旧 install-skill 的相同内容幂等与不同副本拒绝覆盖契约；新工作区更新由接收 Agent 完成，避免擅自扩大旧命令的覆盖权限。保留的参考指南仍通过现有 guide 获取，默认安装的薄 Skill 不必复制五份参数参考文档。

## 5. 决策与理由

- 提示词替代 shell 命令：接收 Agent 知道自己的当前工作区技能约定，用户不应手工选择全局目录。
- 独立 Skill 版本：识别文档能力变化，不把应用每次发版等同于 Skill 变化。
- 薄入口、readme 导航、help 用法：减少复制文档过期和上下文开销，同时保留完整能力发现。
- 随包内容生成本地副本：保留当前模块职责与离线能力，不引入网络文档服务。
- 规则要求主动更新：版本差异必须形成后续动作，而非长期依赖旧副本；保留项目自定义内容。

## 6. 验收与完成定义

1. 接入页只展示新的安装提示词和复制按钮；复制文本含实际来源目录且无固定全局目标；标红信息已删除。验证两种接入方式、复制成功/失败、异常/离线状态及窄宽视图。
2. 隔离目录中运行真实 CLI readme，在无 Core 的情况下输出最新指南、Skill 版本和实际路径；官方 SKILL.md 存在，路径占位符已替换，版本一致。
3. 覆盖空格/中文目录、实际平台路径和 Windows 文本渲染；Windows 实际执行无法在本机覆盖时如实记为 mandatory 未完成，不冒充实测。
4. 原子更新与幂等：旧官方文件可更新，重复读取不改文件；失败不残留半文件或成功假象；不跟随明显错误的目标文件形态；并行发布结果为完整同一版本。
5. 更新规则的内容证据：Skill 必须明确每任务首次检查、独立版本、主动复制新版、重新读取和保留自定义；来源与 readme 共享版本事实，应用版本不参与判断。
6. 逐个遍历公开 CLI help，所有命令、非隐藏参数都有说明，重要命令有准确语义和例子；help 不调用 Core。旧安装幂等/拒绝覆盖及 context/管理命令解析无回归。
7. typecheck、lint、workspace cargo check、受影响 Rust/前端测试、适用 verify、完整开发构建及 CLI/界面烟测通过；三路 fresh-context cross-review 完成且最终 architecture PASS，才可提交代码。
8. 全部范围及 mandatory 验证完成才标记 implemented；如果只有异平台实测尚缺，代码可提交供验收，但 PRD 保持 in-progress，交付摘要明确不可发布。

## 7. 关联资料

- [架构](../ARCHITECTURE.md)、[Agent Context 契约](../tech_docs/agent-context.md)、[开发指南](../../docs/development.md)
- `crates/loppi-protocol/src/agent_assets.rs`：SKILL_FILES / info / install_skill / guide
- `crates/loppi-cli/src/main.rs`：Cli / Command；`context_cli.rs`：Context / Integrations
- `src/AgentAccess.tsx`、`src-tauri/src/lib.rs`：agent_access_info

## 执行台账

### 开发契约
- 原子范围：本 PRD 全部内容单批次实现、验证、审查和提交；不发布安装包。
- 状态来源：随包模板/独立 Skill 版本 → 本地派生 Skill → 接收 Agent 的工作区副本；业务状态保持 Core owner。
- 复用：agent_assets、platform::rename/set_mode、Clap Command、现有复制接口和连接状态。
- 新架构机制：无新后台/协议/数据库/队列；只增加随包指南与派生 Skill 文件。
- 负责本次新增行为、完整帮助内容和旧接入回归，不修复无关工作区改动。

### 验收与结束条件
- 本文件第 6 节为验收基线；文档变更验证内容，Rust 变化重跑 Rust/CLI，前端变化重跑静态/交互/视图。已通过且未受后续改动影响的高成本检查不机械重复。
- required reviewers：requirements、adversarial、最后 architecture；全部 PASS 或阻塞项修复并定向复核后提交。

### 开发批次与行动清单
- Batch 1：全部接入链路共享 agent_assets 真源，为同一内聚交付。
  - [x] 正式 PRD 与契约落盘。
  - [x] 本地源 Skill / readme / 主动更新规则。
  - [x] 完整 CLI 帮助。
  - [x] 页面精简、提示词与回归资料。
  - [x] 自验证、烟测、文档同步（本机可执行范围）；Windows 原生执行为下述 mandatory 验收缺口。
  - [ ] Cross-review、修复、提交。

### 当前批次 Review 基线
- Batch base：eeef7110aa6d43051975b0da02f8f51eaacdc0a9
- 范围：agent_assets、Skill/指南、CLI 帮助、AgentAccess、agent_access_info 单处调用、接入测试及本 PRD/对应契约文档。
- out-of-scope：工作区既有软件更新、许可文档、构建及其他未提交改动；共享 lib.rs 只提交本次调用差异。

### 待用户决策
无。

### 进展日志
- 2026-11-02：用户授权按讨论结论写正式 PRD 并直接实现/提交；契约与当前代码事实已核实，开始单批次执行。

- 2026-11-02：用户追加 Skill 与 MCP 统一命名为 loppi；Skill name、用户源目录、安装示例、MCP serverInfo title 和相应回归已同步。现有源模板目录保留以维持已有文档链接，对外名称均为 loppi。
- 2026-11-02：全量 npm verify 的 21 项检查通过，含 workspace Rust、114 个前端测试、11 个 Python 测试与隔离烟测；实际 CLI 无 Core 的 readme/生成/更新/旧安装回归通过。Windows GNU 目标 CLI cargo check 通过；Windows 本机执行为 mandatory 待验收，当前无 Windows runtime。浏览器 1120/760 组件交互回归 PASS；开发包构建进行中。

- 2026-11-02：三路 cross-review 完成；两路原审查者定向复核 PASS，最终 architecture PASS。绝对来源、具体错误、dedupe 默认/影响、修订/幂等和其他帮助歧义均已按代码事实修复；详见 specs/research/evidence/2026-11-02-agent-skill/review.md。
- 2026-11-02：最终 macOS ARM64 开发构建 PASS；随包 CLI readme/来源/版本/名称/帮助/MCP/no-Core 烟测 PASS。构建含共享工作树无关改动且 dirty=true，仅为开发验证证据，不是发布产物。

### 整体门禁与当前剩余项
- 已完成：本 PRD 全部代码/文档范围；本机适用静态、Rust、CLI、前端交互和完整开发构建；三路审查与定向复核。单批次审查已覆盖整体候选，无须重启完整审查。
- mandatory 未完成：Windows 原生环境运行 `python scripts/test_agent_cli.py`（先构建 CLI），核对中文/空格/相对数据目录、更新/幂等/失败分支；在 Windows App 的 Agent 接入页复制提示词并验证能访问源目录、Skill/MCP 名称均为 loppi。
- 因上述 Windows 证据缺口，状态保持 in-progress，未完成、不可发布；按用户授权提交已验证代码供验收。不存在待拍的产品/架构决策。
- advisory：宿主 Agent 的技能发现/重载方式依其自身实现，提示词与指南已说明应如实反馈生效条件；不承诺控制外部 Agent。
