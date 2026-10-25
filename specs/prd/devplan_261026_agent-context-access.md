---
status: implemented
updated: 2026-10-26
---

# 同机 Agent Context 与接入页面

用户已明确授权「干！」，接受上一轮完整方案并新增左侧 Agent 接入 tab。需求源：[方案](../research/2026-10-26-agent-context-access-design.md)。本轮按已收敛对话直接实施，不另请求批准草案。

## 执行台账

### 开发契约
- 必赢场景：同机 Agent 通过 CLI 或 MCP 找到当天/某话题记录，展开当前有效理解和证据；hear 原采集继续。用户在侧栏的「Agent 接入」中理解用途、复制真实接入配置或安装 CLI skill。
- 原子交付：Core Context Query、六个 MCP 工具、CLI/skill/schema/guide、搜索投影和证据/分页、接入配置/自检、侧栏页面、回归和交叉审查一起交付。现有 CLI/GUI 保持业务语义。
- 必须保证：同 UID、本机 only；新入口不自动启动 Core/采集/处理任务；查询经 Core 读取池；共享 JSON 契约；可见引用/内容层级/缺失状态；分页有界和可续读；清理/修订后不复活旧内容；安装配置使用实际路径。
- UI：沿用灰绿 canvas、白表面与深绿操作；一个主标题、一句用途、少量示例、MCP/CLI 两种接入、真实状态。避免营销标语、卡片嵌套、技术说明墙。支持窄窗和键盘；复制成功/失败真实反馈。
- 不做：网络监听、跨设备同步、语义向量搜索、管理 MCP、自动写入内容、额外问答模型、自动修改当前用户 Agent 配置。可选 MCP Prompts/Resources 不作为读取前提。
- 状态 owner：现有 Core 与 SQLite 唯一事实源；新索引是可重建投影；CLI/MCP 不读库、不持久化内容缓存；UI 仅显示实际接入路径与调用状态。
- 复用：hear_protocol::send_existing、reads::Pool::snapshot、events::{timeline,event_detail,range_detail,timeline_gaps}、knowledge::{search,search_empty_reason,people}、moments::list、storage::migrate、Button/Tabs 与现有导航。
- 新机制：版本化 context IPC/DTO、结构化事件搜索投影、受限引用和分页游标、stdio MCP adapter、随包 skill 与显式安装辅助。均属于已授权方案；无新后台 owner。
- 架构不变量：采集 owner/生命周期不改变；内容有效性由 Core 判断；查询不持有写锁或跨请求 WAL 快照。

### 验收与结束条件
- 静态/构建：TypeScript、lint、Rust workspace check；前端与 CLI/Core/Tauri 构建。
- 行为：隔离数据验证 list/search/event/read/people、粗/精/修正/理解引用、空/缺口、分页长文本、修订/删除、MCP 协商和机器错误、CLI/MCP 结果一致、existing-only 不唤起。
- 集成：实际子进程 stdio 与隔离 Core/CLI；连接配置/skill 安装用临时目录；不修改用户真实 Agent 配置。
- UI：浏览器实际页面截图，常规和 760×540、两接入方式、复制反馈、键盘、离线错误；Tauri 接入路径通过编译/原生路径逻辑实测。既有录音链路用适用离线回归证明未改变，不为本任务启动真实录音。
- 性能：有界查询/输出和超时、并发行为；测量代表性隔离记录的延迟。P95 <1秒是目标，不隐瞒测试规模。
- Review：requirements + adversarial fresh context，裁决修复后 architecture；三路 required。
- 结束：范围完成、受影响测试通过、架构 PASS，按文件显式提交；不把签名发行/替换 /Applications 作为本次请求，产出可构建实现与验收证据。

### 开发批次与行动清单
- 单批次：同一 context 契约及其两种消费入口与 UI。
  - [x] 共享契约、日期、schema、错误、Core 调用
  - [x] Core 查询、搜索投影、证据/有效性、分页预算
  - [x] CLI、MCP、skill 与接入辅助
  - [x] 侧栏 Agent 接入页面
  - [x] 单测、隔离烟测、UI 验证、构建与回归
  - [x] Cross review、修复、文档、提交

### 当前批次 Review 基线
- Batch base：d48e31d（开工时 HEAD）。
- 既有未提交修改：events.rs、events/fit_prompt.txt、events/summary.rs、events/understanding_tests.rs、evaluate_event_prompt.py、ARCHITECTURE.md、algorithms.md、App.css、Controls.tsx 及原有 untracked 文件；不纳入本次提交/评审。
- 本批次文件清单随实际实现补齐。避免编辑上述已脏代码文件，必要的共享文档仅提交本次增量。

### 待用户决策
无。用户已授权完整方案和新增页面，按上述范围实施。

### 进展日志
- 2026-10-26：读取 start-dev/cross-review、设计规范及当前源码；以对话模式建立执行台账。当前仓库未发现 AGENTS.md 或 CLAUDE.md。

- 实现进展：六个只读入口、分页/引用、事件索引、安装辅助与侧栏页完成。新查询单测、真实子进程 smoke、原 Core/诊断 smoke、前端 lint/typecheck/build、53 项 JS 与 6 项 Python 回归通过。
- 4200 条模拟记录，191 个受预算约束页面，单查询 P95 32–89ms（debug / 内存数据库；不宣称真实全历史规模）。
- 全量 Rust：新增索引使两条“手动删列模拟旧库”的测试夹具需同时移除新索引，已加入各一行测试清理；除此之外既有脏文件保持不变。原有 prompt_output_examples_follow_event_contract 断言 2 个示例，既有工作区 fit_prompt.txt 只有 1 个；已确认文件与开工快照完全相同，归为已有失败。排除此单测后 workspace 回归通过。

- Phase 1：requirements 2 项、adversarial 4 项，DST 重合，共 5 个 blocker；均裁定为本次契约内真实缺陷。修复：事件 moments 保留额外一条 lookahead；午夜跳时用 chrono-tz GapInfo；空响应旧 Core 用已有 Lifecycle Inspect 稳定接口只读识别版本；无文本截图保留 locator/可用性；quality 与 empty_reason 复用新增来源参数的知识查询。
- 定向验证：8 项 context 测试 + 3 项日期/参数测试通过，包含长评论分页、四种截图状态、跨来源失败与待处理、Havana/Santiago/Sao Paulo 午夜跳时及前日。补旧 Core 协议仿真 CLI/MCP 集成回归；待复核及最终 architecture。

- Review 结束：requirements 两项 focused PASS；adversarial 四项 focused PASS，其中版本识别使用实际安装的 protocol20 Core 在临时目录重验 CLI/MCP；architecture 最终 PASS。无剩余 blocker 或用户决策。ARCHITECTURE 仅同步协议版本与契约链接，其余既有修改保留。
- 交付验收：[记录与截图](../research/evidence/2026-10-26-agent-context/verification.md)。完成工作区构建与实现验收；未执行签名发行或替换安装包。
