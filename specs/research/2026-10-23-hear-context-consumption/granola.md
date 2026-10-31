# Granola 深度调研：从会议笔记走向工作上下文消费

[返回主报告](../2026-10-23-hear-context-consumption.md)

调研截止与访问日期：**2026-10-23（Asia/Shanghai）**。本报告依据可打开的官方文档、产品发布、作者自述及公开讨论，未登录 Granola 做现场产品测试。下文的“已提供”指官方当前操作文档确认有入口与使用条件，不等于研究者亲测；“用户体验”是具体作者自述；“建议”是对 hear 的推断。

## 1. 对 hear 最重要的发现

Granola 值得研究的重点是：它把同一份对话内容放进了不同时间、不同工作任务的入口。会中只写自己在意的几个点，会后得到可编辑笔记，再把笔记用于找回决定、准备下一次对话、写跟进邮件、形成项目材料。到 2026 年，其官方产品已经包括 **Briefs、自动建议的 Follow-up emails、Chat 中的外部动作、跨会议 Chat、Recipes、People/Companies、MCP**，不能再按“无 bot 的总结器”理解。[G02–G13]

但证据强度不同。独立作者反复提到的是“少量手写线索 + AI 补全”“无 bot 的轻量体验”“得到可修改、可带走的工作笔记”。本次没有找到足以证明 Briefs、自动 Follow-up、教练 Recipes、跨会议分析已经成为广泛用户高频习惯的独立数据。官方销售团队有使用案例，但属于厂商自述。[G16–G21]

对 hear 的推断：**持续采集的价值，应该在“我现在要找回什么、完成什么、继续哪件事”时兑现。** 自动收集只是输入优势；如果产物仍然是需要逐条阅读的全天摘要，用户依然承担筛选和搬运成本。Granola 的启示更偏向结果入口、证据回溯和工作流衔接，而不是模板数量。

## 2. 已核验的能力与交互

