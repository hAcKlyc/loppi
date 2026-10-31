---
status: implemented
created: 2026-10-23
updated: 2026-10-23
---
# 晚间链路可靠性、事件完整性与处理状态

用户要求修复本次日志调查列出的所有问题，保持架构正确、简单可靠，最终提交 PR。交付包括事件漏覆盖、精转误切分、活动粒度、等待时间、结果状态、入库日志、电脑声音验证与历史失败恢复。调查基线为 0.1.4 / 0666e02；私有证据在 `/tmp/hear-audit-20261022`，正文、音频、数据库、凭据不得进入仓库。

## 执行台账

### 开发契约
- 一份计划、一个原子 PR；实现、自验证、三路审查、提交与 PR 全部完成。此次目标是可审查代码与有据的数据恢复，不替换用户安装包或合并 PR。
- 必赢场景：样本连续而毫秒存在 1 ms 抖动的静默正常连接；有意义的短句不会未经解释落在事件之间；持续多议题会议跨计算窗续接；新活动首次自动分析约 15 分钟内进入队列；空识别可与仍在处理区分并定位；入库锁竞争不再冒充 IO 故障；实际系统声探测有非零声音；32 个历史失败沿既有 Core 入口处理并逐条核实结果。
- 必须保证：原始样本、粗转、已提交 ASR 输入、远端 ID 不被重写；真实来源/session/缺口/清理边界不被容差抹掉；人工边界优先；旧计算窗口与已完成任务不因策略缩短自动重新付费；ASR 完成不承诺逐字正确；模型必须显式说明未归入活动的观察，结构校验不能代替活动语义评估。
- 不保证任意真实录音的模型划分总正确、不强制把时间相近的事件合并、不在睡眠时阻止系统休眠、不新增声音留存规则；自动修复不回溯改写用户事件。模型质量用合成真实调用和私有离线结构回放验证，不能将用户录音正文发送到新服务。
- 状态来源：Core 的 audio_blocks/session/sample_start 与 media_gaps 决定连续性；candidate_windows/cloud_tasks/event_candidate_links 决定分析版本和续接；asr_inputs 与 event_asr_refs 为物理请求和当前归属；UI 只显示 Core 派生结果。历史恢复仅通过已安装 Core 的 cloud retry，不直接写活跃 SQLite。
- 复用：asr::confirmed_silence/plan、build_candidates_with_cuts/candidate_cut_positions、llm_input/publish_fit_with_input、现有有界 LLM 重试、event_state/transcript_tasks、ingestion::pending、capture_supervisor/capture_retry_delay、system_audio::probe_child、retry_task_in/replan_failed。
- 允许的接口变化：LLM 单次输入/输出加入观察的明确未整理归属；TimelineItem 可加向后兼容的可选派生统计/空结果范围，以便解释与回听。没有新持久状态、owner、队列、后台重试器或独立云恢复通道。
- 架构依据：specs/ARCHITECTURE.md、specs/tech_docs/algorithms.md；本仓库无 AGENTS.md/CLAUDE.md。唯一写入 owner、真实时间轴、清理墓碑和已提交输入不可变。

### 方案
1. 静默拼接在同来源、同 session 且样本连续时接纳毫秒量化误差，保留一分钟上限和真实 gap/clear 拦截；以生产 planner 在冻结库上离线复算验证。
2. LLM 输入保留可定位的完整观察，输出事件并显式列出未整理观察。Core 原子校验全部观察覆盖、来源/范围/重叠，遗漏走原有至多三次格式修复路径；不另建二次模型、语义阈值或自动合并器。补多议题、未结束措辞、短问题、真正活动切换等真实模型合成验收。
3. 新计算窗最长 15 分钟，安静等待 5 分钟；已有持久窗口保持原分组，不因新策略拆旧结果。事件稳定观察维持 15 分钟，前序上下文和 ASR 复用不变。
4. 精转任务完成但空结果时显示完成及空段数量，详情可定位回听；跨人工边界的文字单独说明，不能误称空结果。统计派生自当前有效引用，不做第二套状态。
5. 入库枚举区分锁繁忙与真正 IO 失败；繁忙留待原轮询继续，不消耗失败、不刷 warn；真错误保留有限的脱敏原因码。
6. 系统声用安装版与必要的隔离签名候选执行有声探测，核实 SCK/Tap 当前路径。仅修有证据的生命周期/恢复根因，不因为夜间数字静音推断权限损坏；核查既有 2–30 秒退避与正常唤醒恢复；若无新根因证据，不改原生生命周期。
7. 先冻结恢复前状态，再对 32 个历史失败走现有显式重试；可用原结果直接发布，不可用或旧无效结果由已实现的当前输入规划重识别。逐项记录完成、替代关系、剩余错误与实际提交次数，不把重试入队当恢复成功。

