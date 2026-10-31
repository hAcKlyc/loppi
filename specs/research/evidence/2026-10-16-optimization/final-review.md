# Final P0/P1 Review Contract

- Mode: repair；fresh reviewer只读，不修改、不派生reviewer；主Agent裁决修复。
- Requirement source: specs/prd/devplan_261016_reliability-performance.md；specs/research/2026-10-16-hear-reliability-performance-optimization.md全部F01–F14/P0/P1及验收。用户要求现有交互功能不变，录制稳定、内存可控、提速，测试验证。
- Exact baseline: `2c81ebd49601c85d89aa3529555aeddaa98d63f5`。Target production HEAD: `ba7e90e7bac25dc9cce649e612f327794561d105`，加当前最终集成修复（见下方 decisions）与文档/证据。原始四批不是独立产品发布，只是内部提交边界。
- In scope: 下方原始baseline到target全部文件；主要关注采集→journal→DB→VAD/粗转→当前投影→清理/恢复/资源治理的组合态，也核对完整需求。不能因为某问题属前批就跳过。
- Out of scope: 既有未跟踪design兔子素材、playground、两份原研究文件和旧reliability证据本身；它们作为需求/背景阅读但不更改。个人数据/core及系统偏好不可操作；所有写测试隔离HEAR_DATA_DIR。当前有旧SHA的2h隔离录制运行，不修改/重建其bundle；另一个最终修复签名包在target/release/bundle/macos/hear-verified.app。
- Required guarantees: 原页面/CLI/外部协议、来源/真实时间、模型/阈值、人工修订、暂停意图和云外发范围保持；core DB唯一业务事实，journal为其可恢复输入；清理墓碑/generation控制迟到发布，不能数据复活；有声无字、未知/失败保留；采集回调预分配且不等待业务DB/重型推理；来源故障域与停止截止有界；VAD/粗转按来源恢复且无未确认尾部遗漏；持久资源延期不耗尽失败重试；内存/缓存/队列有界，压力不静默丢音频；读取不占写锁、版本摘要兼容；连续播放/取消不退化；云真实付费请求身份与query-before-submit不变；观测正确归属且无敏感正文。
- Non-goals: 新UI/功能删减、替换模型或激进阈值、数据库服务化、取消fsync、自动删除历史人声、定时重启core掩盖泄漏；不保证OS DNS/Keychain/不可取消文件IO绝对deadline，软件预算不是OS任意瞬时内存硬上限；不将模拟/公开短样本当真实全天/低配/留出集证明。
- Owned defects: 全部F01–F14所要求的问题及本整份diff引入/显著放大的组合态问题；不把原始基线相邻且未放大的无关问题升级阻塞。
- Approved mechanisms: 版本化增量容量投影/访问索引/有界后台删除；预分配SPSC/独立来源进程/有界编码写入/可靠ingestion journal与回执；VAD-ASR角色拆分/persistent coarse_jobs/worker绝对deadline；原governor统一峰值准入、压力迟滞、延期、公平与空闲卸载；声纹worker复用/有界样例匹配；2连接短只读快照/data_version单页缓存/批量revision；单AudioContext双块预取；provider原队列的deadline/cooldown/公平；固定原子指标、replace-only诊断、限额资源采样与只读媒体审计。不得因批次共存引入双owner/临时兼容壳。
- Required lenses: fresh requirements/adversarial并行固定快照；主Agent裁决，只修本契约blocker并复验受影响行为；定向复核后fresh architecture审最终整个候选。之前批次reviews不能替代本轮。
- Code evidence: 191 workspace Rust tests (178core+9desktop+4protocol), cargo check workspace, TS/lint/build,21 frontend,3 sampler tests，CLI4组；batch-a/b/c/d-review.md均三视角PASS。B含DB锁11s、单源强杀、core强杀journal恢复；C含公开8组固定输入对比、连续语音/旧水位/双源恢复、声纹复用及30轮最大输入footprint；D含30/90/365天实际SQLite、真实WK、最终SHA999f41ae…的SCK/Tap双源12s。见对应JSON。模型实现未在D变化；最终组合态公开pipeline复验将更新独立证据，运行期间可读/tmp相应日志。
- GLOBAL ACCEPTANCE GAPS (mandatory, NOT waived): 最终版本2h/8h/多组24h/72h已知参考双路音频与资源趋势，最低macOS13/8GiB实机，真实标注语音留出集；真实设备/蓝牙/锁屏/睡眠/权限与慢磁盘/断电矩阵没有全部完成。当前M5/24GiB不能证明以上。Requirements应明确UNVERIFIED及最小实证要求；这些阻止status:implemented/发布，不允许用加代码或无关测试代替。当前2h旧SHA环境声观察不是已知参考或最终质量证据。
- Checks invalidation: DB/schema/recovery/cleanup改动重跑关联Rust/隔离故障与规模；采集/时序改动重跑签名native/媒体覆盖/长跑；模型/分段改动重跑固定输入与质量/资源；播放改动重跑调度取消测试及WK；governor生命周期改动对应状态/资源tests；文档/纯格式不使无关证据失效。

## In-scope files

