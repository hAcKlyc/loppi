---
status: in-progress
updated: 2026-10-22
---
# 精转文字按请求时间轴发布，纠正暂停缺口

用户已确认保留完整文字，接受偶发长时间跨度；进一步明确优先保证数据可用、移除过度防御。本次删除文字发布的原声占比、静默长度、首尾容差及最小原声重叠过滤，不用本机 VAD 重判云端文字。只保留当前任务/清理归属、可还原的输入时间轴、正时长和请求范围校验。声纹与回放仍独立读取真实原声。

## 执行台账

### 开发契约
- 必赢场景：10/21 的 18 个云端成功结果离线重放全部发布、779 句保存且幂等；录音尾块入库后暂停缺口不再覆盖已经确认的原声。
- 原子范围：ASR 发布过滤删除、相应回归；意图缺口关联既有 session、入库校正、旧缺口有据修复；技术文档、研究结论、验证与三路审查、提交。
- 必须保证：完整文字和供应商时间；不因当前媒体留存或纯补入静默拒绝文字；真实请求越界和旧压缩输入映射歧义仍拒绝；清理不复活、事务原子和幂等不变。意图缺口仅由其来源/session 的确认封块缩短；回调/失败缺口不变；迟到、关闭、清理分片和升级同样生效。
- 不做：更改一分钟输入策略、词时间猜测、声纹阈值、UI 状态标签、自动重转/新付费请求、安装/重启客户端、活跃库恢复。历史结果恢复另需使用已修复版本，原 remote ID 不变；普通重试的远端缺失分支可能重新提交，不能宣称其为严格 query-only。
- 本次负责：研究报告定位的错误文字过滤与暂停意图/封块收尾不一致；既有无关采集稳定性问题不扩大。
- 状态来源：asr_inputs 为已提交输入事实，cloud_tasks 为发布归属，clear_ranges 为墓碑；audio_blocks/session.last_confirmed_ms 为采集确认事实；media_gaps 的 source_session_id 复用现有归属字段，Core ingestion 在入库事务中缩短本 session 的意图缺口。
- 复用抽象：publish_asr、asr::map、task_current、open_intent_gap、ingestion::commit、source_session_id、storage::migrate、cleanup 的 gap 分片。
- 新增机制：无新 owner/store/协议/队列/重试。一次旧持久数据迁移复用 core_migrations，处理真实已安装版本的数据兼容。
- 不变量：Core 唯一业务状态 owner；输入/remote ID 不变；保留真实时间；清理 tombstone 不复活。仓库没有 CLAUDE.md/AGENTS.md，架构依据 specs/ARCHITECTURE.md 与算法文档。

### 验收与结束条件
- mandatory（实现）：合成回归覆盖长静默/纯静默/媒体清理后文字、旧输入映射、越界回滚、重复发布；四类意图缺口/来源session隔离、最终块前后顺序、迟到关闭尾块、gap分片、升级幂等；冻结真实库和原响应重放。全仓 verify all、三路 review 和 scoped commit。
- mandatory（安装发布）：本轮不安装、不重启，保留后续发布门槛：签名安装版实际暂停/恢复与定时停止，核对最终封块/gap、文字和声纹边界；完成前台账保持 in-progress，不宣称安装修复。
- advisory：词时间精度、更多天的识别质量/资源观察。
- 改动影响：Rust 生产代码变更重验 core check/test/build 与隔离烟测；前端未改但最终默认 verify 全门禁；后续仅文档不重复构建。审查修复按影响重验。
- 结束：当前代码范围验证与审查通过后提交，报告安装验收边界，不因额外假设继续添加防御。

### 行动清单
- [x] 移除文字启发式过滤与旧假设测试，保留位置/生命周期约束。
- [x] 同 owner 修复意图缺口、限定历史修复及回归。
- [x] 原响应离线回放、验证与文档同步（默认 all 入口的既有手动脚本发现问题及独立补验见下）。
- [x] cross-review（requirements/adversarial 并行，最终 architecture）、修复、提交。
- [x] 记录发布验收边界及交付状态。

