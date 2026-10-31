# Plaud：从录音归档到可复用上下文的产品研究

[返回主报告](../2026-10-23-hear-context-consumption.md)

研究日：2026-10-23（Asia/Shanghai）。所有链接访问日期均为 2026-10-23。本文核验公开文档、实际体验文章和用户讨论；没有登录付费账户或亲测设备。结论服务于 hear 的“随电脑持续收集环境音频”定位，不是购买建议。

## 1. 最重要的判断

1. **Plaud 的价值已经超出“硬件 + 转写摘要”。** 它把录音做成可询问、可多次加工、可向现有工作工具交付的资料库。2026-10-22 发布的 Plaud Intelligence / App 4.0 更进一步加入 Work、Artifacts、Routines 和 Connectors。官方说正在分批推出，不能把这些新能力等同于已被长期验证的体验。[P01][P03]
2. **有重复定性证据的价值，是减少会后加工，而不只是减少记笔记。** 用户需要的是按职责分组的行动项、可发给学生的课程补充、访谈 Q&A、口述成稿、固定格式的总结和自动投递。不同来源描述了具体产物，证据比“拥有第二大脑”的抽象宣传更扎实。[P16][P17][P20][P22][P25]
3. **全天收集后再随意提问，并不自动产生可信记忆。** 一位持续记录个人项目的用户给出了跨日查询漏项的具体反例；另一销售团队用户发现模板混淆自己公司与客户的背景。它们只能证明存在这些失败模式，不能证明发生率，也不能直接推断 4.0 仍有相同问题。[P21][P23]
4. **可配置的确定性流程本身有价值。** 4.0 上线首批用户不满的一个具体点，是原本固定的“转写 → 自选模板 → 指定附件邮件”被改成需要 Credits 的 Routines。新功能更多，不一定使已有工作更省事；这是一条产品设计预警，远非“4.0 已失败”的证据。[P08][P22]
5. **hear 不宜只把“桌面自动录音”当差异点。** Plaud Desktop 已支持电脑会议音频和无 bot 录制；其自动化围绕可检测会议，下载文档仍要求账户绑定 Plaud 设备。hear 值得验证的空间，是把没有明确会议边界的环境对话变成可查证事件，并以低整理成本送进用户已有工作流。[P13][P14][P15]（最后一句是研究推断）

## 2. 先处理版本事实，避免用旧体验描述新产品

| 时间与版本 | 可以确认的事实 | 解读边界 |
|---|---|---|
| 2025–2026 年的旧版体验 | 转写、摘要、模板、Ask Plaud、AutoFlow 是很多文章和帖子的讨论对象 | 旧文章中的“不支持集成”“只能单条导出”不能直接作为今天的能力结论 |
| 2026-10-20 帮助中心版本拆分 | 多篇旧文明确只适用于 v3.30.x 或更早，另链接到 v4.0.x 文档 | 搜索结果可能先命中旧文；需查看适用版本 |
| 2026-10-22 官方发布新 Plaud Intelligence | Chat / Work、Routines、Artifacts、Connectors、Memory 等被公开发布，分批推出 | 属于官方发布与操作文档证据；没有本研究的账户实测与长期留存证据 |
| 2026-10-22/23 上线初期社群反馈 | 有 Unlimited 用户投诉旧 AutoFlow 工作流和付费预期变化 | 只作升级摩擦样本；不能宣称普遍不满，更不能据此判定新产品成败 |

特别容易误读的三点：

- **“重做摘要收费”不是所有路径都收费。** 发布博客把 re-summarising 放在消耗 Credits 的项目里；4.0 模板帮助却明确从录音使用模板生成/替换摘要不消耗 Credits 或转写分钟。应按入口区分“模板处理”和“Work 任务”，不能简单写成摘要全收费。[P01][P04]
- **“外发一定逐次确认”有 Routine 例外。** 发布博客概括为发送前需要确认；连接器帮助进一步说明 Routine 的 Auto-approve 可以代为批准，而 Routine 帮助写明新建默认开启。具体帮助给出了例外，概览没有完整呈现。本研究未亲测开关默认值。[P01][P08][P09]
- **“无 API 所以数据被锁住”已经过时。** 旧 OAuth API 确实只返回最近 20 条且不开放新申请，但官方推荐 MCP / CLI 访问完整历史。MCP 是读取既有内容的接口，不能替用户新建转写或调用指定模板。[P11][P12]

