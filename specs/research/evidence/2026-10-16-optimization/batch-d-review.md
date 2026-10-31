# Batch D Review Contract

- Mode: repair。主 Agent 裁决、修改；reviewer 只读，不得派生 reviewer。
- Requirement source: specs/prd/devplan_261016_reliability-performance.md；已接受研究 F09–F14 与阶段 D，附本批对 F01 的规模查询及 F04 启动恢复收尾。用户要求全部 P0/P1，交互功能不变。
- Exact baseline: 1ba00e6dbfa990d440a85fe9bce0ec5ed46ccb15；target = 当前工作树的下列完整候选。
- Out of scope: 已提交 A/B/C 代码本身（只有被 D materially exposed 的缺陷入本轮）；既有未跟踪 design/playground 和 research 分析/旧 evidence。整体24h/72h、最低macOS13/8GiB、真实标注留出质量属于最终原子交付 mandatory 门槛，不能将本批内部提交当成可发布交付；最终跨批审查另做。不能据此降低门槛或宣称通过。
- Must guarantee: 现有页面/CLI/功能/来源和真实时间语义不变；core DB为唯一业务事实；已有人工修订/revision兼容；清理屏障和外发范围不扩大；暂停不被覆盖；读取不占业务写锁、快照与缓存有界；双块播放按音频时钟接续，seek/切源/关闭/本页清理失效；云任务真实身份/query-before-submit保留，持久限流和公平调度；非关键启动IO后台有界恢复；诊断无音频、正文、密钥且采样有界。
- Non goals: 新UI、换模型/阈值、数据库服务化、取消fsync、自动删除历史人声、重启core掩盖泄漏、不能声称OS DNS/Keychain/文件系统同步调用绝对可取消，不能把短测当长跑。
- Owned defects: D 引入/显著暴露的查询快照/修订/播放取消/云队列/恢复/指标和采样器缺陷，以及上述研究明确要求。
- Approved mechanisms: 2只读SQLite连接、3秒获取/SQL预算、每连接1页4MiB data_version缓存；批量revision和原字节摘要；音频时钟双块预取单在途；原provider单任务owner的deadline/typed failure/Retry-After/jitter公平；已有governor/cleanup_gate下的分批后台恢复；固定直方图与replace-only诊断、PID birth/coalition归属及限额轮转采样。不新增业务owner。
- Required lenses: fresh requirements + adversarial 固定快照并行；主 Agent裁决修复、受影响验证和定向复核；fresh architecture 审完整最终候选 PASS 后内部提交。
- Evidence: 176core/4CLI tests，19前端测试、3采样覆盖测试、cargo check/build、TS/lint、隔离CLI四组通过。见 batch-d-native.json (签名SCK/Tap同时双路12s)、batch-d-gui.json (真实WKWebView)、batch-d-fixture-coverage.json (公开20s逐块解码)、batch-d-scale.json (30/90天实际SQLite，365天仍运行)。签名短链早于最后音频区间索引；索引对应规模证据已更新，最终签名包重建/短链在提交前更新。短样本与采样峰值不代表24h。
- Checks invalidation: 读取/SQL/revision变更重跑对应Rust+规模；播放变更重跑TS/前端取消调度测试+WK关键路径；cloud变更重跑本地TCP慢body/429及调度任务身份测试；capture/指标/source变更重跑签名native；恢复变更对应Rust/隔离启动；仅文档不使无关检查失效。

## In-scope files