### 当前批次 Review 基线
- 单批次，base: 83bc811。
- 预计范围：crates/loppi-core/src/{events.rs,events/asr/tests.rs,main.rs,ingestion.rs,diagnostic_tests.rs}；specs/tech_docs/{algorithms.md,diagnostics-and-verification.md}；本计划与本次研究/脱敏证据。
- 排除：主工作树其他未跟踪材料、安装包与活跃数据；历史实验补丁仅证据，不作为最终生产代码。

### 待用户决策
无。2026-10-22 已接受完整文字/偶发长跨度，并要求进一步移除原声重叠等启发式防御。

### 进展
- 2026-10-22：从既有私有实验迁入独立 fix/asr-publication-input-clock worktree，沿用冻结证据与编译缓存；最终契约去掉实验版“纯补入静默也拒绝”的过滤。

- 2026-10-22：整组文字过滤删除，纯补入静默也保留；94 项事件定向回归和 19 项真实 journal/入库回归通过，准备全仓验证与三路审查。

### Review Contract
- Mode: repair；base 83bc811；target `<USER_HOME>/Documents/project/hear-asr-publication` 当前工作树完整差异。
- Requirement source: 本计划完整契约，用户最新“优先保证数据可用，拒绝过度防御”。不再要求纯补入静默文字拒绝。
- In-scope: `crates/loppi-core/src/diagnostic_tests.rs`, `crates/loppi-core/src/events.rs`, `crates/loppi-core/src/events/asr/tests.rs`, `crates/loppi-core/src/ingestion.rs`, `crates/loppi-core/src/main.rs`, `specs/tech_docs/algorithms.md`, `specs/tech_docs/diagnostics-and-verification.md`, 本计划, `specs/research/2026-10-22-asr-publication-failures.md`, `specs/research/evidence/2026-10-22-asr-publication/summary.json`。
- Out-of-scope: 安装版、活跃 DB、主工作树调查实验补丁、其余代码与私有音频/供应商正文。禁止 reviewer 修改文件或派生子 agent。
- Guarantees / non-goals / responsible defects: 严格采用上方开发契约，不提高保证等级。特别是没有自动恢复历史云任务、没有新付费调用承诺；安装真机验收为后续发布门槛，代码状态保持 in-progress。
- Allowed mechanisms: 无新 owner/store/协议/queue/retry；复用 `source_session_id` 与 `core_migrations` 做真实旧数据的限定校正。删除过滤，不增加文字防御。
- Required lenses: requirements, adversarial, final architecture；Phase 1 两路审只读同一候选，修复后最终架构全量差异。
- Evidence: events 94 定向通过、ingestion 19 定向通过、18 个原结果离线恢复 779 句且幂等、10 事件不再 asr_failed、22:31 原暂停事件 transcribed；全仓 verify 在 `/tmp/hear-asr-publication-verify.log` 运行中，报告将写 `/tmp/hear-asr-publication-verification.json`。验收证据按代码影响重验；文档更新不失效构建。

### 自验证结果与审查裁决
- 最终原响应重放：18/18 发布、779 句保存，重复发布 0 新增；10 事件退出 asr_failed，22:31 为 transcribed；旧暂停 gap 与确认原声不重叠。
- 默认 `verify all` 已执行：typecheck、lint、workspace cargo check/test、49 个前端测试、3 个兔子资源测试通过。Rust core 304 通过/9 opt-in 跳过，protocol 11、worker 9 通过。
- 默认入口未整体通过：既有 `scripts/test_model_preparation.py` 在 unittest discovery 导入时直接执行，把 argv 的 discover 当模型目录。该文件与 base 的 SHA256 相同；本次不改无关模型脚本，不宣称 all 绿灯。
- 按原入口顺序补验其余项目：6 个普通 Python 测试（正确设置 scripts 模块路径）通过；native entry policy 编译/实跑通过；`verify --suite smoke` 全部通过，含前端构建、workspace 二进制构建、四组件身份、隔离 Core/CLI 与只读诊断烟测。需要真实模型夹具的手动脚本未执行；它不是本次变更的验收证据。
- Phase 1 requirements：产品契约 PASS、无代码阻塞；唯一证据项是同步以上剩余检查。该证据已补齐，原 reviewer 定向复核 PASS。
- Phase 1 adversarial：PASS，无可达契约违反；没有追加文字过滤或架构机制。
- 裁决：默认测试发现问题为 baseline/out-of-scope，单独记录，不驱动额外代码；其余默认检查均已独立完成。安装版暂停/恢复、定时停止仍未做，不以离线结果冒充；状态保持 in-progress。

