# 排障

- core_unavailable：hear 未运行，提示打开 hear。读取不会唤起录音。
- core_incompatible：重新打开配套版本 hear，并重连 MCP；不要直接读 SQLite。
- invalid_argument：按错误修正参数，使用 context schema 或子命令 help。
- read_busy / query_timeout：等待 retry_after_ms 后缩小范围重试，避免高频轮询。
- cursor_stale：相关内容变化，丢弃游标重新查询。
- ref_unavailable / stale：当前证据不可用，重新定位；不把旧正文作为当前事实。
- record_too_large：缩小时间范围或选择更具体的 event section。

无匹配或待处理可以是成功查询。检查质量字段；不要把网络/协议失败解释成空记录。
