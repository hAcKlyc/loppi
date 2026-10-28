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

### 1. 后续：digest compact 模式

不阻塞本 PR。10-28 全天目前需 21 页、24 个 fragment 才覆盖 7 个事件，每页约 2000 字符。记录后续需求：增加 compact 模式，只返回标题、topics、decisions、todos 和计数，不分页展开 key_points 正文；正文继续按需通过 event/read 获取。本 PR 不扩展实现范围，也不将此项计为已完成。

### 2. 当前限制：旧事件没有结构化行动字段

升级前的 understanding 不含 decisions/todos，因此 digest 返回空数组及 `structured_actions_available=false`。这只表示尚未生成，不表示原谈话没有决定或待办。

需要补齐时，由用户显式执行 `hear understanding rebuild EVENT_ID`，等待已配置且未暂停的 LLM 队列成功，再读 digest。重建可能计费；不重跑 ASR、不改事件标题和边界。新版也会在处理新录制时请求这些字段，但本次没有新录制/真实云端生成的效果验收。CLI/MCP SKILL 已补充这一路径；云端 ASR 词库按 Ethan 决定留待 release 后实测。

### 3. 已确认：protocol 21 可打开迁移后的库

结论：本次迁移可直接切回已安装的 hear 0.1.10（protocol 21），无需用旧数据库备份覆盖现库。

验证使用真实安装版 CLI/Core，构建为 `bf161ccb5311743d037af05a938d8f211ca2becd`、epoch `1793124296`。先从已迁移生产库制作临时 SQLite backup，在同一副本上依次运行 protocol 21 和 protocol 22：

- 两版均成功启动，状态、事件列表、5 分钟原文读取、cloud status 均成功。
- 两版的 existing event update 触发器执行正常（验证事务随后回滚）。
- named_events、coarse_segments、precise_segments、vocabulary_entries、entities 的逐行内容散列与数量保持一致。
- 两版启动后的 `PRAGMA quick_check` 均为 ok。

随后实际运行 `control.py restore` 切回原安装版 Core，直接打开原库成功；`context status`、事件列表和短区间读取通过。没有恢复数据库备份、没有替换 `/Applications/hear.app`。录音仍暂停，原有云端队列开关已恢复。机器可读副本验证见 [downgrade-compatibility.json](evidence/hear-agent-review/downgrade-compatibility.json)。

兼容原因：本批新增表、触发器、索引及可重建查询视图，没有改名/删除原版依赖的表或列，也未引入拒绝旧版打开的 schema gate。两版启动时各自重建查询视图。新增词库表可保留在库内，但 protocol 21 不提供新版词库、digest 和健康度接口，P0 查询性能修复也不会随数据库保留给旧二进制。CLI 与 Core 必须配套使用；数据库可回退不代表 protocol 21/22 可混用。

## 切回步骤与备份

本机已完成以下步骤，当前无需重复执行：

```sh
python3 /Users/zhihu/Documents/project/hear/build/agent-review-debug/control.py restore
/Applications/hear.app/Contents/MacOS/hear build-info
/Applications/hear.app/Contents/MacOS/hear context status
```

MCP 指回 `/Applications/hear.app/Contents/MacOS/hear`，args 为 `["mcp","serve"]`，移除 debug 专用 env 并重连。此次没有改 MyAgents 配置；若验收时自行改过配置，应自行切回。

恢复脚本先核对 debug Core 身份，正常退出并等待 owner 锁释放，再启动原 Core；不同时运行两个 Core，不覆盖数据库，不恢复录音。原库备份仍保留在 git 忽略目录 `build/agent-review-debug/backup/hear-before-debug.sqlite`。备份仅作异常恢复兜底；正常切回不应用旧备份覆盖后续新增数据。

源代码回归与完整 11 条验收矩阵见 [修复与验收](hear-agent-review-acceptance.md)。

提交前统一离线 gate `python3 scripts/verify.py` 全部通过，含静态检查、Rust/前端/Python/原生回归、构建身份及 Core/诊断冒烟；见 [pr-verification.json](evidence/hear-agent-review/pr-verification.json)。SKILL 校验与格式检查通过。