### 用户补充的文档范围（2026-10-22）
- 用户明确要求所有相关技术/架构文档同步。生产代码与 Phase 1 审查候选一致，仅扩展文档范围：`README.md`、`specs/ARCHITECTURE.md`、`specs/tech_docs/{README.md,speaker-identity.md,cloud_usage.md,local-processing-verification.md}`、`specs/prd/{prd_0.1.md,tech_rfc_0.1.md,devplan_261007_asr-timing-fix.md,devplan_261021_pipeline-readiness.md,devplan_261021_one-minute-silence.md}`。原范围算法/诊断/研究与本计划同样更新。
- 同步现行一分钟输入规则、文字按输入发布、声纹与回放独立、意图缺口归属/迁移、历史查询与潜在重传边界；旧开发记录添加后续变更说明，不改写当时验证结论。技术索引日期/协议号同步到当前 20。
- 文档检查：无现行“3 秒拆上传”、纯静默文字拒绝或实际媒体覆盖发布准入的残留；旧错误码标为历史。只更新文档不失效已通过构建/测试；交最终架构审查一并核对。

### 最终审查与原子门禁
- requirements 定向复核 PASS，剩余检查与证据同步项关闭；adversarial PASS；最终 architecture（含用户要求扩展的全部文档）PASS。无代码阻塞、无新增产品决策、无额外文字防御。
- 架构审查两项旧文案一致性已更正：历史 audio_coverage 错误码、最终仓库无实验补丁；私有正文表述明确为“不进入仓库”，回放仍在临时 DB 保存文字。
- 生产代码自 Phase 1 后未改变；后续仅文档/证据更新，不失效已通过测试与构建。diff/格式检查通过，30 个新增本地文档链接已核对。
- 对整份计划重读：当前代码范围、文档、离线回放与三路审查均完成；未引入临时 flag、兼容壳或新队列。旧持久数据迁移有真实已安装版 owner；普通云重试不承诺 query-only，未操作活跃任务。
- 状态保持 in-progress：签名安装版的真实暂停/恢复与定时停止尚未执行，本轮不替换安装版、不写活跃库。发布前须确认最终封块后的 gap 起点、精转文字保留及声纹/回放独立可用性；不能把本轮代码门禁当作已发布修复。

### 提交与交付备忘
- 实现提交：`b12197daeb4f37e0bcdce55e78ceb08cdf5d413b`（`fix: publish ASR text against its submitted input clock`）。单批次，无代码修复遗留，三路 PASS。
- 提交后最终静态门禁：workspace cargo check、cargo fmt --check、TypeScript typecheck 与 lint 均通过；后续仅本台账/脱敏摘要收尾，不改变已验证代码。
- 文档与实现随 `fix/asr-publication-input-clock` 分支交付；独立 worktree 完成审查后，将该分支切回用户原工作区，便于后续构建。原工作区早期未跟踪调查材料先保留私有备份，避免覆盖；已安装 App、活跃 DB 与录制意图不动。
- 真机验收（mandatory，尚未执行）：① 签名修复版实际暂停后核对最终封块终点与 gap 起点，恢复后缺口范围有效；② 定时停止走同样边界，保留完整尾句；③ 长静默文字保留、声纹/回放按真实原声独立可用；④ 历史精转任务未自动重送，任何恢复先核对原 ID 与实际查询/提交行为。状态保持 in-progress，不能宣称已安装发布。
