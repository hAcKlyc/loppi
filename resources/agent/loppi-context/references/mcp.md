# MCP

七个只读工具：loppi_status、loppi_list_context、loppi_search_context、loppi_get_event、loppi_read_context、loppi_list_people、loppi_digest。

首次连接时调用 loppi_status，传 client_tools 数组，列出当前客户端实际可用的 loppi 工具名（去掉 MCP 命名空间前缀）。tool_inventory.state=matched 才说明一致；mismatch 列出 missing/unexpected；client_not_reported 表示未核实。服务端无法自行观察客户端的注入结果。少工具时检查客户端启用范围和缓存，并重新连接，CLI 可作为临时入口。

时间使用 time.date 或 time.from/to。read 的 selector 恰好选择 event_id、time、refs 之一。refs 来自先前响应，不能推断编码；refs 模式只传 selector、limit、cursor。

分页原样携带 page.next_cursor 与原查询。page.truncated/has_more 为 true 表示尚有内容，不能把首屏当完整结果。正文的 fragment 是片段，不是完整引文。工具的 structuredContent 与文本 JSON 是同一份结果。

loppi_digest 使用 time、sources、mode、limit、cursor，返回 data.events。默认 mode=compact，每个事件一行：event_id、标题、时间、topics 和 key_points_count，不展开要点正文。标题最多 160 字，topics 最多 6 条、每条 80 字；title_truncated/topics_truncated 标识截断，topics_count 给出总数。理解不可用时 key_points_count=null，不能当成零条。需要完整内容时按 event_id 读取 event section。

mode=full 返回现有 summary、topics、key_points、mentioned_objects；大事件可能分为多个 event_fragment，用 event_id 合并并按 index 排序。summary 是可拼接的文本片段数组。两种模式都受单页预算和 limit 约束；游标不能跨模式使用，读取不会生成缺失的理解。

understanding_state 区分 ready、pending、failed、not_generated、invalid，含 reason、retryable 与重建入口。需要重建现有摘要或轻量理解时，通过 CLI 显式提交并等待 LLM 队列完成。

loppi_read_context 默认 min_chars=1，过滤空白原文；min_chars=6 可略去短语。默认保留精转未完全覆盖的粗转，dedupe=true 可额外隐藏部分重叠粗转，但可能省略独有内容。refs 读取不受过滤影响。event/read 可用 include_coverage=false 跳过覆盖率计算；返回 coverage_state=not_requested，不等于完整覆盖。

loppi_status 的 health_7d 按音源统计过去七天 recorded_ms、gap_ms、success_rate 与原因；成功率分母是已记录与已知 gap 区间的并集，未知时间排除，不是识别准确率。时段查询的 quality.gap_summary 不因 gaps 详情截断而缺失。

stdio adapter 不启动 Core。Core 离线时打开配套版本 loppi 后重试。词库配置、理解重建、mark/comment 等写操作走 CLI；授权依据是当前用户请求。