## 3. 真实能力与操作路径

下表中的“已文档化”表示官方提供操作说明，并不表示本研究测量过质量、速度或可用率。

| 用户任务 | 已文档化的真实流程 | 关键边界与产品含义 |
|---|---|---|
| 会议结束后拿到结构化结果 | 录音同步、生成转写与摘要；模板规定要提取的栏目，如决定、行动项、待确认问题 | 结构化输出是把音频变成工作材料的第一层；生成建议不等于会议真实共识。[P04][P16] |
| 同一谈话从多个视角重看 | 4.0 Web 打开录音，`+ New note` 选择模板保留额外摘要；或从重新生成入口替换现有摘要 | 不是每种用途都要重录。一个源记录可以同时支持纪要、访谈报告、行动项等；模板使用本身不消耗 Credits。[P04] |
| 少量定制变为稳定默认 | 在 Tools → Templates 创建名称和 prompt，可收藏；自动摘要采用 Intelligent generation | “固定几种我真的需要的格式”比模板总数量更接近价值；4.0 自动摘要不等同于旧版每次自动使用指定模板。[P04][P22] |
| 在对话中告诉系统什么重要 | 录音面板可 Highlight、Type、Photo；Memo 和照片关联到对应转写片段；标记在正文/摘要中可见 | 这是用户意图注入：一句补充或一次标记，能告诉系统哪些内容值得保留。官方流程是录制时操作；无需假设系统能独立知道重要性。[P05] |
| 找回之前说过的内容 | 旧版全库 Ask 从已转写内容检索，并给录音来源；4.0 从底部 Ask 进入 Chat，可附文件或由系统查找 | 全库回答是否完整，仍须单独验证；来源引用与召回完整性是两个问题。[P02][P03][P21] |
| 限定一个客户或项目来问 | 4.0 Work 的 `+` 可添加单条、多条、文件夹、外部文件；文件夹限定搜索范围 | 官方承认缩小范围可加快并聚焦任务；Agent 当前不读取 Plaud Team workspace 文件。不要把个人库与团队库能力混写。[P10] |
| 从谈话直接形成可交付材料 | Work 产出 PDF、PPTX、DOCX、Markdown、HTML 等，在旁边画布预览，继续对话原地修改 | 官方已给流程；输出质量和修改可靠性缺少新版本长期实测。官方发布稿宣称产物内可追溯到转写时间点，不能当成经本研究验证的每条引用覆盖率。[P01][P06] |
| 重复执行同一加工方式 | Skill 保存提取要求、结构和格式；可由范例文档、对话、表单或既有产物创建，再审核保存 | 把一次满意结果固化为可复用流程是值得借鉴的入口；运行限 Work，使用 Credits。[P07] |
| 自动完成会后或定期整理 | Routine 将触发条件与动作连接；可设置计划时间或让 Work 创建规则；任务在 Tasks 查看 | 官方例子为转写后生成报告、归档并存 Notion；云端运行；Auto-approve 需理解其例外。没有证据证明所有复杂触发的执行可靠性。[P08] |
| 把结果送到工作真正发生处 | 授权后用 Gmail、Slack、Notion、Docs、Linear、日历或 Zapier；支持的读写操作按工具不同 | 连接器不是“所有工具都能任意操作”；如 Notion 可建页/增补/加数据库行，但不建数据库。Chat 对连接器的例外仅是读取 Google Calendar。[P09] |
| 继续用自己已有的 AI | 连接 Plaud MCP 后，外部助手可检索录音、读完整转写及全部已生成摘要、取音频临时链接 | 要启用 Cloud Sync 并先完成生成；只读；Team 中的个人区可读，移入 Team files 后暂不可读。免费是当前政策，并非永久承诺。[P11] |
| 电脑会议自动进入同一资料库 | Desktop 检测支持的会议，录电脑音频，同步到同一账户；设置中可开自动开始/停止 | 支持面因系统和应用而异；明确是“可检测会议”，不等同 24h 环境录音。文档仍列绑定硬件为下载前提。[P13][P14][P15] |