- `Cargo.lock`
- `crates/loppi-core/Cargo.toml`
- `crates/loppi-core/src/archive.rs`
- `crates/loppi-core/src/capture.rs`
- `crates/loppi-core/src/cloud.rs`
- `crates/loppi-core/src/events.rs`
- `crates/loppi-core/src/knowledge.rs`
- `crates/loppi-core/src/main.rs`
- `crates/loppi-core/src/moments.rs`
- `crates/loppi-core/src/reads.rs`
- `crates/loppi-core/src/record.rs`
- `crates/loppi-core/src/recovery.rs`
- `crates/loppi-core/src/resources.rs`
- `crates/loppi-core/src/source_process.rs`
- `crates/loppi-core/src/storage.rs`
- `scripts/audit_recording_coverage.py`
- `scripts/macos_rusage.c`
- `scripts/measure_macos.sh`
- `scripts/report_macos_resources.py`
- `scripts/sample_macos_resources.py`
- `scripts/test_playback.mjs`
- `scripts/test_resource_sampler.py`
- `specs/ARCHITECTURE.md`
- `specs/prd/devplan_261016_reliability-performance.md`
- `specs/research/evidence/2026-10-16-optimization/batch-d-fixture-coverage.json`
- `specs/research/evidence/2026-10-16-optimization/batch-d-gui.json`
- `specs/research/evidence/2026-10-16-optimization/batch-d-native.json`
- `specs/research/evidence/2026-10-16-optimization/batch-d-review.md`
- `specs/research/evidence/2026-10-16-optimization/batch-d-scale.json`
- `src/App.tsx`
- `src/DetailDrawer.tsx`
- `src/playback.ts`

## Findings and adjudication

- Requirements 初审：代码需求无已确认 blocker；365天与最后签名短链为 required evidence gap。
- Adversarial A1/P2 blocking：下一块预取失败会立刻停止已成功排程的当前块，违反原播放失败顺序。主 Agent确认，修复归现有播放器owner；重跑前端取消/成功/错误测试与真实WK关键路径。
- Adversarial A2/P2 blocking：来源在最后心跳后正常报错退出，最终Progress漏发已测的capture计数，可能隐藏最后overflow。主 Agent确认；修复归Progress最终快照；定向错误帧测试与签名native复验。
- 主Agent E1/required evidence gap根因：365天timeline_day触发3秒SQL取消，不能放宽deadline。实际SQLite3.53.2 EXPLAIN显示OR-source导致audio_blocks_source_time扫旧历史。以同义有界来源集合表达保留范围语义；同谓词playback::chunk纳入直接修复范围（crates/loppi-core/src/playback.rs），不新增owner/机制。重跑30/90/365天和读取/播放回归。
- 后续：以上为原契约的定向修复与证据闭环；最终架构审全部候选。

- Adversarial定向代码PASS：A1/A2关闭，独立5项播放器测试及CASE/IN语义矩阵通过。177 core /21前端 /3采样，最终签名native与真实WK修复路径均通过；365天定向复测仍进行中。

- Required evidence gaps已补齐：最终修复签名core SHA256 fc27be0e8c3b2b9ad4c941677d9bb4296dbff3af280eeb3b2517ee6ecab6a738，SCK/Tap各双源12秒0丢包，真实WK故障边界与正常连续20秒通过。实际SQLite 30/90/365天全部在3秒短快照内通过；365天12,614,400块，时间线median1.873ms/max6.580ms、100句详情median29.640ms、FTS全历史median141.525ms。原超时与修复原因保留在scale JSON。

- Architecture发现A3/P2 blocking：新session设备打开前capture=None时，Governor沿用上一个session/PID指标，造成失败计数错归属。主Agent确认；只允许相同session+PID继承缺省指标，新owner起点为None。新增同会话保留／跨session与PID清空／inactive快照回归；不改变采集或资源准入。先前运行包2h观察必须标记旧SHA，不充当这项修复证据。

- A3定向architecture PASS、adversarial PASS；requirements定向PASS已关闭365天与签名证据缺口。最终单独签名包core SHA256 `999f41ae216592d55780e01268c20b72f108d919c04d82929deb66f18a5d269c`，deep/strict及SCK/Tap双源短链通过。全workspace 191 tests（178core+9desktop+4protocol）/1 scale单独执行、check、TS/lint、21前端、3采样通过。前端仅格式调整后的构建JS hash与已实测WK包完全相同；A3仅诊断归属不使此前GUI/规模/云证据失效。本批内部提交门禁全部通过；全局仍in-progress。
