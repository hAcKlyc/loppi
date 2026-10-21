# 诊断与开发验证

## 状态与所有权

Core 仍是唯一业务状态 owner。`hear diagnose` 通过协议 18 的只读请求，使用 `reads::Pool` 的 3 秒查询快照。SQL 只选择 ID、状态、时间、数量、模型/策略标识和错误码，之后释放快照再读取轮转日志；两者有不同观察时间，报告不承诺跨文件原子快照。它不复制数据库、录音、文字、标题、截图、供应商正文或凭据，也不提交后台任务。

报告是临时投影，不参与调度、重试或 UI 状态裁决。无需维护新的状态库、日志服务器或故障恢复框架。CLI 在 Core 未运行时直接失败，不能为了诊断悄悄启动处理服务。协议版本不匹配时应使用与安装包配套的 CLI。

## 使用

```sh
# 本组件编译身份；不会打开 UI、模型或服务
./target/debug/hear build-info
./target/debug/hear-core --build-info
./target/debug/hear-worker --build-info
./target/debug/hear-desktop --build-info

# 毫秒 UTC，左闭右开，最多七天；通常按当地一天换算边界
hear diagnose --from-ms 1792339200000 --to-ms 1792425600000 --output report.json
hear diagnose --from-ms 1792339200000 --to-ms 1792425600000 --event-id <uuid>
hear diagnose --from-ms 1792339200000 --to-ms 1792425600000 --job-id <uuid>
hear logs --from-ms 1792339200000 --to-ms 1792425600000 --capture-span-id <uuid>
```

`--output` 创建 0600 新文件，拒绝覆盖已有目标；不指定时打印 JSON。报告仍有时间、稳定内部 ID 与技术元数据，分享前按受众检查。工具不自动上传。

- `core_build` 来自服务进程编译时身份，`cli_build` 来自当前 CLI。不能拿磁盘新二进制的身份替代旧运行进程。GUI 的 `build_info` command 和 `gui.opened` 日志报告 GUI 自己的身份；worker 支持无模型启动的 `--build-info`。它们并不冒充“诊断时所有进程都已匹配”。
- `cloud_tasks` 按事件通过 candidate links / ASR refs 解析关联物理任务，再与时间、job 条件取交集。失败/待处理优先，最多 1000 条；`cloud_summary` 聚合整个筛选结果。历史关联可保留，不等于当前任务仍可发布。
- `window_local_jobs` 是时间窗内未完成 VAD / 粗转任务；传 job 时也可查已完成任务。每阶段最多 1000 条。`window_local_processing`、`window_capture_coverage` 始终覆盖整个时间窗，不随事件 ID 缩小，避免把某个事件的状态冒充设备总体健康。
- 覆盖按每来源区间并集合并，不能把重叠块重复累计。录制 session 声明区间、封块清单覆盖与仍保留媒体覆盖分开；session 的存在不证明其整个区间采样成功。每来源块/缺口最多各 100,000 行，超过时明确标记下界。尚未封块的活动尾部、空闲、已清理数据及未知时间均可能落入 uncovered，**不能据此直接算漏录率或语音召回**。保留媒体也未在此解码验证；需要音频完整性用 `audit_recording_coverage.py --decode`。
- `window_usage` 按实际调用时刻统计 requests、accepted、已知 token/音频/费用和 incomplete；未知不视为零，不能按音频事件时间归账。
- 队列年龄基于当前未结束任务的创建时间；完成 elapsed 包括排队。云任务 failed_attempts 只在失败时递增，正常轮询不计入；completed_after_failure 统计当前计数仍大于零且已完成的任务。手动重试或迁移重置过的更早失败不可凭该字段还原，需结合保留日志。VAD 的 attempt_counter 是领取次数，coarse 的 attempt_counter 是失败次数，不能混用。
- `logs.retained_from_ms/to_ms` 是当前轮转文件可解析记录的最早/最晚时间，不保证其中没有丢失。`malformed_lines`、`truncated` 和 `logs_available` 分别表示坏行、返回记录截断、日志不可用。最多返回 500 条，但 `matched` 和最多 128 类 `failure_counts` 统计全部匹配记录；分类超限单独标记。事件过滤可联查报告中列出的云任务日志；任务列表截断时事件日志依赖展开也不完整。
- 日志的时间是故障发生时刻，DB 的筛选是任务音频时间范围，历史音频延后处理可能需要扩大日志查询时间窗。VAD 失败的 `context.start_ms/end_ms` 保留领取时的原音频范围，即使该块不在故障发生窗口的 DB 查询结果中，仍可定位处理对象。