**关于 Memory：** 4.0 发布说明支持保存角色、工作风格和偏好，并允许用户查看、修改、移除。本研究只确认官方已宣布这项能力，没有把它等同于经验证的跨数月人物关系、承诺履约或事件状态记忆。[P01]

**关于 action items：** 旧版模板和用户实际使用已经能抽取行动项；4.0 又增加向其他工具交付的路径。本文没有找到足以确认“长期任务状态自动跟踪、自动判断完成、跨会话取消与改期都可靠”的证据。抽取一条任务、写入任务工具、长期维护承诺状态是三种不同能力。

## 4. 用户到底在消费什么：真实案例与反例

### 4.1 有具体工作产物的正面案例

| 场景与来源 | 实际消费方式 | 哪部分值得信任，哪部分不能推大 |
|---|---|---|
| 编辑会议与采访；Tom’s Guide 的记者实际测试 | 用 Meeting Notes 回收要点/行动点；用 Q&A 模板整理采访；另用自定义模板把口述变成文章初稿 | 有任务、输出和错例。采访样本漏了一组问答；会议建议误把未提及的已知截止期当缺失。说明结构化有价值，但建议应与事实分开。[P16] |
| 摄影教学；TechRadar 三周体验 | 课后用课程模板得到摘要和待办，再发给学生；特别重视计划外的问题和现场讨论发展 | 价值不是重写已有教案，而是保存课堂发生的增量。属一个评测者案例，且有导购佣金披露。[P17] |
| 负责多个项目和下属的管理者；Reddit | 对同一会议使用普通纪要、沟通教练、情绪理解三个自制模板；行动项按职责和谈话对象分组 | 比通用会议摘要更贴近实际管理工作。是单个自述，不证明情绪分析准确；应把可观察发言行为与模型解释分开。[P20] |
| 带孩子就医；法语 Reddit 用户 | 默认摘要不满意，改用四个简单个人 prompt；信息密集的问诊后用自定格式回看 | 证明“少量个人格式”可能比模板大全更有效。只是单一回忆记录案例，未核验医疗事实，不能推导诊疗可靠性。[P24] |
| 个人灵感、周回顾；少数派作者 | 手工导出转写进入 Obsidian，按周组织，n8n 分类成纪要、灵感、复盘，再接不同处理流程 | 直接显示用户愿意自行搭系统来消费录音。Plaud 是输入源，后续是作者自建能力；文章有另一款硬件的厂商供货披露，不当成独立市场统计。[P25] |
| 固定会后投递；4.0 首日 Reddit 反馈 | 旧 AutoFlow 自动生成个人模板摘要并把指定附件发送到固定邮箱 | 投递本身就是使用闭环：用户不必回来清理录音库。只有升级初期样本，不能据此推算规模或流失率。[P22] |

### 4.2 值得做产品设计输入的失败样本

**录得越多，不代表能完整回答。** 个人项目/日记用户积累约两个月记录后，要求列出音乐创作的时间线以及跨日情况，报告有已知片段未被找到；修改日期范围后才出现部分遗漏记录。这个反例的价值是可测量的“漏找”问题，而不是该用户对所有模板的主观否定。4.0 尚无相同查询的复测。[P21]

**摘要里的合理建议，可能来自错误前提。** 记者的编辑会议已经有默认截止时间，但模型因对话没说就建议制定期限。hear 若要生成“下一步”，应显示这是明确承诺、待确认提议，还是模型建议；不宜用同一种待办样式混合三者。[P16；后一句为研究推断]

**模板承担不了所有长期背景。** 销售团队把自己公司的定位、竞争者、方法论塞入模板后，用户报告这些背景被归给了被访客户。该案例支持“角色与来源归属必须显式分开”，不能据此宣称所有模板都会发生问题。4.0 Memory 是否解决这一点尚无验证。[P23]

**泛化摘要可能增加阅读量。** Wallpaper 的作者把家庭闲聊送入处理后，得到比原转写更冗长的总结和图示。对 hear 的环境音频，值得测试“没有值得总结的事件就不产生报告”，而不是把任何录音都强制格式化成会议。[P18；最后一句为研究推断]

