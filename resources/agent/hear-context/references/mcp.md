# MCP

七个只读工具：hear_status、hear_list_context、hear_search_context、hear_get_event、hear_read_context、hear_list_people、hear_digest。

首次连接时调用 hear_status，传 client_tools 数组，列出当前客户端实际可用的 hear 工具名（去掉 MCP 命名空间前缀）。tool_inventory.state=matched 才说明一致；mismatch 列出 missing/unexpected；client_not_reported 表示未核实。服务端无法自行观察客户端的注入结果。少工具时检查客户端启用范围和缓存，并重新连接，CLI 可作为临时入口。

时间使用 time.date 或 time.from/to。read 的 selector 恰好选择 event_id、time、refs 之一。refs 来自先前响应，不能推断编码；refs 模式只传 selector、limit、cursor。

分页原样携带 page.next_cursor 与原查询。page.truncated/has_more 为 true 表示尚有内容，不能把首屏当完整结果。正文的 fragment 是片段，不是完整引文。工具的 structuredContent 与文本 JSON 是同一份结果。

hear_digest 使用 time、sources、limit、cursor，返回 data.events；事件可能分成多个 event_fragment，用 event_id 合并并按 index 排序。summary 是可拼接的文本片段数组，其余字段为 topics、key_points、mentioned_objects、decisions、todos。content_kind=derived，须核对 evidence_refs；owner/due=null 表示未明确给出，期限可能保留原文相对时间。structured_actions_available=false 时空数组表示旧理解尚未生成行动字段，不能推断“没有待办”。

hear_get_event 的 section 也支持 decisions/todos。understanding_state 区分 ready、pending、failed、not_generated、invalid，含 reason、retryable 与重建入口。读取只返回现有结果，不创建生成任务。

要为升级前的事件补齐行动字段，需用户授权后走 CLI `hear understanding rebuild EVENT_ID`，并等待 LLM 队列成功；仅反复读取 digest 不会补齐字段。新录制可由新版生成这些字段，提取结果仍以实际证据为准。

hear_read_context 默认 min_chars=1，过滤空白原文；min_chars=6 可略去短语。默认保留精转未完全覆盖的粗转，dedupe=true 可额外隐藏部分重叠粗转，但可能省略独有内容。refs 读取不受过滤影响。event/read 可用 include_coverage=false 跳过覆盖率计算；返回 coverage_state=not_requested，不等于完整覆盖。

hear_status 的 health_7d 按音源统计过去七天 recorded_ms、gap_ms、success_rate 与原因；成功率分母是已记录与已知 gap 区间的并集，未知时间排除，不是识别准确率。时段查询的 quality.gap_summary 不因 gaps 详情截断而缺失。

stdio adapter 不启动 Core。Core 离线时打开配套版本 hear 后重试。词库配置、理解重建、mark/comment 等写操作走 CLI；授权依据是当前用户请求。