| 能力 | 真实入口与行为 | 条件、边界、证据状态 |
| --- | --- | --- |
| 主动开笔记、被动听会议 | 点击会议通知、打开已开始的日历会议，或 New Note 后采集；提前打开会议笔记可以到点开始。绿色音量条、实时 transcript 面板和浮动指示器显示运行状态 | **不是全天自动采集**；只开着软件并不保证所有通话都会记录。桌面取麦克风与系统音频，无参会 bot。[G01] |
| 转写而非原音档案 | 音频直接交给转写提供商；不保存可回放的原音/视频；可查看、搜索、复制 transcript | 不能上传录音文件进行离线转写。当前 speaker tags 支持部分会议平台；不适用时主要是 Me/Them。不能把“本机采集”写成“全部本地处理”。[G01] |
| 人的线索决定笔记重点 | 会议结束自动增强；输入是 transcript、用户 raw notes、日历信息；用户原笔记有视觉区分；可编辑并重新生成 | 每条增强笔记旁可追溯到 transcript/raw notes。对单次笔记的修改不会自动训练未来风格。[G02] |
| Templates 定输出结构 | 会中在笔记区输入 `/`，选择会议模板；会后换模板会重新生成；可自建和分享 | 当前在 Mac/Windows 设置和应用，手机笔记同步后可用桌面模板处理。模板指定重点、结构与细节程度。[G03] |
| 单会、跨会 Chat | 可从主页、单次会议、文件夹、多选会议、人物或公司页面发问；选择范围本身就是交互的一部分 | 免费计划可用最近 30 天；Business/Enterprise 可用完整历史。当前文档已不是旧版“默认最近 40 场摘要”的说明。[G04] |
| Recipes 定下游任务 | Chat 输入 `/` 调出保存的 prompt，也可在侧栏浏览；可规定 single/multiple meetings，创建、分享、remix | Template 是笔记结构；Recipe 是消费笔记的任务，例如提取洞察、起草材料。分享 Recipe 不等于分享自己的会议内容。[G05] |
| Spaces/Folders 定上下文范围 | 私人 My notes、团队 space；笔记可进多个 folder；周期会议可设置以后自动归档 | 当前支持一层子文件夹。文件夹不仅帮助浏览，也约束 Chat 范围与共享范围。[G06] |
| People/Companies 自动聚合 | 根据日历参会者自动建目录；点人或公司即可看到有关笔记并围绕该对象 Chat | 依赖日历参与人，当前不能手动补建人物/公司。临时环境对话和未列入日历的人不是天然覆盖范围。[G07] |
| Pre-meeting Briefs | 夜间为后续外部会议准备；打开会议笔记，Chat 顶部自动出现约 2–3 点提示，可展开 | 结合过往同人/公司会议、共享笔记、日历、web、可选 Gmail。Mac/Windows/iOS；2026-05-20 已有发布文，当前帮助的 Troubleshooting 明确限 Business/Enterprise 且连接 Google Calendar，Microsoft Calendar 尚未支持。**官方确认，独立使用价值证据不足**。[G08] |
| Suggested follow-up emails | 合适的外部会议后自动起草；主页 Suggestions 或会议顶部 View draft 进入编辑器，确认后发送 | 付费/试用、Google 登录、同账号 Gmail read/send；至少一位外部参与者、总人数不超过 10 人。约笔记生成后一两分钟出现；判断不适合邮件时可不生成。**官方确认，非已证明高频留存功能**。[G09] |
| Chat 中执行后续工作 | 用户要求写邮件、发 Slack、约时间 → 可编辑草稿 → 用户确认 Send/Create event | 当前桌面 Mac/Windows；Gmail 仅 Google 账号，Slack 支持频道/群聊/私聊，约会支持同 Google Workspace 域人员。独立于自动 Follow-up 建议；文档未明确通用套餐门槛，且提到账号需启用相应动作。[G10] |
| MCP 把上下文送到外部 AI | OAuth 接入后，外部 AI 可查询会议、读取笔记、取 transcript/附件、浏览 folder | 免费最近 30 天；部分 folder/search/transcript 能力需付费；只在当前 active workspace 内检索。企业可控制摘要/原文/修改范围。不能把 MCP 简化成“自动执行全部工作”。[G11] |
| 外部工具与导出 | 官方有 Notion、Slack、Zapier、Attio、HubSpot、Affinity；全量个人导出包含笔记/摘要/transcript | 连接方式各不相同；CSV 导出是异步邮件，默认仅自己的、有摘要的笔记。免费版历史超过 30 天不在界面展示，不等于已删除，完整导出仍可包含。[G12] |
| 共享屏幕 Snapshots | 在特定会议平台中自动抓取对方共享内容，放入笔记 Images 并用于笔记增强 | Mac、macOS 14+；需权限与开关，Windows 未支持；不是全天桌面录屏。2026-10 官方已有发布与帮助文档。**能力确认，用户反馈不足**。[G13] |

这里最有价值的交互细节是“有默认行为，也可轻量修正”：用户不用整理完整 transcript；几句 raw notes 提供个人重点，范围选择约束后续回答，引用允许检查 AI 做了什么，草稿允许把结果变成真正工作材料。这个组合比单纯“准确率更高”更接近用户获得价值的过程。[G02、G04、G10]

## 3. 从任务到产物的完整链路

以下链路区分已经能按文档完成的操作，以及尚未得到独立重复使用验证的价值主张。

| 任务与触发 | 输入与关键交互 | 产物 | 如何继续复用 | 证据 |
| --- | --- | --- | --- | --- |
| 接连会议中避免漏记 | 开始通话时打开笔记；听会，仅写几条个人重点 | 可编辑增强笔记，带源文定位 | 会后修订、复制、转给同事，之后继续查询 | 官方操作 + 独立作者共同支持。[G01–G03、G16、G17] |
| “上次到底决定了什么” | 打开人物/公司/文件夹，或多选相关会议；直接询问 | 有上下文范围的回答 | 回到对应会议核实，再形成新决定 | 官方 Chat/对象目录能力明确；跨会质量未独立测量。[G04、G07、G14] |
| 下一场客户会议马上开始 | 日历给出人物与时间；打开会议笔记自动见 Brief | 极短的前情、未闭环事项和相关信息 | 用户可追问，进入本次会谈 | 已有正式发布与操作文档；独立长期证据不足。[G08] |
| 会议结束，需要推进下一步 | 自动 Follow-up 或主动在 Chat 说明收件人、意图；检查编辑 | 邮件、Slack 消息或日历邀请草稿 | 确认后进入原有沟通工具，减少搬运 | 官方文档；厂商销售团队自述有日常使用。[G09、G10、G15] |
| 设计评审后要开任务 | 选择单会；运行明确区分决定、建议、缺失信息的 Recipe | 实施简报/拟建任务，未约定的负责人和日期不补造 | 先核对，再复制到任务系统或交给外部 AI | 独立作者有实际界面演示，但使用的是**虚构会议样例**，不证明真实 ASR 质量。[G17] |
| 汇总长期客户反馈 | 把相关访谈放入 folder，统一笔记结构，再对整个集合问问题 | 主题、异议、机会、待核实问题 | 用于产品讨论、销售复盘、项目材料 | 已有功能与官方示例；没有量化证明这些产物改善了决策。[G03–G06、G14、G15] |
| 已经在别的 AI 中写 PRD/代码/方案 | 外部 AI 通过 MCP 按人物、主题或时间找会议 | 当前工作需要的会议上下文 | 在工作发生的地方继续生成/执行 | 官方接口确认；具体外部写操作仍依赖目标工具与权限。[G11] |