- `Cargo.lock`
- `crates/loppi-cli/src/main.rs`
- `crates/loppi-protocol/src/lib.rs`
- `crates/loppi-core/Cargo.toml`
- `crates/loppi-core/build.rs`
- `crates/loppi-core/native/resources.m`
- `crates/loppi-core/native/system_audio.m`
- `crates/loppi-core/native/system_audio_tap.m`
- `crates/loppi-core/src/analysis.rs`
- `crates/loppi-core/src/archive.rs`
- `crates/loppi-core/src/capture.rs`
- `crates/loppi-core/src/cleanup.rs`
- `crates/loppi-core/src/cloud.rs`
- `crates/loppi-core/src/coarse.rs`
- `crates/loppi-core/src/events.rs`
- `crates/loppi-core/src/ingestion.rs`
- `crates/loppi-core/src/inventory.rs`
- `crates/loppi-core/src/knowledge.rs`
- `crates/loppi-core/src/main.rs`
- `crates/loppi-core/src/media.rs`
- `crates/loppi-core/src/microphone.rs`
- `crates/loppi-core/src/moments.rs`
- `crates/loppi-core/src/playback.rs`
- `crates/loppi-core/src/reads.rs`
- `crates/loppi-core/src/record.rs`
- `crates/loppi-core/src/recovery.rs`
- `crates/loppi-core/src/resources.rs`
- `crates/loppi-core/src/retention.rs`
- `crates/loppi-core/src/source_process.rs`
- `crates/loppi-core/src/storage.rs`
- `crates/loppi-core/src/system_audio.rs`
- `crates/loppi-worker/Cargo.toml`
- `crates/loppi-worker/src/main.rs`
- `scripts/audit_recording_coverage.py`
- `scripts/macos_rusage.c`
- `scripts/measure_macos.sh`
- `scripts/report_macos_resources.py`
- `scripts/sample_macos_resources.py`
- `scripts/verify_ingestion_quality.py`
- `scripts/test_playback.mjs`
- `scripts/test_resource_sampler.py`
- `scripts/test_native_resources.py`
- `specs/ARCHITECTURE.md`
- `specs/prd/devplan_261016_reliability-performance.md`
- `specs/research/evidence/2026-10-16-optimization/README.md`
- `specs/research/evidence/2026-10-16-optimization/batch-a-review.md`
- `specs/research/evidence/2026-10-16-optimization/batch-b-native.json`
- `specs/research/evidence/2026-10-16-optimization/batch-b-review.md`
- `specs/research/evidence/2026-10-16-optimization/batch-c-quality.json`
- `specs/research/evidence/2026-10-16-optimization/batch-c-review.md`
- `specs/research/evidence/2026-10-16-optimization/batch-d-fixture-coverage.json`
- `specs/research/evidence/2026-10-16-optimization/batch-d-gui.json`
- `specs/research/evidence/2026-10-16-optimization/batch-d-native.json`
- `specs/research/evidence/2026-10-16-optimization/batch-d-review.md`
- `specs/research/evidence/2026-10-16-optimization/batch-d-scale.json`
- `specs/research/evidence/2026-10-16-optimization/final-review.md`
- `specs/research/evidence/2026-10-16-optimization/final-integration.json`
- `specs/research/evidence/2026-10-16-optimization/preliminary-ambient.json`
- `specs/research/evidence/2026-10-16-optimization/final-history-1m.txt`
- `specs/research/2026-10-16-hear-optimization-delivery.md`
- `specs/research/evidence/2026-10-16-optimization/history-1m.txt`
- `specs/tech_docs/algorithms.md`
- `specs/tech_docs/speaker-identity.md`
- `src/App.tsx`
- `src/DetailDrawer.tsx`
- `src/playback.ts`

## Decisions and remaining acceptance

Phase 1 completed against ba7e90e: requirements found F10/P2; adversarial found F05/F07/P1. Main Agent accepted both. Overall status remains in-progress.

