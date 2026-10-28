# Mino 真机验收与回退兼容性

## 验收范围与结果

2026-10-29，Mino 使用本机 debug 包、原数据库和真实 CLI/stdio MCP 路径独立验收，录音保持暂停。以下来自 Mino 的验收回执；与开发阶段的副本测试分开记录。

| 项目 | Mino 实测 |
|---|---|
| 事件 key_points | 6/6 成功，0.14–0.24s；170 分钟大事件 0.24s。开发侧此前已覆盖全部 7 个事件。 |
| 三小时时段原文 | 0.66s，返回 truncated 和 next_cursor。 |
| 连续读取 | 50 次，0 失败，p50 0.17s，最大 0.50s。 |
| 稀疏长区间 | 53 分钟、几乎无语音的原失败区间正常返回。 |
| stdio MCP | hear_get_event 0.51s；hear_search_context 0.62s；均成功。 |
| tools/list | 7 个工具，包含 hear_digest。 |
| 错误语义 | 不存在事件返回 ref_unavailable、retryable=false。 |
| 理解状态 | understanding_state 含 reason、rebuild_command、retryable。 |
| 健康度与自检 | health_7d 含各音源 gap 原因及占比；tool_inventory 提供 expected。 |
| 词库 | 临时词条写、读、删成功；已清理，entries 为空。 |

Mino 结论：验收通过，授权提交 PR。正式合并与发布由 Ethan 在另一台机器完成。原始问题文档与本机验收脚本位于 `/Users/zhihu/Documents/project/mino/workspace/1029-hear-agent-review/`；仓库不复制用户谈话正文、数据库或本机 debug 产物。

## 三点反馈的处理

### 1. 已实现：digest compact 模式

初版 10-28 全天需 21 页、24 个 fragment 覆盖 7 个事件。最终默认 compact：每个事件一行，只含标题、时间、topics、要点条数和理解状态，不展开 key_points 正文。标题最多 160 字，topics 最多 6 条、每条 80 字，并标识截断；细节通过 event_id 读取。保留 mode=full 供需要正文的调用，游标绑定模式，单页仍有大小限制。7 个极端长事件的边界测试两页内拿全；真实数据结果见下方补充记录。

### 2. 不做：决策/待办抽取和旧事件补齐

按 Ethan 最新决定，第一次粗转 LLM 处理只保留 topics、key_points、mentioned_objects。移除了决策/待办的生成定义、输出 schema、增量替换、event section、digest 字段和状态标记；不保留空字段或开关。旧理解 JSON 中额外字段忽略，不迁移，不为其重建。通用理解重建仍可修复缺失理解或应用新词库。云端 ASR 词库留待 release 后实测。

### 3. 已确认：protocol 21 可打开迁移后的库

结论：本次迁移可直接切回已安装的 hear 0.1.10（protocol 21），无需用旧数据库备份覆盖现库。

验证使用真实安装版 CLI/Core，构建为 `bf161ccb5311743d037af05a938d8f211ca2becd`、epoch `1793124296`。先从已迁移生产库制作临时 SQLite backup，在同一副本上依次运行 protocol 21 和 protocol 22：

- 两版均成功启动，状态、事件列表、5 分钟原文读取、cloud status 均成功。
- 两版的 existing event update 触发器执行正常（验证事务随后回滚）。
- named_events、coarse_segments、precise_segments、vocabulary_entries、entities 的逐行内容散列与数量保持一致。
- 两版启动后的 `PRAGMA quick_check` 均为 ok。

随后实际运行 `control.py restore` 切回原安装版 Core，直接打开原库成功；`context status`、事件列表和短区间读取通过。没有恢复数据库备份、没有替换 `/Applications/hear.app`。数据库兼容性成立；当时只恢复 Core 的 on-demand 状态未恢复 GUI/采集通道，后续按下面的正常应用路径修正。机器可读副本验证见 [downgrade-compatibility.json](evidence/hear-agent-review/downgrade-compatibility.json)。

兼容原因：本批新增表、触发器、索引及可重建查询视图，没有改名/删除原版依赖的表或列，也未引入拒绝旧版打开的 schema gate。两版启动时各自重建查询视图。新增词库表可保留在库内，但 protocol 21 不提供新版词库、digest 和健康度接口，P0 查询性能修复也不会随数据库保留给旧二进制。CLI 与 Core 必须配套使用；数据库可回退不代表 protocol 21/22 可混用。

## 切回步骤与备份

本机已完成以下步骤，当前无需重复执行：

```sh
python3 scripts/restore_debug_session.py --session-dir build/agent-review-debug
/Applications/hear.app/Contents/MacOS/hear build-info
/Applications/hear.app/Contents/MacOS/hear context status
```

MCP 指回 `/Applications/hear.app/Contents/MacOS/hear`，args 为 `["mcp","serve"]`，移除 debug 专用 env 并重连。此次没有改 MyAgents 配置；若验收时自行改过配置，应自行切回。

恢复脚本已改为 `scripts/restore_debug_session.py`：核对 owner，必要时退出 debug Core 并等待锁释放，然后通过 LaunchServices 打开原应用。应用自行管理 Core，脚本不手工改运行标志。完成后必须通过 GUI 存活、配套 Core 构建、normal_run/allow_capture、capture_blocker、暂停意图与计划配置自检，否则非零退出。原库备份仍保留在 git 忽略目录 `build/agent-review-debug/backup/hear-before-debug.sqlite`。备份仅作异常恢复兜底；正常切回不应用旧备份覆盖后续新增数据。

源代码回归与完整 11 条验收矩阵见 [修复与验收](hear-agent-review-acceptance.md)。

提交前统一离线 gate `python3 scripts/verify.py` 全部通过，含静态检查、Rust/前端/Python/原生回归、构建身份及 Core/诊断冒烟；见 [pr-verification.json](evidence/hear-agent-review/pr-verification.json)。SKILL 校验与格式检查通过。

## 最终范围修订复验

2026-10-29 开发侧以迁移后原库的 SQLite backup 启动隔离 Core，使用本次源码构建的真实 CLI 和 stdio MCP 复验，未向生产库写入，也未启用录音或云端请求：

| 项目 | 结果 |
|---|---|
| 默认 compact，10-28 全天 | CLI 一页返回全部 7 个事件，6,696 bytes，220ms；stdio MCP 同样一页 7 个事件，240ms。 |
| full 模式 | 原摘要和轻结构仍可分页读取，输出无已移除字段。 |
| 边界回归 | 7 个长标题、多主题、各 200 条要点的事件两页内返回；模式变更不能复用游标；旧 JSON 额外字段不影响读取且存储不变。 |
| 原安装版实际恢复 | 正常 GUI 存活，protocol 21 Core，normal_run=true、allow_capture=true、capture_blocker=none。 |
| 暂停与计划 | paused_intent=true；计划仍启用，10:00–23:00，schedule.error=null；采集通道已恢复，录音仍暂停。 |
| restore 失败检测 | query_only、GUI 缺失、暂停/计划变化和暂停时仍采集均有失败回归；自检不通过返回非零。 |

机器可读记录：[scope-revision.json](evidence/hear-agent-review/scope-revision.json)。恢复使用原生产库且没有覆盖备份。初轮 debug app 保留作验收历史，本次 compact 复验使用新构建的 `target/debug` 二进制，未重打或替换该历史包。