Granola 从 2025 年 Recipes 发布时就展示“根据 brainstorm 写 brief/PRD、根据 1:1 做 coaching”等场景。其销售团队 2026 年又说明，最常用的 Recipe 内嵌 discovery framework，用于发现“对客户还不知道什么”。这提示了一个可迁移方向：**输出不只是告诉用户“说过什么”，还可以指出“为了继续这件事，仍缺什么”**。但这些仍是厂商给出的案例，不能直接当作普通用户高频需求。[G05、G15]

## 4. 用户口碑：可以信到哪一层

这不是随机抽样，也没有访问留存或使用量后台。下面只记录可核查的小样本；多个站点复述同一卖点不按多个独立证据计算。论坛中大量竞品创始人自荐与推广内容已降权。

| 样本与使用背景 | 具体体验 | 可支持的判断 | 不应推出的结论 |
| --- | --- | --- | --- |
| **Celine Wee**，个人文章，2025-02-15；作者简介为 GTM/Payments/Biz Ops 背景，beta 使用者，未报使用时长 | 喜欢非美式口音转写、自己的粗笔记能融入结果、界面简单；仍认为亲手写少量笔记有助思考 | “保留人对重点的选择，再由 AI 扩展”有明确真实体验支持。[G16] | 她的一次主观“转写很好”不能代表全部口音、语言和噪声表现 |
| **Zachary Proser**，2026-08-02；自述数月团队会议、访谈和规划会使用；明确 affiliate 披露 | 强调可编辑笔记是最终产品，少量 cues 可以引导结果；缺少音频回放、导入和无人出席录制不适合所有人 | 可解释持续使用的工作方式；其 45 秒实施简报演示展示了从笔记到草稿、再保存 Recipe。[G17] | 演示是虚构文本输入，不能作为真实会议识别准确性实验；商业推荐有偏差 |
| **App Store：m571**，2025-05-22，自述使用近半年 | 除会议外，还把零散想法说出来，再结构化并对自己的想法追问 | 自我口述 → 整理 → 追问，是会议之外的真实用途线索。[G18] | 不是“全天音频自动生成个人第二大脑”的验证 |
| **App Store：Jlyddiatt**，页面显示 9 月 3 日、无年份；自述日常用在 1:1、个人电话、预约 | 搜索和问答有用；笔记积累后，想要更好的组织和层级 | 价值随积累出现，同时也产生组织负担。[G18] | 不能据此断言当前完全没有子文件夹，官方当前文档已经有一层子文件夹 |
| **Reddit：Onefootforward_7818**，相关主题发表于 2026-02-09，评论仅显示相对时间；专业对话场景 | 自述两次界面像在运行，但事后无笔记且无告警；同帖 morphector 说自己未遇到 | “静默失败”是值得专门验证的产品失败模式。[G19] | 没有日志，不能确定根因，更不能计算 Granola 的整体故障率 |
| **Reddit：Kitdeluca1404**，2025-11-29，项目管理使用背景 | Granola 记会议、Bear 记项目、Asana 管任务，来回搬运耗时 | 笔记生成后仍可能增加工作，最后一步的衔接本身有价值。[G20] | 不能推成所有用户都需要内置完整任务管理器 |
| **HN：eawgewag**，2025-07-30 | 自述发现 folders 对全公司公开，涉及 1:1，损害信任 | 用户对“谁能看到”有强烈预期；共享状态需要明显可理解。[G21] | 无法复现旧版配置；当前官方已区分私人 My notes 和 team space，不能作为现存默认泄露事实 |
| **HN：tomelliot**，页面仅显示约 3 个月前；自述 Obsidian 同步插件作者 | 认为认证调整阻断原有自动拉取；寻找后台 daemon、本地处理、弱 GUI 的替代 | 稳定可编程出口对一类重度用户至关重要，和 hear 定位很接近。[G22] | 当前有官方 MCP、API 和 CSV，不能写“Granola 无法导出” |

