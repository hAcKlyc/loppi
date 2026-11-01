---
status: implemented
created: 2026-10-31
updated: 2026-10-31
---

# 时间线录制状态去重与筛选语义核实

用户开启下一轮需求，要求从最新main建立dev分支。先修复最新卡片的“正在记录”占据额外位置、与最右“待整理”重复的问题；再解释“隐藏粗转”的实际筛选字段、产生原因及产品必要性。本批只在dev完成一项已定界面修复和代码事实说明，保留筛选行为供后续讨论；不自动合main或发行安装包。

## 执行台账

### 开发契约
- 必赢场景：活跃卡片最右侧主状态为“正在记录”，不再另显“待整理”等普通处理状态；暂停、停止、历史日期或无法确认录制后恢复实际处理状态。
- 原子范围：TimelineView既有展示投影、相关回归、设计说明及筛选语义核实；Core事实、筛选结果、数据/凭据/计费/版本不变。dev保留为下一轮需求工作分支。
- 必须保证：处理异常、局部整理/本地分析异常及手动任务提示仍可见，不被录制状态覆盖；命名标题、摘录、时间“至今”、录制底色/点、详情点击/筛选/来源与历史规则保持。只保留一个普通主状态，异常提示独立表达。
- owner与复用：Core TimelineItem.recording_sources及ServiceState仍为录制事实；liveCard决定实时条目，TimelineView复用live/processingIssue/localIssue/fitIssue与processingLabel，现有recording-label样式与最右主状态位置。hideCoarse仍为App会话内显示偏好，visible/cardExcerpt投影不改。
- 新增机制：无；仅调整既有标签条件与顺序，不新增状态、owner、协议、store、重试、兼容层。
- 明确不做：删除/更名筛选、调整默认筛选、重跑云任务、读传真实正文、发布新版或替换正式安装版；筛选必要性的建议供用户继续需求讨论。

### 验收与结束条件
- 活跃pending_fit卡片只有最右“正在记录”，不含“待整理”；普通fitting/asr进度亦不重复占据主状态位。真实处理异常仍单独可见，录制标记始终最右；非活跃恢复普通状态。
- 复用时间线回归覆盖实时/暂停/历史/错误/来源/隐藏开关；本批typecheck、lint、前端构建与相关Node测试PASS。
- mandatory：真实浏览器的合成活跃、暂停、异常、命名卡片及1120/760布局检查，确认标签位置/数量和异常可见。没有OS集成变化，不重做录音/签名安装验收。
- 逐项对照后按cross-review-code并行fresh requirements/adversarial，裁决、必要修复后fresh architecture PASS，才以Conventional subject+非空body提交dev。完成即结束，不扩张到筛选产品重设计。
- 筛选说明按当前前端visible/cardExcerpt及Core timeline/unorganized/manual_ranges/named_events/coarse生成与事件整理流程核实；说明按钮动作文案、默认值、保留例外、无删除/后台停用效果，并给出改名或保留价值的判断。

### 清单
- [x] 核实main干净、建立dev。
- [x] 录制主状态去重与回归/文档。
- [x] 筛选字段与产生原因事实链。
- [x] 静态/构建、浏览器验证。
- [x] 三路审查、提交与需求闭环。

### Review基线
Base da55f130c2427d9963e31336acb6601eef67c421，<USER_HOME>/Projects/hear，branch dev；范围仅本批最终diff，原本机忽略资料/归档/运行中正式App均不属于改动。

### 待用户决策
本项修复无岔路。“隐藏粗转”后续是否改名/取消需用户理解当前规则后继续讨论，本批不预先改变该行为。

## 核实的筛选语义

- App的hideCoarse默认true，按钮文字显示下一步动作：true时“显示粗转”，false时“隐藏粗转”；状态只在App会话内，后端始终返回完整timeline。
- TimelineView.visible仅在两个ID都为null、不是当前活跃条目、没有本地分析失败、processing_state不在异常/重试集合、且没有落入该区间的moments时收起卡片。保留例外：活跃、local_analysis_failures>0、fit_failed/fit_retrying/fit_credentials_unavailable/analysis_failed/asr_failed/partial_asr_failed、任何现场记录。
- id来自named_events，LLM成功发布事件或用户“新建事件”生成；manual_range_id来自manual_ranges，用户对一段素材显式transcribe_range生成。两者均为空表示尚未被事件或手动范围承接的素材投影，可能积累/待整理/处理中/未成事件/仅原声，不是文字质量判断。
- 本地VAD识别任务经coarse::publish写coarse_segments，并触发note_analysis；候选窗口及LLM处理结果形成named_events。unorganized的processing_state结合有音频、有声音与candidate_windows状态派生：pending_fit也是尚无窗口状态时的默认值，不能等同“已有排队任务”。
- preview.is_precise标识摘录来自精转还是粗转；timeline_preview优先有效精转，否则粗转，最多160字符。该字段不参与卡片筛选。命名事件可能还没精转仍保留；手动范围没有精转结果时可能仍显示粗转摘录。
- 隐藏时，例外保留的普通未整理卡片会收起转录摘录，改用现场笔记/截图文字或诚实空态；活跃卡片实时摘录不受影响。命名事件始终优先摘要，不把原文当摘要。
- 2026-10-31 15:24:56本机只读快照：11条，隐藏开关保留6、收起5；5个命名事件、0手动范围、6未整理投影。收起5条中2条有粗转摘录、3条无preview。只保存聚合计数在build/dev-timeline-status/filter-sample.json，不输出私有标题/正文。
- 产品判断：开关没有识别或存储上的技术必要性，只用于减少列表未整理素材噪声；当前“粗转”名字与ID/例外规则不符。建议后续讨论改成“展开/收起未整理素材”，本批保持原规则不擅改。

## 进展

- 用既有live/processingIssue调整展示顺序和条件：普通处理中状态在live时收起，错误状态在录制主状态前保留，录制标记末位；暂停与不可用状态恢复真实处理状态。无新状态或owner。
- 时间线/刷新12项定向回归、typecheck、lint PASS；正在执行全前端回归、构建和mandatory浏览器验收。

- 完整前端65测试、typecheck、lint和生产前端构建PASS。真实浏览器1120/760共13次观察涵盖活跃、暂停、状态不可用、历史日期、命名事件、活跃且本地/凭据异常；正在记录均为最后主状态，右边距误差0，无横向溢出，异常文字保留，非活跃恢复待整理；开关仍按原规则展开第三张素材卡。证据build/dev-timeline-status/browser-acceptance.json与recording/error截图。

- 固定candidate tree5b9f78d进行fresh requirements/adversarial并行审查，两路PASS，无阻塞或证据缺口；需求独立12回归PASS，对抗独立11回归及额外异常/手动任务组合和暂停/存储暂停/来源停止/不可用转换PASS。主Agent裁决无需修复。最终fresh architecture完整5文件审查PASS，无新机制或范围扩张；既有浏览器/构建/65前端证据继续成立。全部mandatory项已闭环，本批仅在dev交付。

- 实现提交 01eef6d35b6baa7074ea9a7d3544d96543cdaf96；重读完整契约，UI去重与筛选说明已完成，没有筛选产品改动或未覆盖mandatory项。后续仅此台账的提交记录收尾，版本保持0.1.14，main与正式安装包保持本轮开发前状态，dev作为下一轮需求分支。