## 已保存 ASR 结果的离线发布回放

`diagnostic_tests::replay_oct21_asr_publication` 为显式 ignored 的私有回归：`HEAR_TEST_DB` 指向 SQLite backup 冻结库，`HEAR_ASR_RESPONSES` 指向原请求响应目录。测试只在临时数据库副本调用生产解析/映射/发布和迁移，核对 18 个请求、779 个句段、重复发布、事件状态及暂停缺口；不启动采集、不调用云端。私有正文不进入仓库，默认 CI 使用合成回归。调查与安装边界见 [10/21 精转发布失败调查](../research/2026-10-22-asr-publication-failures.md)。

历史日志中的 `provider_unmapped_timing_audio_coverage` 表示旧版以当前原声留存过滤云端文字。当前已删除该过滤；文字可用、原声回放、声纹质量分别核对，不能据声纹不可用推断精转失败。查询远端原 ID 与离线发布是不同操作；普通队列重试遇到远端不存在仍可能重传，不能标成 query-only。

## 日志写法

继续使用已有 `LogEvent`/`LogWorker`。字段为固定代码、UUID 或数值；禁止 `error.to_string()`、转录、请求/响应正文或路径直接进入事件。未知错误降为固定分类。`LogContext` 用 typed 字段，不能用任意 JSON map 加料。