**较有把握的口碑主题**：参与讨论时少分心；人的短笔记能影响 AI 重点；结果可编辑；上下文能在原有工作中复用。**有价值但待验证的主题**：主动会前提示、跨会主题分析、自动跟进、管理/沟通 coaching。二者应分别排优先级。[G16–G22]

负面反馈集中给出了三种可测试风险，而不是简单“AI 不准”：记录链路是否真的完成；产物是否丢失决定背后的细节；迁出和共享是否符合用户预期。后一类常常比多一款模型或多几个模板更决定信任。

## 5. 应避免的过时信息与证据冲突

1. **“Granola 自动记住全天所有会议”不成立。** 当前官方明确需要打开笔记；对于“开着一整天却漏会”的抱怨，既可能有故障，也可能有产品预期不匹配，不能只认一个解释。[G01、G19]
2. **“音频只在本地、从不上云”不成立。** 官方说不保存可回放原音，同时会把音频送转写提供商；采集位置与计算位置是两回事。[G01]
3. **“当前 Chat 只看最近 40 场摘要”已过时。** 搜索索引还出现旧 help 文本，打开同一帮助链接已转向新版文档；本报告以打开后的正文为准。[G04]
4. **“免费 30 天之后笔记被删除”不能这样说。** 当前导出文档明确完整导出仍能包含更早历史；访问门槛不等于物理删除。[G12]
5. **“没有 speaker identification / 子文件夹 / Microsoft 支持”不能用旧评论定当前状态。** 官方当前能力与早期体验已不同。此报告只把旧评论当时的需求和失望保留下来。[G01、G06、G18]
6. **“MCP 本身会更新 Linear、写代码、发 CRM”混淆了上下文提供与执行。** Granola 发布文举这些场景，实际需要外部客户端再连接目标工具；此外当前官方 MCP 已出现可控的文件夹修改能力，因此也不能称完全只读。[G11]
7. **没有以竞品评测替代中立实测。** 搜索发现 tl;dv、Rimo、Hedy 等竞品发布的 Granola 内容；其中有可复查样例，但利益冲突明显。本报告没有采用其“最佳/最差”、准确率排行或总体优劣结论。部分 affiliate 长测存在“Mac-only”“local audio”与官方时间线/处理方式冲突，未用于当前能力判断。

## 6. 对 hear 的产品启示与优先验证顺序

以下是 Granola 单产品视角下的研究推断，不是其功能事实，也不是已经验证的 hear 用户需求；综合投入顺序以[主报告](../2026-10-23-hear-context-consumption.md)为准。它们基于“hear 持续自动收集电脑环境音频”的定位；具体实现仍需结合主报告的代码事实。

### 第一优先：可相信地找回一次具体表达

值得验证的一类问题是“刚才他说的那个名字”“上周为什么取消方案 B”“我答应周五交什么”。hear 可提供语义找回，结果直接到**时间、场景、原句与可播放片段**；不能确定人、日期或采集覆盖时，应呈现缺口。这里保留原音可能构成对 Granola 的实质差异：它只有文字可查，hear 若已有原音，就能让用户听回语气和模糊词。

原音回放不是要求用户重新听一小时；应当从一句回答直接跳到十几秒证据。衡量指标应是“找对并确认所需时间”“能否确认出处”“未找到时是否诚实”，而不是向量检索返回数量。

### 第二优先：把选中的上下文直接变成正在做的产物

可先做少量明确按钮：“给我一份跟进草稿”“整理成可执行的开发上下文”“提炼这次访谈的需求与证据”“把这段内容交给我正在用的 AI”。用户选会话/时间段/项目范围，产物区分**已决定、建议、待确认**，保留来源；常用做法再保存为个人配方。

