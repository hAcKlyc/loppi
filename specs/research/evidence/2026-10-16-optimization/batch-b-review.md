# Batch B Review Contract

- Mode: repair。主 Agent 修改；reviewer 只读、不得派生 reviewer。
- Requirement source: `specs/prd/devplan_261016_reliability-performance.md`；已接受研究 `specs/research/2026-10-16-hear-reliability-performance-optimization.md` 的 F01/F03/F04/F09、采集可靠日志/范围屏障与有界队列部分。P0/P1 是整体原子交付，本批为内部稳定提交点，不是可发布版本。
- Exact baseline: `d9fbd5712de1cf94666cd9d68c54f65c0dc1aa44`，target = 本批当前工作树。
- In scope: Cargo.lock；crates/hear-core/Cargo.toml；native/system_audio.m、system_audio_tap.m；src/capture.rs、cleanup.rs、events.rs（仅测试 fixture 显式列名）、main.rs、media.rs、microphone.rs、record.rs、system_audio.rs；新增 src/ingestion.rs、inventory.rs、source_process.rs；specs/ARCHITECTURE.md；上述 devplan；本目录 batch-b-review.md、batch-b-native.json。
- Out of scope: 已提交 Batch A；其它未跟踪的兔子素材、playground、旧 research 报告/旧 evidence；worker/前端等未改代码，除非本批引入/显著放大缺陷。
- Must guarantee: 现有功能/交互/来源和真实时间语义不变；单核心拥有业务事实；有声无字/未知/失败保留；用户暂停不被覆盖；不扩大云外发；迟到媒体与恢复不能复活被清理范围；已确认媒体耐久与幂等；有界内存/队列；来源原生异常与停止不能结束或卡住另一来源/核心；文件 IO 不在正常采集消费线程或主库锁中执行。
- Non goals: 新 UI/模型/阈值、外部 DB 服务、删除历史人声、取消 fsync、定时重启 core。不能宣称 24h/72h、低版本/8GiB 或真实标注质量验收已通过；这些是最终组合态 mandatory 门槛，仍在台账。
- Owned defects: 本批引入/显著放大的状态、并发、时序、清理、恢复、资源生命周期问题；上述 F 项本批覆盖。
- Approved architecture mechanisms: 预分配 SPSC 回收缓冲；有界归一化/封块；同签名可执行文件的受监督来源 helper；core 管理的 prepared/ready ingestion journal 与 DB receipt/session generation；既有 cleanup_gate + 精确来源/范围/提交时刻 tombstone；分批后台媒体对账。业务 DB 仍为唯一权威，不新增可编辑事实库。
- Required lenses: fresh-context requirements、adversarial；裁决修复后 fresh-context architecture 审完整最终候选；最终 PASS 才提交。
- Evidence: 146 core tests passed / 1 ignored scale benchmark；cargo check/build；隔离 core/CLI 四组烟测；签名 release app 构建和 deep/strict 验证；原生双路 8 秒无丢包；24 秒双路含 11 秒 DB 写锁，4 条 durable journal 继续积累且释放后全部入库；强杀系统音源 helper 时另一源完成 24 秒、core 存活；强杀 core 后子进程全退出、重启导入未丢已封块数据。原生详情见 batch-b-native.json；采集为隔离目录且实验结束已删除音频，没有操作个人库/暂停状态。
- Applicability: SQL/schema/journal/cleanup changes require relevant Rust/ingestion/cleanup tests；capture/native/process changes require process tests and signed native recheck；native fault runs当前覆盖已实现进程/日志路径，提交前将按最终候选重建签名包并跑原生短链。UI 未更改，无需凭空改变交互验收。
- Boundary carried into later batches: screenshot/export old staging startup IO 尚未拆离，列入 D 的完整 IO/读取边界；VAD/资源 governor/声纹 C；播放/读取/云治理/最终长跑 D。不要把当前批次绿灯说成全部优化完成。

## Findings and adjudication

- requirements R1：blocking。异步 journal 使睡眠尾部/被清理拆分的尾部未被迟到封块修正。为既有 `media_gaps` 添加可空 `tail_session_id`（不增加事实 owner），清理拆分保留归属，迟到确认只缩短/移除该 session 尾部，不能延长已关闭区间。
- requirements R2 / adversarial A1：同一 blocking。生产扫描先解析同 stem cleanup marker，再分类 `.opus`/`.pending`，同时移除两种已失效 orphan 记录；保留真正容量暂停的粘滞语义。
- adversarial A2：blocking。恢复/失败/睡眠生命周期尾部统一经过 core 的 source/generation/committed-time 清理裁决；记录和 session 状态同事务提交。
- requirements 补充候选 / adversarial 定向裁决 A3：blocking。首 128 个持续失败 session 可饿死健康后续 session。journal 采用索引 keyset 续扫；inventory 对未知文件每轮最多检查 128 个 session，保留游标续查，避免遗漏认领或长持 gate。
- 受影响验证：新增真实 cleanup apply + sleep/failure/recover + late journal 联合回归；生产扫描任意扩展名先到的容量回归；128 个旧目录持续错误、后窗正常媒体导入与 inventory 认领回归。核心 150 passed / 1 scale ignored。events.rs 仅 5 处 fixture 因 gap schema 增列改为显式列名，生产行为未改。
- Phase 1 定向复核：requirements PASS；adversarial PASS，独立真实 IPC 复验 A1 usage=0/pause=NULL、A2 cleanup overlap=0、A3 128 个失败旧 session 保留且后窗健康媒体导入。
- fresh-context final architecture：PASS。无新增阻塞/待决策；完整候选 owner、生命周期、批准机制及复杂度符合契约。
- 最终候选验证：150 core tests passed / 1 scale ignored、cargo check/build、隔离 CLI 四组烟测、签名 release 构建及 deep/strict 校验全部通过。SCK/Tap 双路 8 秒均 0 dropped；最终候选重跑 11 秒 DB 写锁、单来源强杀、core 强杀恢复三组通过，详见 batch-b-native.json。系统声短链为数字静音，不冒充语音质量证据。本批门禁完成，C/D 及最终长时/硬件/质量门槛继续保留。
