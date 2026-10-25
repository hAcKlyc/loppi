# MCP

六个工具：hear_status、hear_list_context、hear_search_context、hear_get_event、hear_read_context、hear_list_people。

时间使用 time.date 或 time.from/to。read 的 selector 恰好选择 event_id、time、refs 之一。refs 必须来自先前响应，不能推断其编码。refs 模式不传 transcript、sources 或 include_moments。

分页原样携带 cursor 与原查询。工具返回 structuredContent，同时提供相同 JSON 的文本兼容表示；它们是一份结果，无需重复处理。

仅提供读取工具。stdio adapter 的启动不启动 hear Core。Core 离线时打开 hear 后重试。