这吸收了 Recipes 的价值，但不要求 hear 先运营一个提示词商店。早期更重要的是几个成品是否真的能直接用于工作，以及用户改了多少、复制/发送/交给 AI 后是否继续完成任务。对已有 AI 工作流用户，稳定的 context export/MCP 可能比再造一个全能 Chat 更有杠杆。[G17、G20、G22 提供需求线索]

### 第三优先：在“继续同一件事”的时刻给极短前情

Granola Briefs 的强处是触发自然、内容短、读完即消失。hear 可以验证“继续项目”或“再次与某人讨论”的前情卡：上次停在哪、已做了什么决定、哪些问题未闭环、这次最值得确认什么。并不需要一开始接 Gmail 和全网资料；先让自己的音频上下文被可靠复用。

注意音频里“有人提出”不是“本人承诺”；“没有听到后来完成”也不是“至今未完成”。所以开放事项应显示最近证据和状态未确认，避免把全天捕获的随口话变成硬任务。

### 需要先补的基础：会话边界、人的重点与来源层级

Granola 有日历和主动点击帮助确定“这是一场什么会议”。hear 的全日采集反而更缺稳定边界：什么时候是一场会议、一个临时讨论、一段媒体播放或自我口述？在正确分组之前，大范围问答容易把邻近但无关的人、承诺和项目混合。

可借鉴“人的线索”而非强迫写笔记：轻点一下标记重点、在某段后补一句意图、修正标题和归属。人的少量输入用来表达“这对我为什么重要”。同时把**原始片段 → 会话记录 → 按项目聚合 → 工作产物**串起来，允许从任何高层结论退回原始证据。

Granola 工程团队公开说明，在成本与时延约束下常选择广看更多会议的摘要，再处理问题，而非把少量长 transcript 全塞进上下文；他们更担心“错误却自信”。对 hear 来说，跨日范围更大，应把“广泛找候选”和“对具体决定核对原文”分开设计，别让摘要压缩永久丢掉证据。[G14]

### 不宜直接照搬的方向

| 方向 | 不宜现在照搬的原因 | 更小的验证方式 |
| --- | --- | --- |
| 通用聊天主页成为唯一入口 | 用户要先猜“这些音频能做什么”，回忆负担又回到人身上 | 会话结束/选中片段时提供两三个明确产物入口 |
| 每天推送长篇全天总结 | 全时采集噪声比会议更高；生成容易，阅读收益不确定 | 用用户主动关心的项目生成“新增决定/待确认变化” |
| 大而全的 CRM、团队 spaces、任务管理 | hear 当前定位不天然要求企业多人权限与销售流程；完整产品面会迅速扩大 | 先做单人可验证草稿和稳定出口 |
| 泛化“人生教练/人格分析” | 目前主要看到营销配方，缺乏持续价值证据；音频采样也不代表完整个人 | 只围绕用户主动选中的具体沟通场景复盘，并引用观察依据 |
| 先追求完全自动对外执行 | 捕获内容尚可能有角色/意图/完成状态歧义 | 生成可审查结果，明确目标应用和对象，再由用户完成动作 |
| 全屏持续截图作为默认扩张 | Granola 的截图也是针对共享材料、有清楚场景边界；不证明全天录屏值得做 | 先验证听觉缺失的“这个表/这张图”是否高频阻断理解 |

## 7. 来源索引与审计说明

证据等级：**A** = 官方当前操作/技术文档（可确认功能契约，不代表使用成效）；**B** = 官方发布/案例（可确认发布时间或意图，价值主张有商业偏差）；**C** = 作者自述的第一手使用/讨论（小样本，无法独立复现）；**C-aff** = 第一手作者但有 affiliate 披露。所有链接本次均打开正文核验；访问日均为 **2026-10-23**。文档未注明更新日时不推断上线日；网页搜索摘要的相对日期不覆盖正文日期。

