# CLI

可执行文件：{{LOPPI_CLI}}
下方 `loppi` 表示上述完整路径；不要求它位于 PATH。

```sh
loppi context list --date today
loppi context search '报价' --date yesterday
loppi context people --name '张三'
loppi context event EVENT_ID --section key_points
loppi context read --event EVENT_ID --transcript best_available
loppi context read --ref RETURNED_REF
loppi context read --from '2026-10-26T10:00:00+08:00' --to '2026-10-26T10:30:00+08:00'
```

默认输出 JSON。`--format text` 可供人阅读。日期为 today、yesterday 或 YYYY-MM-DD，范围为带时区偏移的 RFC3339；`--timezone` 接受 IANA 时区。所有范围为 [from,to)。list/read/digest 时间范围最多 31 天；search 不指定日期时检索全部历史。

`--cursor` 原样传入上一页 next_cursor，并保留原查询参数。正文按片段分页；fragment 不是完整引文。event 的 section：overview、summary、topics、key_points、mentioned_objects、moments。

query 是词法短语，不支持任意自然语言语义搜索。person 筛选表示已识别的发言人，不是提及该名字。

需要完整参数时读取 `loppi context COMMAND --help` 或 `loppi context schema COMMAND`。用 `loppi context status` 排障，无需每次查询前调用。

## Digest 与覆盖健康度

```sh
loppi context digest --date yesterday
loppi context digest --date yesterday --mode full
loppi context event EVENT_ID --section key_points --include-coverage false
loppi context read --event EVENT_ID --min-chars 6 --dedupe true
loppi context status
loppi understanding rebuild EVENT_ID
```

Digest 默认 `--mode compact`，每个事件一行，含标题、时间、topics 和 key_points_count，不含要点正文。`--mode full` 展开现有摘要与轻量理解，可能按事件分片分页，结构见 [MCP](mcp.md)。重建走现有 LLM 队列，更新摘要与理解，不重跑 ASR、不改标题或时间边界；队列暂停/未配置会等待，失败原因可查 cloud tasks。词库更新只影响后续识别/生成；已有摘要需显式重建。

## 词库

```sh
loppi vocabulary list
loppi vocabulary set '看山' --kind product --alias '开山'
loppi vocabulary set '盗梦空间' --kind project --alias '梦空间'
loppi vocabulary set 'MyAgents' --kind product
loppi vocabulary configure --llm true --local-asr true --cloud-asr true
loppi vocabulary configure --local-asr false
loppi vocabulary set '新名称' --id ENTITY_UUID --kind product --alias '旧名称'
loppi vocabulary remove ENTITY_UUID
```

同名 set 更新该条，aliases 是完整替换列表；改名传 --id 保留实体 ID。--enabled false 暂停该条注入。最多 100 条、每条 20 个别名，名称最长 100 字，词库总计最多 64 KiB；不允许两个启用条目争用名称或别名。mentioned_objects.entity_id 由本地词库/规范名称生成，跨天稳定；它不证明两个同名对象在现实中是同一对象，也不是说话人 ID。

LLM 使用规范名称、类型与别名上下文。X-ASR transducer 使用对应 BPE 词表和 modified_beam_search 做热词偏置，开关关闭恢复原解码；每条音频开始时取当前词库，词条含 `/` 或 `:` 不送入本地热词。豆包录音识别通过 request.corpus.context 注入规范名称；首次提交后该任务词库固定，重试沿用相同上下文。热词提高命中概率，不保证音频一定识别正确；原文不会被全局字符串替换。

## 智能标签与事件策略

```sh
loppi workflow config show
loppi context list --date today --tag TAG_ID
loppi context event EVENT_ID --section overview
loppi workflow strategies
loppi workflow run EVENT_ID STRATEGY_ID --if-revision EVENT_REV --config-revision CONFIG_REV
loppi workflow runs EVENT_ID --run RUN_ID
loppi workflow result EVENT_ID STRATEGY_ID --version RESULT_VERSION_ID
```

`event tags EVENT_ID EVENT_REV --tag TAG_ID` 默认只预览，确认后将返回的 `plan` 通过 `--apply` 和原参数提交。未确定事件先走 `event complete` 或 `event confirm-range`。执行后返回的 run ID 表示请求已接受，正文以成功版本为准。缺精转稿时，默认执行返回可用选项。经用户授权后显式使用 `--input coarse` 或 `--input prepare-precise`。配置更新用 `workflow config set --file PATH --if-revision CONFIG_REV`，`--preview` 只校验，`--add-required-asr-tags` 明确扩大自动精转范围。配置文件内容取 `workflow config show` 的 `data.config`。写入不明时复用原请求 ID 与同一负载。
