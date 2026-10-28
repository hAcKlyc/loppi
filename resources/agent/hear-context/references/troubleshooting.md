# 排障

- core_unavailable：hear 未运行，提示打开 hear。读取不会唤起录音。
- core_incompatible：重新打开配套版本 hear，并重连 MCP。
- invalid_argument：按错误修正参数，使用 context schema 或子命令 help。
- read_busy / db_locked：retryable=true，按 retry_after_ms 短暂等待后重试；持续失败时报告，不高频轮询。
- query_timeout：cause=deadline_exceeded、retryable=false；原参数盲重试没有帮助。可关闭 include_coverage，选具体 section 或缩小范围，再报告失败。3 秒是读快照保护，不是异步任务；失败不会返回伪空数据。
- cursor_stale：相关内容变化，丢弃游标重新查询。
- ref_unavailable / stale：证据不可用，重新定位；不把旧正文作为当前事实。
- record_too_large：选择具体 event section，按游标继续读取。

error.cause、retryable、partial 描述原因、是否建议重试和有无部分结果。正常分页为 ok=true、page.truncated=true、next_cursor 非空，不是查询失败。空 items 是成功查询的结果，应结合 quality 和 understanding_state 解释；未生成理解与无语音不同。

understanding_state.retryable=true 表示保留了可重建的粗转输入，不表示应重复 GET。用户要求重建时使用 `hear understanding rebuild EVENT_ID`；这是异步 LLM 工作，可能计费，读取本身不会触发。`hear cloud tasks llm` / `hear cloud retry TASK_ID` 可查看和处理失败；`hear process-now` 只推进候选处理，不保证补齐已有事件缺失的理解。

`hear integrations doctor --client-tools hear_status,hear_list_context,...` 比对传入的真实客户端工具集；mismatch 退出码 5。不传清单只能诊断服务端，不能宣称客户端工具齐全。