**模型能力增加，可能破坏熟悉的确定性流程。** 4.0 初期投诉集中描述固定邮件投递、模板配置、附件与额外 Credits，而不是说“不要 AI”。可迁移的原则是保留可审计的规则层，把复杂分析作为可选能力；不是据此复制旧版本或否定 Agent。[P22；后半句为研究推断]

**中文与弱音频仍需自己验证。** 我爱音频网 2025 年测试认为中文转写总体可用，但提到较轻的声音可能“人能听见，转写没有”；这是一篇含实测照片的编辑体验，不能代表当前版本，也没有标准中文测评集。对长期 ambient 捕获，漏语音会沿链路传递成漏记忆。[P19；最后一句为研究推断]

### 4.3 可以怎样说“高价值、高频”

- **多来源重复的定性信号：** 结构化纪要/访谈、按实际工作格式产出、降低会后整理、把内容送入已有工具。在记者、教师、管理者和自建工作流作者中都出现，但没有随机抽样或行为日志。
- **有价值但样本较少：** 个人跨日记忆、就医回顾、沟通教练、灵感自动启动研究。值得访谈和做原型，不应直接称“大众高频刚需”。
- **频率证据不足：** “每天持续录音”的愿望与实际每天消费结果不能混为一谈。现有样本不足以判断日报、跨库问答、情绪分析哪项 DAU 更高；也不足以比较各功能留存贡献。
- **最接近可验证高频的切口：** 有规律参加会议/访谈且每次都需输出固定结果的人。原因是事件和交付重复出现，并已有具体自述；这是优先研究假设，不是已证实市场结论。

## 5. 对 hear 的方向建议：研究推断，不是已承诺功能

| 值得验证的功能 | 对应 Plaud 证据 | hear 的具体产品形态 | 首轮验证问题 |
|---|---|---|---|
| **可信的“刚才/上次说了什么”** | 全库提问已成为基础能力，但用户能遭遇漏项 [P02][P21] | 输入人名、主题或时间描述，返回短答案、候选事件、原话与音频位置；展示覆盖的日期和哪些片段未处理 | 用户能否在 30 秒内找回一条自己记得说过的事实；漏检比例是多少 |
| **事件结束后的小型交付卡** | 会后结构化与固定投递有具体价值 [P16][P20][P22] | 自动从长音频识别候选谈话边界，展示“决定 / 我的承诺 / 待确认”少量卡片；用户可更正或忽略 | 有多少卡片被实际采用、复制、发出，而不只是打开 |
| **按项目/人物的连续记录** | 用户要求时间线；销售背景混淆；官方引入文件夹范围 [P10][P21][P23] | 同一主题列“新决定、变化、冲突、未完承诺”，每项有来源；项目归属可修正，不默认为确定 | 跨三次谈话是否真的减少回顾时间；能否正确处理推翻旧决定 |
| **把语音变成正在做的工作** | 口述成稿与自建灵感流程 [P16][P25] | 选一段音频直接生成用户常用格式：访谈摘录、需求材料、文章草稿、研究问题；满意结果保存为规则 | 用户是否复用第二次、第三次，而不是只体验第一次 |
| **低成本的重要性输入** | Highlight / Memo / Photo [P05] | 全局快捷键标记“刚才重要”，可追加一句意图或关联当前项目；回溯一个短时间窗口 | 是否降低结果漏重点，同时不使用户重新负担完整记笔记 |
| **开放上下文出口** | MCP / CLI 已成 Plaud 正式能力 [P11][P12] | 可搜索的转写、事件摘要、时间戳和音频片段通过文件/接口交给用户已有 AI；先保持读取可控 | 用户能否免复制继续在已有工作环境使用；是否减少重复建设 |

建议排序：先验证“找回事实”和“谈话后可用产物”，再做跨事件连续记忆；模板/连接器作为服务这些场景的手段。**不要先把大型模板社区、泛化情绪评分或全能 Agent 平台当成下一阶段主体。** 这不是说它们无价值，而是本轮证据对前两类任务更具体，对后三类的持续消费和效果支持更弱。

hear 的 24h 特征同时意味着更多低信息音频。功能设计应有“无结果也是正确结果”的路径：背景视频、旁人闲聊、重复播放、无法分清说话人的片段，不应自动变成用户承诺或私人记忆。这里是由输入特性推导出的设计约束，不是声称 Plaud 已解决或未解决这些问题。