### 验收与结束条件
- mandatory：有针对性的 Rust/前端回归，workspace check/test、前端 typecheck/lint/build、隔离 Core/CLI 烟测；冻结库 ASR 规划与结果统计回放；真实模型合成活动评测；真实系统声有声探测；历史失败最终状态清点。
- 所有原始调查与恢复材料留本机；仓库只放合成测试和不含正文/身份的数量证据。用户授权的修复与 PR 包含必要的合成模型调用和既有历史恢复请求。
- 修改原生采集启动/监督则必须补相应真实运行证据；未改原生采集路径时不能把已有探测声称为新路径验收。长时多设备稳定性为 advisory，不用单次探测作全天保证。
- required review：requirements/adversarial 新上下文并行只读；裁决修复后 architecture 新上下文最终审查。全通过才 commit；按影响重验。
- 结束：全部范围、实际证据、三路审查通过，提交 conventional subject/body、push 并建立 PR。发布安装、公证、合并不在此次 PR 交付内。

### 行动清单（单批次）
- [x] 静默连续性与规划回归/冻结复算。
- [x] 事件完整覆盖、活动粒度与短窗口兼容，真实模型验证。
- [x] 处理状态与空结果定位，前端验证。
- [x] 入库锁竞争修复、采集恢复核查与系统声实测（没有改动原生采集）。
- [x] 历史失败恢复与逐条核实。
- [x] 架构/算法文档与脱敏结果，同步验证。
- [x] 三路审查、修复、提交、PR。

### Review 基线
- Base: 0666e0262ce45ae051bd08c4d7fdf2beb26e3c32。
- Branch: fix/evening-pipeline-reliability。
- 范围：Core 事件/ASR/入库/采集监督、可选协议派生字段、前端状态与详情、相关测试/文档；不包含任何私有快照、正文或安装包。

### 待用户决策
无；用户已授权合理优化方案及全部修复。本计划内的局部实现取舍由开发核实完成。

### 进展
- 2026-10-23：建立计划，确认运行版与基线一致；复用本机编译和 npm 缓存，独立分支开发。开始前工作树干净。

- 自验证：`python3 scripts/verify.py --suite all` 全通过（workspace check/test、前端 typecheck/lint/build、资产/Python/native policy、二进制身份、隔离 Core 与诊断烟测）。最后的回听按钮样式/秒级范围调整另跑前端静态、相关测试与 build；浏览器 `scripts/test_transcription_progress.html` PASS，按用户点击定位正确来源。
- 私有冻结回放：19:00–24:00 的 14 个事件，生产 planner 重算由 53 请求变 20 请求，编码 7,300,079 ms（旧 6,775,060 ms）。增加真实静默，因此不承诺费用下降。四个已经完成的晚间候选哈希/状态保留；另有旧半小时窗口合成兼容测试。全历史强制重扫会遇到既有的迟到证据变化，兼容验收限定相同已稳定输入，不宣称历史永不修订。
- 真实 LLM：deepseek-flash，11 个合成场景、67 个计算窗口，最终一轮全通过；包含两小时多议题、短休息、局部收尾措辞、家庭闲聊、散会后同项目电话、连续两场会议、小时边界换活动、独立并行两路、共享线上会、零散短句。首轮发现孤立收尾窗被模型留作未整理，补强结合前序活动保留收尾语后重验全部。最终调用用量 187,539 tokens；合成验证不能保证任意真实录音的语义判断。
- 系统声：运行中的安装版 0.1.4 执行 6 秒 system probe，同时播放合成 440Hz 测试音，48kHz/mono，229,440 frames，peak 0.1831，dropped_packets 0。与日志中的睡眠/唤醒及既有有界退避对应，未确认新的清醒采集故障；没有为数字静音添加新的电源观察器或修改采集生命周期。睡眠本身仍允许系统正常暂停采集，长时多设备质量属 advisory。
- 历史恢复最终核实：原 32 个失败全部经安装版 Core 的显式 retry；18 个保留原 remote ID 和提交次数，取回原结果完成；14 个旧坏结果被替代，共 56 个新物理请求，55 完成、1 失败。原 32 个 remote ID 全保留、旧任务新增提交次数为 0；新增精转句段 1,081。剩余失败只对应原来 1 个任务范围中的五秒片段：供应商返回 54 句，其中 12 句 start=end=5000ms。保留 `provider_invalid_utterance` 可见，不删除坏句后冒充成功、不无界付费重试。恢复范围共 31/32 完成，1 个外部坏结果已确认终态；这不是本 PR 尚未实现的本机代码修复。
- 私有证据：`/tmp/hear-audit-20261022/{verification.json,planning-replay.log,llm-eval.log,recovery-final.json,system-probe.json,transcription-ui.png}`；数据库、凭据、真实正文均未纳入 Git。