- R1/P2: external CLI raw-audio cleanup could leave the next prefetched 5-second chunk scheduled. Repair reuses committed ingestion cleanup generations, adds an additive read-only `playback_valid` request and optional generation in `playback_chunk`; no existing request/response fields or UI change. Player checks only while playing (one validation in flight; ~300ms cadence plus IPC), stops/releases buffered audio on matching committed cleanup; unrelated source/range and rollback do not revoke. Legacy chunks without generations use the prior one-chunk behavior. Protocol file `crates/loppi-protocol/src/lib.rs` is added to the repair scope. This is bounded change detection, not an impossible cross-process zero-latency revocation guarantee.
- A1/P1: closed/old session state could flush VAD while a durable contiguous journal tail remained unimported. Repair requires adjacent metadata, explicit same-session gap, generation-aware cleanup boundary, or fully drained journal; preserves per-session pending order. Source gap ownership is separate from lifecycle tail ownership and survives cleanup splitting. A short directory-inode file lock makes each bounded journal listing a publication prefix: reader never waits for writer, source writer waits at most2s off callback; lock never spans DB/model work in core. This also closes the left-prefix race from enumerating a directory while new entries appear. Legacy non-journal eligibility retains the existing rule.
- A1-related F05: stronger actual-model onset fixture (voiced onset at 4.8s) exposed premature retention of only the immediately prior block: A ended before detection, B had open speech without a completed region, A was deleted, later coarse input failed. Accepted as the same no-unconfirmed-tail-loss guarantee. Worker exposes open-utterance speech_pending; existing retention owner postpones negative deletion and resolves at most six contiguous predecessors when the region is published; replay includes pending onset. No new threshold/model/store. The fixture originally FAILED with both coarse jobs missing source; repaired version passes uninterrupted/closed delayed import/crash delayed import with exact text/time/retention equality.
- Repair verification status: 199 workspace tests (185 core + 9 desktop + 5 protocol), Rust fmt/check/build, TS/build/lint, 24 frontend tests, 3 sampler tests, CLI four groups passed. Public eight-input comparison retains exact segments/flags/text (new speech_pending field compared separately by retention tests); actual dual-source pipeline and continuous restart/legacy tests passed. Real WK external CLI cleanup stopped playback, preserving text/event boundary. Final signed SCK/Tap native and million-row claim-scale checks passed; final signed native writer lock 11s yields 2 durable pending blocks per source without drops; killing core while locked then reboot recovers all four pending blocks with identical frame counts. Original requirements R1 and adversarial A1/onset targeted rechecks PASS. Fresh architecture full candidate initially found F06; its final repaired verdict is recorded below. Detailed results go in final-integration.json.


## Focused follow-up verdicts

- Requirements: PASS，R1/F10 P2关闭。独立复跑8项播放器测试及额外慢校验/dispose场景；核对6个相关source/test哈希。确认WK旧局部构建适用于相同播放代码，不冒充最终二进制全验收。检查周期约300ms加IPC，没有零延迟或严格300ms上界。
- Adversarial: PASS，A1/F05/F07及同一保证下onset问题关闭。核对目录锁前缀、领取证明、gap归属拆分、最多6连续块保留与pending onset重放、实际模型夹具与source哈希；未自行重跑模型/native。
- 主Agent复核：以上局部代码阻塞关闭；全局mandatory证据仍UNVERIFIED，status仍in-progress，不可发布。

## Final architecture finding and adjudication

- Architecture full candidate: FAIL，仅一项代码阻塞F06/P1；其余owner/lifecycle/复用/复杂度PASS。主Agent对照持续观察的独立采样与诊断发现同一计量错误，reviewer独立3子进程复现。`proc_listchildpids` 返回PID个数，原代码又除sizeof(int)，3个孩子全漏计。[Apple原始实现](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c)确认API语义。
- 裁决：blocking defect，违反F06 core_and_direct_children统一准入/诊断；在原resources.m按count有界遍历，无新owner/状态。新增真实3驻留子进程回归先红后绿；原计量约9.2MB，对照应含孩子约131.1MB。
- 受影响验证：原生总量统计、governor与实际模型组合态、最终签名包总量/来源短链；刷新相关源码和二进制哈希。模型/阈值、SQL、GUI播放代码不变，原固定输入/大库/WK证据仍可定向沿用。旧环境声长跑在约60分钟主动结束，未完成2h，更不能作最终版本资源/24h证据。
- 文档义务：algorithms.md同步speech_pending、最多6连续前块、粗转任务先保护保留；不增加机制。最终定向architecture和adversarial复核待完成。

- F06修复复验：原生真实3驻留子进程回归先红后绿（新增脚本test_native_resources.py），修正后独立对照131,221,064bytes与适配器总量完全一致；199workspace tests/check、4项Python资源/采样tests通过。最终签名core SHA `c341c02d9853b35df566929959932104cd5cd58eeca32741a26c5600d629b756`，deep/strict通过；SCK/Tap双路12s再测零丢包，聚合计量/独立逐进程总量median比0.994/0.996（顺序采样、退出瞬态另报）。最终锁库11s/核心强杀4块恢复、实际双源335.52s、连续60s/旧升级、三种迟到入库起点夹具全部再通过。固定输入模型/查询/播放代码未改变，既有质量/规模/WK证据仍适用。定向复核待完成。

## Final verdict

- Final architecture **PASS**：完整原始基线到候选已审，唯一F06代码阻塞修复关闭；44个源码哈希及最终签名core/worker/desktop身份核对一致。reviewer独立真实3驻留子进程测试PASS；算法文档义务完成，owner/lifecycle/复用/复杂度无剩余阻塞。
- Adversarial F06 focused **PASS**：原生计数、64项界限、进入既有governor、真实测试及最终包证据闭环；其此前A1/onset PASS继续成立。requirements R1 PASS继续成立。
- 标准median按偶数样本两中间值平均，SCK0.994289、Tap0.995985；已修正文档为0.994/0.996，未改原始采样点。
- 本次代码修复与内部提交门禁通过。**整体发布验收UNVERIFIED**，mandatory最终已知参考长时、最低macOS13/8GiB、真实留出集和设备矩阵未完成；保持in-progress，不可发布。本轮review到此结束，不以额外假想风险开启新一轮。
