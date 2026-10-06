# 核心处理算法与任务工作流

> 本文描述 2026-09-30 工作树的代码路径。它记录“现有实现如何判断”，不把原型阈值写成永远不变的产品规则。总览见[架构](../ARCHITECTURE.md)，人物识别的详细算法见[说话人和全局人物](speaker-identity.md)。源码含未提交的 Batch 6 修改；安装版与质量验收状态见 [plan.md](../../plan.md)。

## 1. 两层音频处理与虚拟事件

真实轴由 `source + recording_session + sample_start + [started_ms, ended_ms)` 组成。麦克风和电脑声音各自连续推进，可以并行。原声封块及粗转是**发生过什么**；候选、命名事件、未整理块是**如何组织它**。候选不是事件，精转任务也不是事件。任何处理都不能倒改采集时钟或把两段之间的静默时间抹掉。

```text
设备回调 → 真实时刻/来源 → 约 5 秒封块 → 原声落盘
                                    ↘ 保留判定
                                    ↘ VAD → 本地粗转 → 候选区间
                                                       ↘ LLM 拟合 → 命名事件 → ASR
未被命名事件覆盖的有效时间轴 ──────────────────────────→ 未整理块
人工圈选连续区间 ───────────────────────────────────→ 独立未命名精转事件 → ASR
```

### 不变量

- 原声音频有文件内相对偏移；产品对象有真实 UTC 毫秒。供应商相对时间只有通过任务输入的真实时间映射后才能入库。
- 某时间段没有文件可以是**确认静音/无人声而省写**，也可以是**采集失败缺口**；两者分开记录。无文本不等于无人声。
- 事件范围可以包含大量没有句子的静默间隔。一场两小时会议应是一个大块虚拟范围，而非若干发言片段拼凑的列表。
- LLM 的密度/语义决定是否形成命名事件和自动精转，不决定底层原声留存。手动请求可直接走 ASR，不依赖新的 LLM 判断。

## 2. 采集、封块与时钟

每路录音由 [`record.rs`](../../crates/hear-core/src/record.rs) 独立组装。回调带单调时刻和墙钟锚点；核心把输入混为单声道、重采样到 48 kHz，再按 `BLOCK_FRAMES = 48_000 × 5` 封成约五秒 Opus 块。连续文件封口、媒体持久化和 SQLite 清单使用固定顺序；回调队列溢出、采样率突变、顺序倒退、长时间无回调等都会中止该来源并记录缺口，不继续声称完整录制。

`ClockAnchor` 将来源内采样位置映射到真实毫秒。中断后另起 session；大于 250 ms 的回调空档被写成 `callback_gap`，首包明显晚于启动意图写 `source_start_gap`。来源失败的重试由 [`capture_supervisor`](../../crates/hear-core/src/main.rs) 管理，两路独立；睡眠/唤醒与设备异常不会把采样序号和文件时长硬接成“连续音频”。当前重试参数和来源实现应直接看代码，仍需真实故障回归。

## 3. 独立声音保留判断

[`retention.rs`](../../crates/hear-core/src/retention.rs) 是单独策略边界，不读取 LLM 事件价值或粗转文本质量。**当前工作树含尚未安装版验收的 `pending_vad_v2` 候选：**

1. 全零数字静音直接记 `discard_digital_silence_v1`，不编码原声；非零音频先封口持久化，记 `pending_vad_v2`。
2. 本地分析回写该封块的 `speech_active` 和粗转句段后，策略查看当前块以及相邻上一块的时间关系。有人声检测或有时间重叠的粗转句段则转为 `retain_speech_v2`。
3. 确认无人声时先在数据库撤销媒体引用、持久登记 `media_deletion_jobs`，再由后台删物理文件，状态为 `discard_no_speech_v2`。靠近当前分析边界的一块暂缓决定，避免晚到的 VAD 句段丢失句首。
4. 分析尚未完成、工作进程异常或判定仍待定时，原声保持可追踪的待判定状态。策略升级不回溯删除旧版已保留原声。

这段代码回应了“不保留无说话环境噪音”的产品选择，但**实际误删/误留率、弱语音和安装版落盘效果尚未验收**。不能仅凭单元测试宣布全天声音保留正确。

## 4. VAD 与本地粗转录

[`analysis_loop`](../../crates/hear-core/src/main.rs) 从已封口待分析块读取音频，经 [`AnalysisClient`](../../crates/hear-core/src/analysis.rs) 重采样到 16 kHz，按来源与采样号送给独立常驻 [`hear-worker`](../../crates/hear-worker/src/main.rs)。worker 用 **Silero VAD** 检测声音，再对收束的语音窗调用 **SenseVoice INT8** 本地粗转。VAD 以 512 样本小窗输入；两路各有会话状态。非连续采样号或 `flush` 会重建/结束对应状态，防止断流两侧被误连。

