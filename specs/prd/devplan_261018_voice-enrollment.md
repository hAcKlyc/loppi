---
status: implemented
updated: 2026-10-18
---
# 优质声纹补充与短句组合

## 目标与开发约定
用户新建或选择已有的人物后，从人工确认的当前范围自动补充声音样例。逐句显示与云端 ASR 不变；全程本地。自动识别结果不能反向训练人物库。
- 同一已确认人物、有效本场分组、音源、ASR target/version 的相邻短句可组合；不跨其他人插话、重叠、长停顿，不填入间隔音频。
- 采用约 6 秒目标、3 秒有效声音下限、12 秒组合上限、1.5 秒最大间隔、最多 12 个片段。仅本句不扩大人工授权范围。
- PCM 电平/削波/有效声音占比筛选；优先更充分的有效声音。近似重复仅保留更优者；每人物每场每音源每模型最多 3 个、人物每模型最多 24 个有效样例。
- 增量入库检查同人物已有声音的一致性及其他人物冲突，拒绝不确定候选而保留人工姓名归属。阈值是保守工程策略，不保证真实识别准确率，无新增云调用。
- Core knowledge 为持久 owner；沿用 voice_loop、embed worker、资源门控和媒体屏障。新增 Core 内部组合候选及样例证据/评分表，记录所有来源句段及其人工绑定 ID；不新增进程/外部通信/独立服务。
- 入库前重新验证证据；撤销/改名归属/删人物/清理/重转使相关待办失效。组合中任一句撤销会撤销整份样本。原音清理仍保留已提取有效样例。
- 历史有效样例按新数量/去重策略规范化，不凭缺失音质评分宣称已通过新 PCM 门槛；历史无人工来源样例不保留为有效样例。
- 不承诺消除人工错误标注或给出校准置信度；无模型训练/模型更换，无全库历史音频重新提取。

## 验收与结束条件
确定性测试覆盖短句组合、来源/人物/版本/间隔/插话隔离、音质门槛、去重与两级限额、现有人物补充、自动身份不入库、双人物冲突拒绝、迟到结果、撤销/清理/重启。受影响 Rust 测试、clippy、前端静态检查，隔离真实本地模型组合 smoke，签名桌面构建；三视角审查通过后提交。
真实录音跨设备识别准确率为 advisory，需未来留出集评测；不使用用户真实声音/身份做测试修改。

## 执行台账
- Batch base: 419b0f223d8267af1203fbf3c2682747e385a606
- 当前批次：唯一原子交付
- [x] 持久取样与证据链、音质/去重/限额策略
- [x] 组合 PCM 与原 voice_loop 集成
- [x] 撤销/清理/迁移及回归
- [x] 文档、验证、三视角审查、签名构建与提交
- 待用户决策：无

### 实现进展
- Core 内新增 enrollment 模块，复用 voice_loop/worker/voice_outbound；人工绑定证据链与删除触发器已实现。
- 单句与组合共用筛选入库；短句最短 400 ms，组合候选每次每场每路最多 12 份，active 库 3/24 上限。
- 已通过前端 typecheck/lint；新增确定性测试包括实际 Opus 解码；合成语音 + 已安装 CampPlus 本地 smoke 已通过（7120 ms 有效声音），无用户音频/姓名测试写入。
- 初轮测试暴露同批组合候选取出顺序不确定，已按同批有效片段时长优先稳定排序；完整回归执行中。