| ID | 来源与链接 | 发表/更新日期 | 等级 | 本次记录的关键证据 |
| --- | --- | --- | --- | --- |
| G01 | [How transcription works](https://docs.granola.ai/help-center/taking-notes/transcription) | 未标明 | A | 开始条件、录音状态、云转写、不留原音、speaker tags；连轴会议可能合并，需人工 stop |
| G02 | [AI-enhanced notes](https://docs.granola.ai/help-center/taking-notes/ai-enhanced-notes) | 未标明 | A | raw notes + transcript + calendar，来源放大镜，编辑/再生成，不跨会学习编辑 |
| G03 | [Customize notes with templates](https://docs.granola.ai/help-center/taking-notes/customise-notes-with-templates) | 未标明 | A | 桌面 `/` 模板入口、结构与重点、重生成和分享边界 |
| G04 | [Chatting with your meetings](https://docs.granola.ai/help-center/getting-more-from-your-notes/chatting-with-your-meetings) | 未标明 | A | 单会/主页/folder/多选范围，新版历史权限；旧 help URL 已跳到此页 |
| G05 | [Recipes 帮助](https://docs.granola.ai/help-center/getting-more-from-your-notes/recipes)；[Recipes 发布](https://www.granola.ai/blog/say-hello-to-recipes) | 帮助未标；发布 2025-09-30 | A / B | `/` 调用、保存/分享/remix；专家 prompts、PRD/coaching 等是厂商场景 |
| G06 | [Spaces & Folders](https://docs.granola.ai/help-center/sharing/folders/spaces-and-folders) | 未标明 | A | 私人/团队空间、一层子目录、周期自动归档、multi-folder |
| G07 | [People and Companies](https://docs.granola.ai/help-center/people-and-companies) | 未标明 | A | 日历参与人自动聚合，有对象级 Chat；不能手工补建 |
| G08 | [Pre-meeting briefs 帮助](https://docs.granola.ai/help-center/taking-notes/pre-meeting-briefs)；[Briefs 发布](https://www.granola.ai/blog/briefs-prepare-you-for-your-next-meeting-as-you-join) | 帮助未标；发布 2026-05-20（年由 blog 索引确认） | A / B | 夜间准备、2–3 点、打开笔记即见；Business/Enterprise + Google Calendar；Gmail 可选 |
| G09 | [Follow-up emails](https://docs.granola.ai/help-center/taking-notes/follow-up-emails) | 未标明；G15 2026-08 已提及 | A | 外部会议自动草稿的触发、套餐/账号条件、编辑发送及不生成的情形 |
| G10 | [Workflows in Granola Chat](https://docs.granola.ai/help-center/getting-more-from-your-notes/workflows-in-chat) | 未标明 | A | Gmail/Slack/Google Calendar 的草稿→检查→确认动作；与自动建议区分 |
| G11 | [Granola MCP 帮助](https://docs.granola.ai/help-center/sharing/integrations/mcp)；[MCP 发布](https://www.granola.ai/blog/granola-mcp) | 帮助未标；发布 2026-02-04（年由 blog 索引确认） | A / B | OAuth、工具清单、active workspace、套餐/企业权限；发布用例不等于内建所有执行能力 |
| G12 | [Integrations](https://docs.granola.ai/help-center/sharing/integrations/integrations-with-granola)；[Exporting notes](https://docs.granola.ai/help-center/sharing/exporting-notes) | 未标明 | A | 原工作工具出口；CSV 包含完整个人历史但有摘要/所有权条件 |
| G13 | [Shared-screen snapshots](https://docs.granola.ai/help-center/taking-notes/capture-shared-screens)；[官方 blog 索引](https://www.granola.ai/blog) | 帮助未标；索引相关工程文 2026-10-06 | A / B | 针对对方共享屏幕，Mac 限定、权限、可关闭、用于增强笔记 |
| G14 | [How Granola thinks about designing agents](https://www.granola.ai/blog/how-granola-thinks-about-designing-agents)；[Chat 更新](https://www.granola.ai/blog/granola-chat-just-got-smarter) | 2026-05-06 / 2026-04-21（年由 blog 索引确认） | A / B | 广度与 transcript 细节取舍、诚实不完整、引用及 agentic Chat |
| G15 | [How sales teams use Granola](https://www.granola.ai/blog/how-sales-teams-use-granola) | 2026-08-17（年由 blog 索引确认） | B | 厂商自己的销售团队使用 Briefs/Follow-up/discovery Recipe，不是独立成效调查 |
| G16 | [Celine Wee：Granola AI Notepad](https://celinewee.medium.com/granola-ai-notepad-177e86eb8d3c) | 2025-02-15 | C | 人的笔记与 AI 补全、非美式口音、界面；也提出节省时间是否真实的疑问 |
| G17 | [Zachary Proser：My Daily-Use Verdict](https://zackproser.com/blog/granola-ai-review) | 2026-08-02 | C-aff | 数月使用自述；虚构样例的真实界面演示；承认无音频/导入的适用边界 |
| G18 | [英国 App Store 用户评价页](https://apps.apple.com/gb/app/granola-ai-meeting-notes/id6739429409?platform=iphone&see-all=reviews) | 多条；m571 2025-05-22；部分仅月日 | C | 日常 1:1/电话、brain dump、组织与导出诉求；平台选择偏差明显 |
| G19 | [Reddit：Granola AI is still the best regarding meeting notes?](https://www.reddit.com/r/ArtificialInteligence/comments/1r05e7g/granola_ai_is_still_the_best_regarding_meeting/) | 主题搜索元数据 2026-02-09；评论相对时间 | C | 静默失败、漏记自述；有相反体验和大量竞品自荐，不能统计故障率 |
| G20 | [Reddit：Streamlining platforms, notes and actions](https://www.reddit.com/r/productivity/comments/1p9ohzy/streamlining_platforms_notes_and_actions/) | 搜索元数据 2025-11-29；正文相对时间 | C | Granola→Bear→Asana 的重复搬运负担 |
| G21 | [HN：eawgewag 的共享抱怨](https://news.ycombinator.com/item?id=44736878) | 2025-07-30 | C | 文件夹共享预期落差；不是当前默认设置的独立审计 |
| G22 | [HN：Best meeting transcription daemon for macOS?](https://news.ycombinator.com/item?id=48936123) | 仅相对时间约 3 个月前，未推断精确日期 | C | Obsidian 同步作者需要后台采集、本地数据、可编程出口；接口破坏影响留存 |

### 尚未解决的证据缺口

- 没有测量中文、多人重叠、嘈杂环境、长时间运行的识别或总结质量；不能把英文知识工作者反馈直接推广给 hear。
- 没有独立核实 Briefs/自动 Follow-up 的草稿采纳率、长期使用频次或工作节省量；官方功能存在与功能值得照搬是两个问题。
- 没有对官方 API/webhooks/全部 CRM 集成逐一执行；此处确认入口与文档，不保证每条账户/套餐组合的实时可用性。
- 没有把公开帖子中的所有失败都归因到模型；采集权限、开始条件、软件版本、会话切分与后续同步同样可能解释现象。
- 没有采用“记住所有会议”的营销说法推断完整覆盖。对 hear 更关键的后续实测是：从哪段原音到哪条结论、遗漏何处、如何把用户带回可验证依据。

[G01]: https://docs.granola.ai/help-center/taking-notes/transcription
[G02]: https://docs.granola.ai/help-center/taking-notes/ai-enhanced-notes
[G03]: https://docs.granola.ai/help-center/taking-notes/customise-notes-with-templates
[G04]: https://docs.granola.ai/help-center/getting-more-from-your-notes/chatting-with-your-meetings
[G05]: https://docs.granola.ai/help-center/getting-more-from-your-notes/recipes
[G06]: https://docs.granola.ai/help-center/sharing/folders/spaces-and-folders
[G07]: https://docs.granola.ai/help-center/people-and-companies
[G08]: https://docs.granola.ai/help-center/taking-notes/pre-meeting-briefs
[G09]: https://docs.granola.ai/help-center/taking-notes/follow-up-emails
[G10]: https://docs.granola.ai/help-center/getting-more-from-your-notes/workflows-in-chat
[G11]: https://docs.granola.ai/help-center/sharing/integrations/mcp
[G12]: https://docs.granola.ai/help-center/sharing/integrations/integrations-with-granola
[G13]: https://docs.granola.ai/help-center/taking-notes/capture-shared-screens
[G14]: https://www.granola.ai/blog/how-granola-thinks-about-designing-agents
[G15]: https://www.granola.ai/blog/how-sales-teams-use-granola
[G16]: https://celinewee.medium.com/granola-ai-notepad-177e86eb8d3c
[G17]: https://zackproser.com/blog/granola-ai-review
[G18]: https://apps.apple.com/gb/app/granola-ai-meeting-notes/id6739429409?platform=iphone&see-all=reviews
[G19]: https://www.reddit.com/r/ArtificialInteligence/comments/1r05e7g/granola_ai_is_still_the_best_regarding_meeting/
[G20]: https://www.reddit.com/r/productivity/comments/1p9ohzy/streamlining_platforms_notes_and_actions/
[G21]: https://news.ycombinator.com/item?id=44736878
[G22]: https://news.ycombinator.com/item?id=48936123