当前 worker 为持续有声设约 20 秒主动 flush，避免仅靠 VAD 的 `max_speech_duration=30s` 误以为已有硬上限。结果是 `start_sample/end_sample + text + speech_active`，核心将样本位置转为真实时刻、来源和模型版本后写 `coarse_segments`。粗转可以错字、缺字或空文字；人声检测和音频事实仍独立存在。worker 不直接写数据库，变慢时由核心持久待办追赶，录音热路径不等待它。

## 5. 从粗转到候选，再到事件

### 5.1 候选粗切

[`events::evidence`](../../crates/hear-core/src/events.rs) 组合两类证据：已有粗转句段，以及检测到人声却没有可用粗文字的块。候选生成器按真实时间排序；当前 `candidate-v1` 的**策略参数**是“相邻证据相隔超过 15 分钟”或“候选跨度超过 30 分钟”时新开计算窗。已清除全部数据的区间也可成为切点。参数只控制送 LLM 的计算批次，不强制最终事件在这里断开。[`build_candidates_with_cuts`](../../crates/hear-core/src/events.rs)。

候选输入的 ID、时间、来源与文字形成哈希版本。晚到的本地分析可改变候选；大幅重叠时复用既有候选身份，旧版本云任务失效。扫描有持久游标和最近两小时回看，不因应用停机很久就永久跳过旧声音。[`refresh_candidates_in`](../../crates/hear-core/src/events.rs)。

### 5.2 LLM 拟合

LLM 收到粗文字、真实时间、来源、人声但无文字的标记，以及邻近上一命名事件的有限上下文。固定系统约束要求输出 JSON 事件范围、标题、来源及是否延续上一事件；持续会议应保持大范围，零散低密度句子留在未整理。模型输出只是**候选提案**。[`llm_input`、`FIT_SYSTEM`](../../crates/hear-core/src/events.rs)。

核心在 [`publish_fit`](../../crates/hear-core/src/events.rs) 检查 schema、数量、范围、来源证据、同源事件不重叠、前一个事件 ID、清理范围及任务版本；有效命名事件才进入当前事件轴并排队精转。人工修改边界/标题优先于迟到 LLM 返回。被跳过的候选内容仍在未整理时间块和底层粗转中。

### 5.3 时间线投影与人工边界

[`timeline`](../../crates/hear-core/src/events.rs) 以当天真实时间范围查询命名事件，再按来源从有效素材范围减去已被命名覆盖的范围，形成未整理块；静默间隔可以包含在大块中。用户对未整理区间手动精转时，`manual_ranges` 为该**连续真实时间范围**建立独立、可追踪的未命名精转事件，前后剩余部分继续显示粗转未整理块。手动新建/调整命名事件会更新 ASR 待办，不改写原声。日期列表的“隐藏粗转”仅影响 UI 投影，不删除数据。[`transcribe_range_in`、`timeline`](../../crates/hear-core/src/events.rs)；[日期界面](../../src/TimelineView.tsx)。

## 6. 两条云队列与精转

LLM/ASR 各有配置、暂停开关和持久 `cloud_tasks`，可单独继续。取任务时手动任务优先；任务绑定 `target_kind + target_id + target_version`，发布前再次确认仍指向当前事件/范围。[`take_task` / `task_current`](../../crates/hear-core/src/events.rs)。暂停队列不影响采集和粗转；已经外发的请求可以完成并经版本校验保存。

ASR 默认由 LLM 命名且判断值得精转的事件创建；人工建事件、改需精转范围、明确“重新精转”或“选择范围精转”可直接建 ASR 待办。当前同一来源在保留原声之间超过 3 秒的静音处分开物理任务，连续范围仍以最长约 15 分钟为上限，两路分别建任务；一个虚拟事件可关联多个物理请求。整段输入由已保存 Opus 块按真实时间重组，间隙编码为静音；以有界 10 秒 slab 拼 Ogg/Opus，避免一次把整场 PCM 留在内存。临时文件在正常/崩溃恢复时清理，单请求有大小上限。[`queue_asr_with_processor` / `assemble_ogg`](../../crates/hear-core/src/events.rs)。

火山标准版使用 `submit/query`、固定资源 `volc.seedasr.auc`，请求 `enable_speaker_info`、`show_utterances` 与当前 `ssd_version=300`。物理任务 ID 也是远端请求去重 ID；未知提交结果应先查询而非直接付费重送。[`cloud.rs`](../../crates/hear-core/src/cloud.rs)。若供应商把长停顿合为一句，适配器仅在词列表完整、文字可顺序对齐且时刻有效时按超过 3 秒的词间停顿拆句，保留原文、标点和 speaker；不完整/无效的词元数据保持原句交由严格覆盖校验，零时长重复句仍拒绝。供应商 `utterances` 的相对毫秒通过任务真实起点映射，并核验区间确有对应编码输入后，作为精转事实存入 `precise_segments`；speaker 标签的作用域是物理任务 ID。[`publish_asr`](../../crates/hear-core/src/events.rs)。空结果可为已完成但“无可识别文字”，不冒充配置失败。

