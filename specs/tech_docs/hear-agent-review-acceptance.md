# hear Agent 使用评审：修复与验收

评审输入：`/Users/zhihu/Documents/project/mino/workspace/1029-hear-agent-review/hear-agent-review.md`。本次在 hear 仓库完成 P0/P1 及 guide 错误提示修复。业务协议升级为 22；context JSON API 保持 `hear.context.v1`。首轮副本验收没有修改生产录音库、已安装 hear.app 或 MyAgents 配置。后续按 Ethan 授权启动了连接原数据库的 debug Core，随后 Mino 真机验收通过，且已验证原版兼容性并切回安装版；最新记录见 [Mino 验收与回退](hear-agent-review-mino-acceptance.md)，构建路径见 [debug 交接](hear-agent-review-debug-handoff.md)。

## 交付顺序与决策

1. P0：先消除查询放大，沿用同步读取现成结果和有界分页，保留 3 秒保护 deadline。读接口不创建异步 job；只有用户显式请求的理解重建走现有异步 LLM 队列。
2. P1：词库及实体归一、LLM/ASR 注入、digest、理解状态/重建、默认空文本过滤和可选去重、覆盖健康度。
3. 同步 SKILL、真实数据副本及 CLI/MCP 验收。

不需要 Ethan 为 P0 的实现方案做选择。说话人识别按要求不改。P2 的 MCP mark/comment 和 timeline/export 未纳入本批，仍可通过现有 CLI 使用。生产应用更新、MyAgents 客户端实际注入比对、签名运行环境的云端实测仍需后续完成。

## 根因与修复

`reads::Pool` 为快照安装了 3 秒 SQLite progress-handler。超时不是模型生成慢：`context_visible` 对每条证据重复物化全库精转身份投影，混合读又在复合视图上做相关去重/重叠查询。返回内容少，不代表底层扫描少。

修复使用可下推主键和时间条件的 `context_records`，人物投影改为按片段查询；复合视图仅承担不同内容种类的组合。增加 kind/source/time 索引，证据解析复用同一次请求内的结果，粗转点查直接使用现有搜索投影。事件结构化 section 的版本由返回的事件内容与证据生成，不再为读取要点扫描整段精转。

不通过延长 deadline 掩盖问题，不在 GET 中补生成。现有 ref 失效、来源过滤、精转发布状态、人工修正、分页片段与覆盖边界语义继续保留。

## 第 6 节逐项验收

| 原验收项 | 结果 | 证据及边界 |
|---|---|---|
| 10-28 的 7 个事件全部 hear_get_event 成功，P95 <2s | 通过（修复版、数据副本） | CLI 7/7，最慢 163ms；Core 事件调用 P95 465ms；MCP 混合调用包含全部 7 个事件，成功。 |
| 三小时时段一次读取成功或返回分页，不整体超时 | 通过 | `2026-10-28T12:00:00+08:00` 至 `15:00:00+08:00` 返回成功及 `page.truncated/has_more/next_cursor`。 |
| 连续 100 次事件/原文读取无 query_timeout | 通过 | Core 100/100；独立 stdio MCP 100/100，P95 951ms、最大 2305ms。Core 混合 P95 1803ms、最大 2885ms；运行时有其他测试负载。3 秒保护仍开启。 |
| 错误含 cause + retryable，区分没内容/超时/未生成 | 通过 | timeout→deadline_exceeded、retryable=false；db_locked/read_busy 可重试；错误含 partial=false；理解缺失有 understanding_state。分页不是错误。 |
| tools/list 与客户端注入一致（6/6），有启动自检 | 服务端通过；MyAgents 实际注入待联验 | 已安装旧版直接 tools/list 确认 6/6、含 search。修复版增加 digest，共 7/7；独立 MCP 客户端比对通过。新增 hear_status(client_tools) / doctor --client-tools，明确 matched/mismatch/client_not_reported；缺 search 的回归测试通过。当前工作区 project=false，不能冒称 Mino 会话实际注入已修复。 |
| 词库可配置；摘要不再“开山”；支持的 ASR 热词生效、有开关 | 本地与确定性回归通过；云端实测待验证 | CLI 词条/别名/类型/开关已实现；LLM 输入含词库，生成内容规范化回归通过。本地实际音频关闭识别为“到梦空间”，开启为“盗梦空间”；云端请求按官方格式注入、重试保持词库快照，参数测试通过。真实 LLM/云 ASR 测试因 Keychain -25293 在读取凭据时终止，未发云端请求，不能宣称云端效果已验收。 |
| mentioned_objects 带 entity_id，跨两天一致 | 通过 | 专名与实体表打通；跨天“开山/看山”及关闭注入后的 ID 稳定回归通过。未配置对象按规范名称+类型生成稳定 ID；不做隐式语义同一性判断。 |
| hear_digest 返回 decisions / todos | 通过 | MCP 与 CLI 均有入口；两字段携带 evidence_refs，todo 可含 owner/due。已生成快照可一次获取这些结构；内容过大按 event_fragment 分页。旧快照缺少字段时显式 structured_actions_available=false，不伪造决定/待办。 |
| hear_status 提供七天各音源成功率与 gap 汇总 | 通过 | health_7d 按音源给 recorded_ms、gap_ms、observed_ms、success_rate 和原因汇总。比例按已知录音/gap 区间并集计算，排除未知时间；不是识别准确率。时段 gap_summary 不受 gaps 详情截断影响，有重叠区间测试。 |
| 默认去空/去重复，或提供 min_chars / dedupe | 通过 | 默认 min_chars=1；完整被精转覆盖的粗转继续去重。提供 min_chars 和 dedupe=true，后者可隐藏部分重叠的粗转。默认保留尚有独有内容的部分重叠粗转；ref 读不受过滤影响。 |
| SKILL 同步工具、超时、分页、词库、digest | 通过 | 已更新入口与五份 references，quick_validate 通过；安装与 CLI/MCP 冒烟通过。既有用户自定义 SKILL 不被静默覆盖。 |

