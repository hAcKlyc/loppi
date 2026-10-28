# hear debug 真机验收交接

2026-10-29，按 Ethan 指示构建并启动。本文记录首次交接时的环境；Mino 验收通过后已验证兼容性，并通过正常应用启动路径恢复原 GUI 和采集通道，录音仍暂停。最新结果见 [Mino 验收与回退](hear-agent-review-mino-acceptance.md)。未替换 `/Applications/hear.app`，云端词库效果留待 release 验收。

本文所列 debug app 是首轮验收历史包，不含后续 compact 与轻结构范围修订。最终行为以 PR 源码和最新验收记录为准；后续复验使用新构建的 `target/debug` 二进制及隔离数据库副本。

## 构建交接时的状态

- Debug Core PID：`31785`（实时状态以 `control.py status` 为准）。GUI 未启动。
- 使用原数据库：`/Users/zhihu/Library/Application Support/hear/hear.sqlite`，不是数据副本或测试 dispatcher。
- 原 GUI/Core 已正常退出；旧 stdio MCP 进程可能仍存在，它们是协议 21 的客户端，需重连到下面的新版 CLI。
- `allow_capture=false`、`normal_run=false`、`paused_intent=true`，两个音源均 idle。Core 以 `serve --on-demand` 启动。
- LLM/ASR 队列均暂停；切换前均未暂停。录音时间表配置保留，但当前 Core 不会执行正常采集调度。验收期间不要启动 GUI、执行 start/resume 或恢复云端队列。
- GUI/CLI/Core/worker 均为 `0.1.10`、protocol `22`、git `4fd2ba7a72b1a3411a51024e5e333dbaeac0f4dd`、`dirty=true`、epoch `1793209212`。版本号未发布升级，靠协议及构建信息识别本次候选。

## 包和路径

包：`/Users/zhihu/Documents/project/hear/build/agent-review-debug/hear-debug.app`。这是本机 arm64 debug 包，包含模型，未签名/公证，不是发布产物。

CLI 二进制：
`/Users/zhihu/Documents/project/hear/build/agent-review-debug/hear-debug.app/Contents/MacOS/hear`

Core 二进制：
`/Users/zhihu/Documents/project/hear/build/agent-review-debug/hear-debug.app/Contents/Library/LoginItems/HearCore.app/Contents/MacOS/hear-core`

Worker 二进制：同一 `Contents/MacOS` 目录下的 `hear-worker`。

推荐验收入口：
`/Users/zhihu/Documents/project/hear/build/agent-review-debug/bin/hear`

这是固定 `HEAR_DATA_DIR` 为原目录、`HEAR_CORE_BIN` 为本包 Core 的薄包装，随后 exec 真实 CLI。它避免调用管理命令时意外寻找旧 Core，不模拟 IPC 或数据。

## CLI 验收

```sh
hear_debug=/Users/zhihu/Documents/project/hear/build/agent-review-debug/bin/hear
"$hear_debug" build-info
"$hear_debug" context status
"$hear_debug" context list --date 2026-10-28
"$hear_debug" context event 9cc1033b-7acf-4a94-90d8-32bba8d6d4ec --section key_points
"$hear_debug" context read --from 2026-10-28T12:00:00+08:00 --to 2026-10-28T15:00:00+08:00
"$hear_debug" context digest --date 2026-10-28
"$hear_debug" vocabulary list
"$hear_debug" context read
```

最后一条用于检查 invalid_argument，预期非零退出码。分页读取使用同样的查询参数追加 `--cursor '上一页的 page.next_cursor'`，直到 `page.has_more=false`。首轮包的 digest 按 `event_fragment` 分页，不能把首个片段当成整天。最终源码改为默认 compact，每个事件一行，不展开要点正文；`--mode full` 保留正文分片读取。

7 个事件 ID 保存在 `build/agent-review-debug/events-2026-10-28.json`。词库增改删会写原库并保留；测试临时词条可按返回的 entity_id 删除。`vocabulary configure` 修改的是持久开关，测试后恢复原值。