### 本批 Review Contract
- Mode: repair；Base: 0666e0262ce45ae051bd08c4d7fdf2beb26e3c32；目标为当前分支完整工作树 diff（含本 devplan、新浏览器夹具），所有改动均属于本批。
- 范围：`crates/loppi-core/src/{events.rs,events/activity_tests.rs,events/asr.rs,events/asr/tests.rs,ingestion.rs,main.rs,diagnostic_tests.rs}`、`crates/loppi-protocol/src/lib.rs`、`src/{api.ts,TimelineView.tsx,DetailDrawer.tsx,App.css,cloud-task-status.ts}`、`scripts/test_{cloud_task_status.mjs,timeline_view.mjs,transcription_progress.html}`、本文及架构/算法文档。
- 保证/非目标/新增机制以开发契约为准。三路均 required。只让本次引入、显著放大或契约明确负责的问题阻塞；不得将模型任意语义正确、供应商必然返回有效文字、睡眠不中断采集升级为保证。
- 没有新的持久状态、写入 owner、队列、重试器。新增请求内观察覆盖协议和可选派生结果统计是契约已允许的接口；兼容已发布旧响应与持久候选。
- 外部资料、个人数据、安装目录、共享缓存 symlink 不在 PR 范围。无其他用户/会话代码改动。
- 影响验证：planner/候选/LLM 改动重跑相应 Rust 与冻结或真实模型验收；协议/UI 改动重跑前端静态/相关测试/构建、Core 协议测试及浏览器；入库改动重跑锁竞争与 Core 烟测。只改报告不要求重跑未受影响事实。

### Phase 1 裁决与修复
- Requirements R1：全部空结果 `no_transcript` 未显示完成与数量，属于契约内阻塞。已统一使用派生 empty 统计；旧响应也显示“精转完成 · 无可识别文字”。新增 no_transcript/legacy 回归、all-empty 浏览器场景，前端静态/相关测试/build 与浏览器 PASS。
- Adversarial A1：两路观察让相邻计算窗相交时，旧上下文与续接条件排除了前序，提案冲突被跳过却标为完成，属于短窗口显著放大的契约内 P1。已在现有 fit_context 允许起点更早的交叠前序，排除当前 linked 重复上下文；续接结束取 max，不能缩短已有活动。发布冲突现在区分人工优先与自动事件冲突：后者走现有边界错误/三次有界重试，不冒充完成。没有新增持久状态或重试器。
- 修复回归覆盖两窗相交 5 秒、遗漏续接 ID 的原子拒绝、合法续接保留同一 ID、前序已经延长时不缩短，以及原有人为锁定/独立双来源；events 103 项通过。冻结生产 planner 再验仍 53→20、四个既有完成窗口不变。真实模型新增相交窗口场景并重验全部，结果在最终审查记录补齐。

### 最终候选证据与审查状态
- Phase 1：requirements R1、adversarial A1 均按契约内阻塞修复，原 reviewer 定向复核均 PASS；无剩余阻塞、无必需证据缺口。没有新增范围或机制。
- 最终真实模型：12 场景 / 69 计算窗口均 PASS，196,899 tokens；包括新增交叠窗口及全部既有活动粒度场景。前一轮 11/67 为修复前阶段证据，以本轮为最终结论。
- 最终 `verification-final.json`：14 组检查全通过；Rust 315 Core + 11 desktop + 9 protocol 测试，前端 52、资产 3、Python 6、native policy、构建和隔离烟测均通过。11 个 opt-in Rust 测试按默认门禁跳过，其中本需求的冻结和真实模型测试另行实际通过。
- 浏览器：部分空结果与 `?all-empty` 均 PASS，秒级时间与声音来源明确；原详情定位/后页加载/暂停打开/来源筛选回归也 PASS。系统声、历史恢复不受审查修复影响，原证据仍有效。
- 下一步仅最终 architecture 审查、commit/push/PR。发布安装与合并仍不在此次交付。

- 最终 architecture：PASS。确认 Core 唯一写入、当前引用投影、输入不可变及人工边界优先；请求内覆盖契约、可选派生统计、纯校验辅助函数均为已授权的最小变化。三路审查全部完成且无剩余阻塞，进入 Git 提交与 PR。

### 交付
- 实现提交：`c8bc659`（fix: preserve event coverage and continuous transcription inputs）。
- PR：[hAcKlyc/hear#14](https://github.com/hAcKlyc/hear/pull/14)，目标 main，分支 fix/evening-pipeline-reliability；未合并、未替换本机安装版。
- 整体门禁：全部范围按契约核实，三路 PASS，最终组合态静态/测试/构建/烟测通过，必需真实模型、冻结库、系统声和浏览器证据已闭环；没有待用户决策或待完成的本机代码项。
- 限制保留：1 个五秒历史片段的供应商无效输出仍显式失败；不自动重复付费，不伪造时间或丢弃坏句后报成功。任意真实录音的活动语义和长期多设备稳定性仍不由有限样本保证。