## 新入口

```sh
hear context digest --date 2026-10-28
hear context event EVENT_ID --section decisions
hear context read --event EVENT_ID --min-chars 6 --dedupe true --include-coverage false
hear vocabulary set '看山' --kind product --alias '开山'
hear vocabulary set '盗梦空间' --kind project --alias '梦空间'
hear vocabulary configure --llm true --local-asr true --cloud-asr true
hear understanding rebuild EVENT_ID
hear context status --client-tools hear_status,hear_list_context,hear_search_context,hear_get_event,hear_read_context,hear_list_people,hear_digest
```

`understanding rebuild` 保留事件标题和边界，只用保留的粗转重建摘要与理解；复用既有 LLM 队列和凭据配置，可能计费。过期响应不会覆盖事件编辑或词库变化。旧摘要不会因为添加词库而自动触发付费重建；本次也未重建生产数据。

本地 X-ASR：随 worker 带入同一模型版本导出的 BPE vocab；中文按该模型的字符边界编码，英文保留 BPE 单词边界；仅开启热词时使用 modified_beam_search。关闭后恢复原解码。云 ASR：首次提交时保存该物理任务的词库快照，后续重试不改变请求上下文。热词不是准确率保证。

## 验证与复现

- `cargo test --workspace --locked`：全工作区通过；后续修复的 Core 路径另跑完整 Core 回归：358 通过、0 失败、18 个需专用环境的测试忽略。最终 CLI/Core/worker 构建通过。
- `python3 scripts/smoke_agent_context.py`：词库真实 IPC 增改删与开关、七工具、缺工具检测、digest CLI/MCP 一致、离线/参数错误、并发读取、保持采集关闭、安装保留用户内容全部通过。
- `context::tests::review_dataset_acceptance`：读取预先制作的 SQLite backup，再复制进临时目录；不会打开生产库写连接。`HEAR_REVIEW_WIRE=1` 使用真实 CLI/MCP 二进制连接同一 Pool/query 实现的临时只读 dispatcher，避免启动录音/模型后台任务。它验证 wire 和查询路径，不宣称替换了安装版 App。
- `events::rebuild::tests`：词库注入、证据、决定/待办、受 ASR 管理的事件、边界与标题保护、不触发 ASR。
- 词库冲突与容量、云端重试词库不可变、跨天实体、空白过滤、gap 汇总、客户端缺工具/错误重试语义、热词分词均有回归测试。
- 本地热词：使用系统 TTS 生成非用户录音的固定语句，真实 X-ASR worker 解码；测试普通公开模型音频时开关两种路径也正常。
- `git diff --check`、Rust 格式检查及 SKILL 校验。

机器可读结果：[evidence/hear-agent-review/acceptance.json](evidence/hear-agent-review/acceptance.json)。

私有数据及合成音频留在系统临时目录；仓库证据仅保留计时、测试状态、合成语句与工具名，不保存用户谈话正文或凭据。

## 外部边界与参考

1. MyAgents 的 `mcp test hear` 仅证实全局 server 配置握手；不能证明某个 Session/Runtime 实际注入了全部工具。没有修改 MyAgents 仓库或配置。需要在 Mino 的客户端传入实际工具清单完成比对。
2. 开发测试进程被 macOS Keychain 拒绝访问现有凭据（`keychain_read_-25293`）。没有尝试改 Keychain 权限或导出凭据。需要在具有既有凭据访问权的配套签名运行环境完成真实 LLM/云 ASR 效果验收。
3. 已安装应用仍是原版本；本次交付是源代码、开发构建与验收记录。发布时需成套更新 Core/CLI/GUI/worker（protocol 22），重连 MCP，再更新已安装的 SKILL。

能力依据：[Sherpa 热词文档](https://k2-fsa.github.io/sherpa/onnx/hotwords/index.html)、[X-ASR 对应模型与 BPE 文件](https://huggingface.co/csukuangfj2/sherpa-onnx-x-asr-zipformer-transducer-zh-en-int8-2026-06-03/tree/main)、[豆包录音文件识别任务提交](https://docs.volcengine.com/docs/doubao-voice/task-submission-http-1?lang=zh)。运行行为以本次测试记录为准。