## MCP stdio

可直接执行：

```sh
/Users/zhihu/Documents/project/hear/build/agent-review-debug/bin/hear mcp serve
```

配置已写到 `build/agent-review-debug/mcp.json`：

```json
{
  "mcpServers": {
    "hear-debug": {
      "command": "/Users/zhihu/Documents/project/hear/build/agent-review-debug/bin/hear",
      "args": ["mcp", "serve"]
    }
  }
}
```

若必须直接指向二进制，将 command 改为上面的 CLI 二进制绝对路径，并传 env：`HEAR_DATA_DIR=/Users/zhihu/Library/Application Support/hear` 和 `HEAR_CORE_BIN=上面的 Core 二进制路径`。现有会话的老 MCP 进程不会自动换版本；本次未改 MyAgents 全局或工作区配置。

预期 7 个工具：hear_status、hear_list_context、hear_search_context、hear_get_event、hear_read_context、hear_digest、hear_list_people。启动后向 hear_status 传实际可见的 `client_tools` 数组，应返回 `tool_inventory.state=matched`。

## 共存、数据库变化和切回

两个版本共用数据目录、`core.lock` 和 `core.sock`，不能同时启动 Core。新版已取得唯一 owner 锁。启动时新增词库/实体/ASR 词库快照表及触发器，增加索引，重建查询视图；没有替换生产数据库，也没有主动重建历史理解或 ASR。

启动前通过 SQLite backup 保存并检查了：
`/Users/zhihu/Documents/project/hear/build/agent-review-debug/backup/hear-before-debug.sqlite`

这是数据库备份，不是媒体目录备份。Core 的常规恢复和维护逻辑仍运行。验收词库修改和正常维护写入不会随停止 debug Core 自动撤销。

查看状态：

```sh
python3 /Users/zhihu/Documents/project/hear/build/agent-review-debug/control.py status
```

只停 debug Core：

```sh
python3 /Users/zhihu/Documents/project/hear/build/agent-review-debug/control.py stop
```

验收结束切回安装版：

```sh
python3 /Users/zhihu/Documents/project/hear/scripts/restore_debug_session.py --session-dir /Users/zhihu/Documents/project/hear/build/agent-review-debug
/Applications/hear.app/Contents/MacOS/hear build-info
/Applications/hear.app/Contents/MacOS/hear context status
```

restore 会核对当前 Core 的构建身份，必要时正常退出 debug Core 并等待 owner 锁释放，再通过 `open -a /Applications/hear.app` 走应用正常启动路径。它恢复 GUI 和采集通道，保持原暂停/计划状态，恢复云端队列设置；不直接启动 Core、不手工改运行标志、不覆盖数据库备份。自检打印 capture_blocker、paused_intent、schedule、GUI/Core 状态，失败返回非零。实现已纳入 `scripts/restore_debug_session.py`；本机 control.py restore 转调该脚本。MCP command 同时切回 `/Applications/hear.app/Contents/MacOS/hear`，args 仍为 `["mcp","serve"]`，移除本次专用 env 并重连。不要在新版 Core 运行时打开原 GUI 来强行切换。

Mino 验收通过后已执行 restore，原版直接打开迁移后的原库成功；未覆盖数据库备份。若以后其他版本回退启动异常，应停下并保留现场，不自动用旧备份覆盖后续数据。

## 启动后预检

使用上述真实 CLI→生产 Core→原数据库链路：7/7 事件成功，耗时依次为 900、155、125、104、107、117、202ms；三小时时段 547ms，`truncated=true` 且有 next_cursor；status 222ms，含 health_7d；初版 digest 307ms；最终 compact 模式的验收见最新记录。直接 stdio tools/list 返回 7 个工具，清单自检 matched。

完整构建信息：`build/agent-review-debug/build-info.json`；预检数据：`build/agent-review-debug/preflight.json`；Core 日志：`build/agent-review-debug/debug-core.log`。这些本机产物位于 git 忽略目录，不会进入 PR。Mino 独立验收已通过，详见上述最新验收记录。