云 ASR 完成之后才进入本机说话人观察和人物匹配。精转重跑创建新运行版本；旧任务与句段保留作纠错锚点、退出当前投影。人工逐句文字修正独立保存；来源与时间能唯一对应时迁移到新句段，否则进入待核对，不能由新结果静默覆盖。[`retranscribe_event_in`](../../crates/hear-core/src/events.rs)、[`reconcile_corrections`](../../crates/hear-core/src/knowledge.rs)。

## 7. 说话人、人物与可见文本

这里有三个不同的判断：

1. **云端分句/临时编号**：同一物理 ASR 请求里的 `speaker_label`。可能误拆或误并，不是跨事件身份。
2. **本机同场匿名合组**：使用当前事件/来源/版本中合格句段的本机声音向量，谨慎合并被云端拆开的标签；仍只是本场“说话人 1/2”。
3. **本机全局人物匹配**：与用户确认的 `Person + VoiceSample` 库比对，满足门槛才在该句显示人物名；人工确认可作用于单句或用户明确选择的本场组。

算法、样例、阈值、撤销与已知质量风险统一见[说话人和全局人物](speaker-identity.md)。当前 UI 用事件内首次出现顺序生成“说话人 1/2”；这个数字不能跨事件查同一个人。[`speakerLabels`](../../src/DetailDrawer.tsx)。

## 8. 时刻动作与截图

Mark、评论和主动截图是独立的 `moments`，按用户触发或选择的真实 `anchor_ms` 保存；事件边界改变只改变它们在详情中的显示归属，不改原始时刻。评论要求非空文字，Mark 不隐式截图。截图先记触发时刻，再截取当前操作屏幕；私有暂存 PNG 经过文件类型、大小、签名和落盘检查后发布，并单独记录实际捕获时刻。本机 Vision OCR 的文字及完成/失败状态写回截图对象供找回，失败不冒充“截图里没有文字”。[`moments.rs`](../../crates/hear-core/src/moments.rs)、[macOS 原生桥接](../../crates/hear-core/native/moments.m)。

## 9. 读取、清理与恢复算法

回放按真实时刻从有效封块切片，静音或真实缺口不制造伪音频；搜索读当前有效粗/精转、修订文字、笔记和截图 OCR 的派生索引，GUI 0.1 暂不显示搜索页，CLI 可搜索。导出按所选范围冻结对象身份、分页读取并在发布前复核清理状态，只有完整文件同步/发布后报告成功。[`playback.rs`](../../crates/hear-core/src/playback.rs)、[`knowledge.rs`](../../crates/hear-core/src/knowledge.rs)、[`archive.rs`](../../crates/hear-core/src/archive.rs)。

清理使用“范围 + 声音来源 + 模式”的预览及内容签名；执行时先校验预览仍代表当前数据，再提交 `clear_ranges` 墓碑，使查询、回放、云任务与导出立刻服从清理范围，随后重写共享封块并后台删除旧媒体。`raw_audio` 保留文字和既有声纹特征；`all_data` 还使关联内容与范围贡献的声纹样例失效。清理、供应商迟到结果和人工修订都通过当前版本/范围复核，避免被旧任务复活。[`cleanup.rs`](../../crates/hear-core/src/cleanup.rs)。

恢复不是重跑全天录音：核心启动时核对封块/暂存与数据库清单，恢复队列、声纹 `processing` 和未完成物理删除；无法恢复的采集尾部记录真实缺口。处理任务可重试，用户手动暂停意图保持。日志仅辅助定位，不参与事实重建。[`main.rs`](../../crates/hear-core/src/main.rs)。

## 10. 策略与验收分界

| 当前策略点 | 源码位置 | 性质与未关闭风险 |
| --- | --- | --- |
| 五秒物理封块、48 kHz | [`record.rs`](../../crates/hear-core/src/record.rs) | 存储/采集策略；需测不同设备回调和睡眠恢复。 |
| VAD 512 样本输入、约 20 秒主动 flush | [`hear-worker`](../../crates/hear-worker/src/main.rs) | 控制推理资源；需测长连续语音召回。 |
| `pending_vad_v2` 保留判断 | [`retention.rs`](../../crates/hear-core/src/retention.rs) | 工作树候选；安装版误删/误留未验收。 |
| 15 分钟候选静默切点、30 分钟计算窗 | [`events.rs`](../../crates/hear-core/src/events.rs) | 只切计算输入，不决定用户事件边界。 |
| 同源 ASR 最长约 15 分钟、Ogg/Opus、SSD 300 | [`events.rs`](../../crates/hear-core/src/events.rs)、[`cloud.rs`](../../crates/hear-core/src/cloud.rs) | 真实双人案例改善误拆；公开多人反例显示仍会误并。 |
| 本机匿名合组及全局人物门槛 | [`knowledge.rs`](../../crates/hear-core/src/knowledge.rs) | 小样本原型值，跨设备误认/漏认尚未冻结。 |

这些参数的改变须同时检查输入/模型版本、旧任务与修订重放、真实时间对齐、用户可见结果和代表性音频质量。单个案例的说话人组数吻合，不等于逐句分段准确，也不等于跨事件人物识别准确。