## 6. 来源登记

证据标记：**O-D**＝官方操作文档，可核验产品契约但不证明实际质量；**O-A**＝官方发布/宣传，需关注灰度与范围；**R**＝有具体任务的媒体体验，可能有供货/导购利益关系；**U**＝用户自述，身份、结果与代表性未独立核验。精确日期未显示时保留“不确定”，不把搜索引擎的相对日期强行换算成某一天。

| ID | 来源、链接 | 发布/更新日期 | 证据与本研究用途 |
|---|---|---|---|
| P01 | [Meet the new Plaud Intelligence](https://sea.plaud.ai/en-th/blogs/news/meet-new-plaud-intelligence) | 2026-10-22 | O-A；4.0 发布、分批推出、Memory、收费概览；不能证明稳定体验 |
| P02 | [Ask Across All Files](https://support.plaud.ai/hc/en-us/articles/50810294218137-Ask-Across-All-Files-Ask-Plaud) | 更新 2026-10-20 | O-D；旧版只搜已转写内容、来源引用、版本边界 |
| P03 | [Ask Plaud: Chat and Work modes — v4.0](https://support.plaud.ai/hc/en-us/articles/63429926029593-Ask-Plaud-Chat-and-Work-modes-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；模式、入口、任务、上下文；模式选定后需新会话切换 |
| P04 | [Summary templates on Plaud Web — v4.0](https://support.plaud.ai/hc/en-us/articles/63431072621081-Summary-templates-on-Plaud-Web-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；重做/保留多摘要、模板不耗 Credits、自定义 |
| P05 | [Highlight a moment while recording — v4.0](https://support.plaud.ai/hc/en-us/articles/63429919023385-Highlight-a-moment-while-recording-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；标记、文字、照片与片段绑定 |
| P06 | [Artifacts — v4.0](https://support.plaud.ai/hc/en-us/articles/63429954271513-Artifacts-documents-created-by-Plaud-Agent-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；产物格式、画布预览、对话修改 |
| P07 | [Skills — v4.0](https://support.plaud.ai/hc/en-us/articles/63429926356761-Skills-save-your-workflows-for-reuse-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；四类创建方式、保存/调用、Work 边界 |
| P08 | [Routines — v4.0](https://support.plaud.ai/hc/en-us/articles/63429939580697-Routines-run-tasks-automatically-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；触发、计划、任务记录、Auto-approve 默认值 |
| P09 | [Connectors and permissions — v4.0](https://support.plaud.ai/hc/en-us/articles/63307564140825-Connectors-and-permissions-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；各工具读写范围、权限与 Routine 例外 |
| P10 | [Choose the right context — v4.0](https://support.plaud.ai/hc/en-us/articles/63429922328217-Choose-the-right-context-for-an-Agent-task-for-v4-0-x-or-later) | 更新 2026-10-20 | O-D；文件/文件夹/外部输入、个人库与 Team 边界 |
| P11 | [Plaud MCP](https://support.plaud.ai/hc/en-us/articles/57751078986265-Plaud-MCP) | 更新 2026-10-14 | O-D；可读内容、只读限制、Cloud Sync、音频链接、Team 范围 |
| P12 | [Why doesn't the Plaud API return all recordings?](https://support.plaud.ai/hc/en-us/articles/59291709269401-Why-doesn-t-the-Plaud-API-return-all-of-my-recordings) | 更新 2026-08-03 | O-D；旧 API 最近 20 条；MCP / CLI 没有此限制 |
| P13 | [Download the Plaud Desktop](https://support.plaud.ai/hc/en-us/articles/53792807283225-Download-the-Plaud-Desktop) | 更新 2026-08-27 | O-D；macOS/Windows、无 bot、绑定设备前提 |
| P14 | [Before you start: Plaud Desktop FAQ](https://support.plaud.ai/hc/en-us/articles/58739679361305-Before-you-start-Plaud-Desktop-FAQ) | 更新 2026-09-07 | O-D；线上/线下一库、支持会议、输出；混合语言限制仅为该文档当前说法 |
| P15 | [Automatic Recording](https://support.plaud.ai/hc/en-us/articles/53793036250521-Automatic-Recording) | 更新 2026-10-14 | O-D；可检测会议的自动开始/结束及停止控制 |
| P16 | [Tom’s Guide: Plaud Note review](https://www.tomsguide.com/ai/plaud-note-review) | 2025-01-28 | R；记者实际编辑会议、采访 Q&A、口述初稿；披露导购佣金；旧模型/旧版本 |
| P17 | [TechRadar: Plaud NotePin review](https://www.techradar.com/ai-platforms-assistants/claude/plaud-notepin-bundle-review) | 2025-09-14 | R；三周使用与课堂产物；披露导购佣金；旧“不支持第三方集成”已不代表现状 |
| P18 | [Wallpaper: Plaud Note Pro review](https://www.wallpaper.com/tech/plaud-note-pro-review) | 2025-08-27 | R；家庭对话生成冗长输出的反例；披露导购佣金；与 P16/P17 同属 Future 媒体集团，不能当三个完全独立机构 |
| P19 | [我爱音频网：PLAUD NOTE 评测](https://www.52audio.com/archives/247661.html) | 2025-07-28 | R；中文转写、弱音频、同步/处理实测；产品宣传内容较多，未见标准化识别率数据 |
| P20 | [Reddit: meeting insights in 3D](https://www.reddit.com/r/PLAUDAI/comments/1q5aoox/it_feels_like_my_meeting_insights_are_now_in_3d/) | 精确日未核验；页面显示约 9 个月前 | U；多模板、按管理职责整理行动项、个人沟通复盘 |
| P21 | [Reddit: Thoughts on the Future of Plaud](https://www.reddit.com/r/PLAUDAI/comments/1rk48nb/thoughts_on_the_future_of_plaud/) | 精确日未核验；正文相对日期约 8 个月前 | U；使用约两个月后跨日召回漏项；旧版数据出口抱怨已部分过时 |
| P22 | [Reddit: Plaud 4.0 / AutoFlow downgrade](https://www.reddit.com/r/PlaudNoteUsers/comments/1xbjrll/plaud_40_is_a_major_downgrade_autoflow_removed/) | 2026-10-22/23 上线初期；索引与正文相对时间不一致 | U；固定投递与升级付费预期；仅首批/灰度样本，不代表成熟口碑 |
| P23 | [Reddit: Am I asking too much of Plaud?](https://www.reddit.com/r/PLAUDAI/comments/1v8d4vr/am_i_asking_too_much_of_plaud/) | 精确日未核验；页面约 2 个月前 | U；销售团队把长期背景塞入模板后出现角色混淆 |
| P24 | [Reddit: Plaud Note is very practical](https://www.reddit.com/r/PlaudNoteUsers/comments/1pt131y/plaud_note_is_very_practical/) | 精确日未核验；索引/正文约 9–10 个月前 | U；四个个人 prompt、孩子就医回顾；默认法语摘要不满意 |
| P25 | [少数派：「咔哒」一声，是 AI 时代的松弛感](https://sspai.com/post/105801) | 2026-02-02 | U/R；作者自建 Plaud → Obsidian → n8n 消费链；文章披露 Mind2 厂商供货；不验证其第三方 API 说法 |

## 7. 检索覆盖、排除与仍待验证的事

检索路径覆盖：官方产品发布与帮助中心、Release Notes、Reddit 两个 Plaud 社区、英文长评、中文我爱音频网与少数派；关键词包括 Ask Plaud、cross-file、templates、highlights、MCP、AutoFlow、4.0、workflow、长期使用、中文体验。查询 YouTube 和小红书可公开索引内容后，未取得足以核验视频全过程/长文的样本，因此没有假装完成平台内部系统抽样，也没有将搜索片段当完整观看证据。

有意降低权重或排除：

- [I tried every way to process Plaud transcripts](https://www.reddit.com/r/PLAUDAI/comments/1rmlhwa/i_tried_every_way_to_process_plaud_transcripts/) 有工作流细节，但作者在评论导流自己的教程，且关于模型能力/上下文的泛化说法不严谨。保留为寻找用户工作流的线索，没有采用其“某模型总是更可靠”的结论，也不把它与引用同一作者的其他贴重复计算。
- [官方 Release Notes](https://sea.plaud.ai/en-th/pages/plaud-release-note) 检索页出现重复 Desktop 条目及异常重复日期；本报告改用有明确更新时间的功能帮助核实，不据此断言 Desktop 最初发布日。
- 没有把模板数量、App Store 总评分、销量、厂商列出的行业案例，当成某个 LLM 功能的留存或使用频率证据。
- 没有依据旧媒体评测来宣称当前订阅价格、当前准确率或 4.0 已解决/仍有某个问题；没有做实时成本横评。

下一轮最值钱的验证不是再收集更多功能清单，而是拿同一批跨多天中文录音做任务测试：事实查找、遗漏恢复、人物归属、撤回/改期、少量重点标记、跨事件输出与下游复用。对 4.0 还应实测：模板与 Work 的收费入口、Routine 迁移、Auto-approve 默认状态、来源引用能否精确回放，以及不同账户是否已经获得新功能。

<!-- 来源引用定义 -->

[P01]: https://sea.plaud.ai/en-th/blogs/news/meet-new-plaud-intelligence
[P02]: https://support.plaud.ai/hc/en-us/articles/50810294218137-Ask-Across-All-Files-Ask-Plaud
[P03]: https://support.plaud.ai/hc/en-us/articles/63429926029593-Ask-Plaud-Chat-and-Work-modes-for-v4-0-x-or-later
[P04]: https://support.plaud.ai/hc/en-us/articles/63431072621081-Summary-templates-on-Plaud-Web-for-v4-0-x-or-later
[P05]: https://support.plaud.ai/hc/en-us/articles/63429919023385-Highlight-a-moment-while-recording-for-v4-0-x-or-later
[P06]: https://support.plaud.ai/hc/en-us/articles/63429954271513-Artifacts-documents-created-by-Plaud-Agent-for-v4-0-x-or-later
[P07]: https://support.plaud.ai/hc/en-us/articles/63429926356761-Skills-save-your-workflows-for-reuse-for-v4-0-x-or-later
[P08]: https://support.plaud.ai/hc/en-us/articles/63429939580697-Routines-run-tasks-automatically-for-v4-0-x-or-later
[P09]: https://support.plaud.ai/hc/en-us/articles/63307564140825-Connectors-and-permissions-for-v4-0-x-or-later
[P10]: https://support.plaud.ai/hc/en-us/articles/63429922328217-Choose-the-right-context-for-an-Agent-task-for-v4-0-x-or-later
[P11]: https://support.plaud.ai/hc/en-us/articles/57751078986265-Plaud-MCP
[P12]: https://support.plaud.ai/hc/en-us/articles/59291709269401-Why-doesn-t-the-Plaud-API-return-all-of-my-recordings
[P13]: https://support.plaud.ai/hc/en-us/articles/53792807283225-Download-the-Plaud-Desktop
[P14]: https://support.plaud.ai/hc/en-us/articles/58739679361305-Before-you-start-Plaud-Desktop-FAQ
[P15]: https://support.plaud.ai/hc/en-us/articles/53793036250521-Automatic-Recording
[P16]: https://www.tomsguide.com/ai/plaud-note-review
[P17]: https://www.techradar.com/ai-platforms-assistants/claude/plaud-notepin-bundle-review
[P18]: https://www.wallpaper.com/tech/plaud-note-pro-review
[P19]: https://www.52audio.com/archives/247661.html
[P20]: https://www.reddit.com/r/PLAUDAI/comments/1q5aoox/it_feels_like_my_meeting_insights_are_now_in_3d/
[P21]: https://www.reddit.com/r/PLAUDAI/comments/1rk48nb/thoughts_on_the_future_of_plaud/
[P22]: https://www.reddit.com/r/PlaudNoteUsers/comments/1xbjrll/plaud_40_is_a_major_downgrade_autoflow_removed/
[P23]: https://www.reddit.com/r/PLAUDAI/comments/1v8d4vr/am_i_asking_too_much_of_plaud/
[P24]: https://www.reddit.com/r/PlaudNoteUsers/comments/1pt131y/plaud_note_is_very_practical/
[P25]: https://sspai.com/post/105801