- `speech.vad_complete` 在日志线程汇总为 `vad_completed_summary`（按来源计数；读日志时也会刷新）。故障事件仍逐条保存。汇总不绑定单个 job/session，不据此反推某个 block 已完成。
- `speech.coarse_complete/coarse_failed` 只代表真实 SenseVoice 粗转阶段；清理或重复发布使结果未落库时记录 `coarse_not_published`，不冒充完成或故障；旧版本同名事件曾代表 VAD，分析旧日志须按构建版本区分。
- 云任务记录 provider、音频范围、尝试次数、单次处理耗时；不完整输出保留固定 finish_reason 分类，JSON 解析失败带行/列和字节数，不保存原输出。`fit_published` 的事件数写 `context.count`，不再冒充 duration_ms。
- 采集失败保留原生阶段/整数错误、来源和已确认帧数，另记录主显示器当时是否休眠。后者只是 [CGDisplayIsAsleep](https://developer.apple.com/documentation/coregraphics/cgdisplayisasleep(_:)) 的只读观察，不等于机器睡眠、不解释全部失败因果，也不控制是否录制。
- 无需给所有成功高频事件逐条打日志；任务完成事实已有数据库。失败去重不跨 job 吞记录，以免下一次调查失去关联。

## 一个验证入口

```sh
npm ci
npm run verify
# 等价；可保存可机器读取的结果
python3 scripts/verify.py --report build/verification.json
# 调试时按变更选择，最终 PR 跑 all
python3 scripts/verify.py --suite static
python3 scripts/verify.py --suite test
python3 scripts/verify.py --suite smoke
```

macOS、Node 24、当前稳定 Rust、Xcode 26.0.1 或更新版本（含 macOS 26 SDK）是前提；现有 Audio Tap 代码引用该 SDK 的字段，部署最低版本仍为 macOS 13。GitHub Actions 采用相同入口与 [macos-15 runner](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-arm64-Readme.md)。CI 通过 `DEVELOPER_DIR` 选择 runner 已安装的 Xcode 26.0.1，避免默认 Xcode 16.4 头文件缺字段。测试依赖由 package-lock/Cargo.lock 固定，React 交互测试不再依赖开发者 `/tmp` 下手动安装的 jsdom。

2026-10-22 验证发现一个既有入口问题：unittest discovery 会导入手动 `scripts/test_model_preparation.py` 并把 `discover` 当作模型目录，导致默认 `all` 返回失败。该手动入口不属于本次修复；普通 Python 模块的 6 项测试、native entry policy 和 `--suite smoke` 已独立补验通过，完整结果见本次 devplan。不能把这些结果写成默认 `all` 已通过。

默认门禁包含前端静态检查、Rust workspace check/test、Node 逻辑/React 交互、Python 资源测试、真实 native entry policy、全工作区构建、四组件身份校验、隔离 Core 和诊断导出烟测。禁用私有 fixture 环境；不启用 ignored 测试、不读用户活跃库、不调用付费 API。脚本及 CI 报告明列未覆盖项。

默认门禁复用生产函数的合成最小回归，避免复制一份“模拟处理器”：

| 真实故障结构 | 默认回归 |
| --- | --- |
| VAD 尾段/旧失败块、保留卡住、历史覆盖空洞 | `coarse::tests`、`tests::upgraded*` / retention tests（Rust suite） |
| ASR 跨长静音接缝、真实采集缺口 | `events::tests::asr_windows_cut_long_silence_but_keep_short_pauses_and_real_origins`、`persisted_request_clock_owns_text_while_voice_requires_actual_audio` |
| 暂停意图早于最终封块、迟到/清理分片与旧数据 | `ingestion::tests::confirmed_final_audio_shortens_only_its_session_intent_gaps`、`intent_gap_correction_survives_cleanup_split_and_late_upgrade_journal`、`intent_gap_open_after_confirmed_tail_and_old_gap_migration_are_idempotent` |
| observation 越出 candidate、无效输出无限重试 | `fit_input_intersects_crossing_observations_without_retiming_evidence`、`invalid_fit_output_retries_are_bounded_and_keep_task_identity` |
| 失败日志泄漏正文、查询漏报/错误归因 | `unilogs::tests`、`diagnostic_report::tests`、`cloud::tests::malformed_output_preserves_only_numeric_parser_context`、`scripts/smoke_diagnostics.py` |

真实模型的独立端到端检查需要显式准备公开 WAV、ffmpeg 和已有模型：

```sh
python3 scripts/verify_ingestion_quality.py \
  --core "$PWD/target/debug/hear-core" --cli "$PWD/target/debug/hear" \
  --models /private/path/models --wav /private/path/public-speech.wav \
  --output /private/path/ingestion-report.json
```

三种生命周期（正常、迟到封块、异常退出恢复）运行同一生产解码/保留/粗转链，不启用云服务。真实私有失败块回放及查询历史远端请求仍遵循[本机处理验证](local-processing-verification.md)，不能放进默认 CI。浏览器视觉、权限、关屏、睡眠、设备切换及签名安装属于单独发布验收，不因 CI 绿灯自动算通过。

## 构建与迭代流程

每个任务从当前 dev 建独立分支/工作树；一个开发 agent 写一份改动，review agent 只读。PR 写清代码提交、检查证据和实际安装版本，避免“已 merge”被当作“本机已运行”。

`scripts/build_macos.sh` 为 Cargo/Tauri 注入同一个 `HEAR_BUILD_EPOCH`，读取四个最终二进制的编译身份，任何混合构建直接失败。清单写入 bundle `Contents/Resources/build-info.json` 后再签名。版本包含 SHA、dirty、构建秒、协议版本；dirty 是编译时的工作树状态，不证明构建内容可由 SHA 单独复现。

沿用运行中 build bundle 禁止覆盖的保护。验证只使用隔离数据目录；日常录制运行 `/Applications/hear.app`。发布仍须整包停止/替换/重新启动并确认运行身份；本 PR 不安装、不重启现有客户端。

LLM 返回 evidence IDs 后让 Core 导出时间看似更简单，但 coarse observation 可覆盖同一段内多个主题，只选择整段 ID 会丢失片段内部毫秒切分；要保留精度仍需额外边界协议。本轮继续当前毫秒契约与严格校验、有限重试，不在可观测性改动中切换整理语义。

2026-10-22 的 0.1.4 打包检查将上述手动模型入口重命名为 `scripts/smoke_model_preparation.py`，避免普通 unittest discovery 导入需要显式模型目录的脚本。它仍是单独运行的模型烟测，不加入默认离线门禁。
