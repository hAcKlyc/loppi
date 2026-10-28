# CLI

可执行文件：{{HEAR_CLI}}
下方 `hear` 表示上述完整路径；不要求它位于 PATH。

```sh
hear context list --date today
hear context search '报价' --date yesterday
hear context people --name '张三'
hear context event EVENT_ID --section key_points
hear context read --event EVENT_ID --transcript best_available
hear context read --ref RETURNED_REF
hear context read --from '2026-10-26T10:00:00+08:00' --to '2026-10-26T10:30:00+08:00'
```

默认输出 JSON。`--format text` 可供人阅读。日期为 today、yesterday 或 YYYY-MM-DD，范围为带时区偏移的 RFC3339；`--timezone` 接受 IANA 时区。所有范围为 [from,to)。list/read/digest 时间范围最多 31 天；search 不指定日期时检索全部历史。

`--cursor` 原样传入上一页 next_cursor，并保留原查询参数。正文按片段分页；fragment 不是完整引文。event 的 section：overview、summary、topics、key_points、mentioned_objects、moments、decisions、todos。

query 是词法短语，不支持任意自然语言语义搜索。person 筛选表示已识别的发言人，不是提及该名字。

需要完整参数时读取 `hear context COMMAND --help` 或 `hear context schema COMMAND`。用 `hear context status` 排障，无需每次查询前调用。

## Digest 与覆盖健康度

```sh
hear context digest --date yesterday
hear context event EVENT_ID --section decisions
hear context event EVENT_ID --section todos --include-coverage false
hear context read --event EVENT_ID --min-chars 6 --dedupe true
hear context status
hear understanding rebuild EVENT_ID
```

Digest 返回 data.events，结构、事件分片和旧数据状态见 [MCP](mcp.md)。重建走现有 LLM 队列，更新摘要与理解，不重跑 ASR、不改标题或时间边界；队列暂停/未配置会等待，失败原因可查 cloud tasks。词库更新只影响后续识别/生成；已有摘要需显式重建。

升级前生成的事件可能没有 decisions/todos，`structured_actions_available=false` 不表示没有决定或待办。若需要这些结构字段，须显式执行 `hear understanding rebuild EVENT_ID`，等 LLM 任务成功后再读 digest；这可能计费，读取本身不会自动重建。也可等待新版对新录制生成理解。

## 词库

```sh
hear vocabulary list
hear vocabulary set '看山' --kind product --alias '开山'
hear vocabulary set '盗梦空间' --kind project --alias '梦空间'
hear vocabulary set 'MyAgents' --kind product
hear vocabulary configure --llm true --local-asr true --cloud-asr true
hear vocabulary configure --local-asr false
hear vocabulary set '新名称' --id ENTITY_UUID --kind product --alias '旧名称'
hear vocabulary remove ENTITY_UUID
```

同名 set 更新该条，aliases 是完整替换列表；改名传 --id 保留实体 ID。--enabled false 暂停该条注入。最多 100 条、每条 20 个别名，名称最长 100 字，词库总计最多 64 KiB；不允许两个启用条目争用名称或别名。mentioned_objects.entity_id 由本地词库/规范名称生成，跨天稳定；它不证明两个同名对象在现实中是同一对象，也不是说话人 ID。

LLM 使用规范名称、类型与别名上下文。X-ASR transducer 使用对应 BPE 词表和 modified_beam_search 做热词偏置，开关关闭恢复原解码；每条音频开始时取当前词库，词条含 `/` 或 `:` 不送入本地热词。豆包录音识别通过 request.corpus.context 注入规范名称；首次提交后该任务词库固定，重试沿用相同上下文。热词提高命中概率，不保证音频一定识别正确；原文不会被全局字符串替换。
