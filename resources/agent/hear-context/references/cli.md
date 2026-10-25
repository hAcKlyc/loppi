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

默认输出 JSON。`--format text` 可供人阅读。日期为 today、yesterday 或 YYYY-MM-DD，范围为带时区偏移的 RFC3339；`--timezone` 接受 IANA 时区。所有范围为 [from,to)。list/read 时间范围最多 31 天；search 不指定日期时检索全部历史。

`--cursor` 原样传入上一页 next_cursor，并保留原查询参数。正文按片段分页；fragment 不是完整引文。event 的 section：overview、summary、topics、key_points、mentioned_objects、moments。

query 是词法短语，不支持任意自然语言语义搜索。person 筛选表示已识别的发言人，不是提及该名字。

需要完整参数时读取 `hear context COMMAND --help` 或 `hear context schema COMMAND`。用 `hear context status` 排障，无需每次查询前调用。