### Review Contract
- Mode: repair；baseline: 419b0f223d8267af1203fbf3c2682747e385a606。
- Requirement source: 本 devplan 和用户明确的“短句组合取样、已有身份补样本，筛选、去重、限额，避免自动识别污染”请求。
- In-scope: 本次 crates/loppi-core/src/knowledge.rs、knowledge/enrollment.rs、knowledge/enrollment/tests.rs、events.rs、main.rs、cleanup.rs；src/DetailDrawer.tsx（异步筛选提示）；specs/ARCHITECTURE.md、specs/tech_docs/speaker-identity.md。其余已有未跟踪文件不在范围。
- Guarantees: 人工确认来源；同组/target/version/音源组合，排除间隔音频及插话；新 PCM 质量门槛；重复择优及 active 3/24 上限；同人一致性/他人冲突拒绝；迟到发布复验，纠正任一成员/删人物/重转/清理撤销；沿用 Core owner 和既有资源门控；保留逐句显示及已确认姓名。
- Non-goals: 不保证真实准确率/消除首次人工误标，不训练/更换模型，不新增云能力，不对历史全库音频重新提取；历史样本只做证据/去重/限额迁移，不假称已通过新的 PCM 门槛。无新 UI 或新协议。
- In-scope defects: 本改动引入/放大，或上面 guarantees 明确要求的故障。不得把未修改的旧 ASR/归属/UI 问题纳入必做。
- Allowed architecture: Core knowledge 内部 voice_enrollments/parts 持久组合候选、voice_sample_parts 人工证据及 quality 元数据表；沿用已存在 voice_loop、worker、generation、迁移和 voice_outbound，无新 owner/服务/外部通信。
- Required lenses: requirements、adversarial，然后 final architecture。审查只读，不编辑文件，不额外派发。

### 交叉审查裁决与修复
- Requirements P2：无标签极短句的 NULL group 被当 String 读取，导致人工绑定回滚。确认是本次引入且违反保留姓名承诺。先新增回归复现（/tmp/hear-enrollment-null-regression.log），统一 nullable current_group，无法确定组时不组合，单句姓名保留。
- Requirements evidence gap：补充有效组、target、historical version 的显式边界测试，均通过。
- Adversarial P2：取样过程中清理外部支持声音导致组拆分，旧组合仍可发布。确认违反同组与发布复验；真实声学合组+cleanup回归先复现（/tmp/hear-enrollment-group-regression.log），发布与领取复核当前组成员同组，不固定旧派生组 ID。
- 修复验证：40 项 knowledge/相关测试通过（1 个显式模型 smoke 忽略，之前已单独通过）。首次签名构建通过，修复后的最终构建待完成。
- 两个新的 clippy chunks_exact 建议已机械修正；不处理已有无关警告。

### 架构审查修复
- Architecture P2 确认：新入库策略在写事务里逐份扫描全库，违反既有分批读取/锁外计算边界。修复复用 VoiceMatcher 的排名与缓存；候选表既支持组合取音，也支持已提取单句/组合向量后台筛选。无新增 owner/协议；已有 generation 在发布时验证，失效则重取快照。
- 名字确认先完成，声纹通过后台筛选才入库；显式追加按钮反馈改为“已提交声纹筛选”，不提前宣称保存成功。原音清理只取消尚需音频的候选，已提取向量可继续。
- 新回归：全库比对每页至多256条、计算期间 Store 可获得；库变更拒绝旧判定；已提取候选经数据库重开及原音清理后仍可入库。Phase1 reviewer 将复核本次生命周期调整。
- 异步修复后最终验证：Core 228 项、protocol 7 项通过；2 项显式测试默认忽略，其中真实本地 CampPlus 合成语音 smoke 单独通过（7120 ms 有效声音）。typecheck/lint 通过，clippy 通过且无本次新增警告；git diff --check 通过。
- Requirements / adversarial 聚焦复核均 PASS；final architecture 确认锁内全库比对 P2 已消除，PASS。源码冻结，最终签名桌面构建与运行升级待完成。
- 最终签名构建通过（/tmp/hear-enrollment-final-build.log），codesign deep/strict 校验通过。本机已启动新 Core（PID 91837）及 GUI（PID 92195）；CLI 与主窗口均确认麦克风/电脑声音正在记录，恢复升级前两路状态。未对真实人物或录音做测试性修改。
- mandatory 验证全部完成；advisory 仍为真实多人/跨设备留出集准确率评测，不宣称工程阈值已校准。
- 交付提交：bf8e40d（dev），仅本任务9个源文件/技术文档；未提交 devplan、playground、research 及其他既有未跟踪内容。最终 HEAD 与通过测试、审查、构建的候选源码一致；单批次无需重复全套审查。无 push/merge/tag。
