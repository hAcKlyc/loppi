# 本机处理可靠性：修复与验证

2026-10-20 更新，基于 `a926d39` 与本机冻结诊断材料。仓库只保存代码、复现入口及脱敏结果，原数据库、日志、音频、转录与凭据不入库。本轮交付 PR；没有替换正在录音的客户端，没有对历史音频发起新的付费识别。Batch 2 查询了原请求，并仅上传自生成的非私人语音作小样本验证。

## Issue 处置

| Issue | 本次处理 | 证据与剩余边界 |
| --- | --- | --- |
| #1 VAD 保留状态残留 | 已关闭区间可结束前块等待；无法再补证的旧状态保守保留媒体并解除阻塞 | 冻结库 42 个完整待判定块中 41 个收敛，19 个阻塞事件降至 8；3,820 个媒体引用与语音标志不变。活动尾块继续等待。保守留存可能增加少量噪声音频和后续 ASR 输入。 |
| #2 ASR 时间映射失败 | 移除新输入的长静音压缩接缝，复用现有分窗产生连续物理请求；严格映射不变 | 查回 10/10 原请求，全部因词时间落在/跨过压缩接缝而失败。旧输入与远端 ID 不改写；历史歧义需要显式重新精转，普通查询重试无法消除。请求数可能增加。 |
| #3 LLM 拟合失败 | observations 时间与 candidate 取交集；无效模型输出在原队列最多尝试 3 次 | 3 个现存失败窗都收到越过窗口结束的观察时间，最大约 17.7 秒。回归证明输入范围一致、物理证据不变、越界仍拒绝、重试有限。旧 LLM 原输出未保存，无法证明这就是全部历史失败原因；不自动重跑历史任务。 |
| #4 本地语音分析失败 | VAD drain 不返回零样本空片段；旧失败块一次本地升级重试 | 两个真实失败块：同一生产解码/重采样+旧 worker 均产生 `start=end`，整块发布失败；新 worker 均通过发布，每例补回 1 个当前结果粗转任务，并断言全部返回区间有持久覆盖、对应原媒体仍在。粗转按实际覆盖去重，避免后面的成功水位吞掉较旧重试。核心仍拒绝非法区间。迁移只触及保留媒体的 pending_vad 本地失败块，重试仍有 3 次上限。 |
| #5 采集中断 | 保留原生阶段/整数错误、子进程退出/信号、心跳/启动/进度/停止超时及回调错误类别 | 851 次 system 失败中 818 次结束于关屏期间；34 次 microphone 中 30 次在关屏期间。首个夜间长 session 中断与 WindowServer 关屏、replayd 停流相邻，仍不能推定全部因果。保持开放，需新错误码及睡眠/唤醒验收；不靠禁用后台采集隐藏失败。 |
| #6 错误不可定位 | VAD 日志关联 block job_id + session，标明解码/回放/调用/发布阶段；ingestion_retry 写具体步骤；采集/云错误细分 | 只写固定子码或整数，未知自由文本不进入日志。 |
| #7 卡片失败范围不一致 | 卡片与精转调度共用下一同源事件截断的稳定性范围和阻塞查询 | 回归覆盖范围内/边界上/范围外失败与不同来源；冻结库 18:58 卡片从不相关的拟合失败投影恢复为等待精转。 |
| #8 SCK 丢帧日志风暴 | 采集和探测同时注册 Audio/Screen 输出；非音频回调直接忽略，不保存图像 | macOS 15.7.1 arm64，同机 3 秒原生对照：旧/新均收到 144,000 音频帧，旧实现 4 条 `stream output NOT found`，新实现 0。短探测不能证明全天采集或睡眠稳定性。 |

## Batch 1 已执行验证

- `cargo test -p hear-core -p hear-worker --no-fail-fast`：265 项通过、5 项 opt-in 测试跳过。下面两项本次新增 opt-in 测试已使用私有夹具分别实跑通过；另外 3 项是既有手动/规模测试，不在本次变更验收范围。
- `cargo check -p hear-core -p hear-worker`、`cargo build -p hear-core -p hear-worker`：通过，包含原生桥接编译。
- `npm run typecheck`、`npm run lint`、`npm run build`、`node --experimental-strip-types --test scripts/test_cloud_task_status.mjs`：通过。Vite 仍有大于 500 kB chunk 的体积提示，与本修复无关。
- 对抗审查发现历史重试被 session 全局 coarse 水位吞掉、可能删除句首的缺陷，已在 coarse owner 改为按已有区间覆盖去重。回归覆盖 A 句首 / B 失败 / C 已完成后重试 B、多个覆盖区间之间补洞、重复回放、旧版本毫秒精度和清理墓碑；真实回放断言持久区间及媒体保留。
- 原生 SCK probe：旧/新同机对照及系统日志确认；只短时读帧计数，不保存录音或图片。

## Batch 2 验证与归因更新

- 原请求查询：隔离 Core 测试 bundle 使用项目原签名身份与已有 Keychain ACL，禁止授权弹窗；未导出凭据、修改 ACL 或替换安装版。查询 10/10 个已提交失败 ID 的供应商原响应成功，严格映射均返回 `provider_unmapped_timing_word_in_separator`。部分词跨 2—3 个原声区间，无法靠裁剪词时间唯一还原。此前“历史 ASR 响应已丢失”的结论撤回。
- 供应商合成 A/B：8 句自生成非私人中文、同一 PCM，旧接缝输入分别用默认分句和 VAD 分句提交，两者均映射失败；仅调分句参数不足以修根因。按新规划拆为 8 个连续请求后，8/8 时间映射通过，7/8 返回文字、1/8 为空结果。该探测额外断言全部有文字，**整项因此未通过**，不能当成完整识别质量验收；这里只支持映射修复。总计 10 个小请求、约 69 秒音频，无历史音频重送。
- 本地回归：长静音/未知缺口/采集缺口/清理范围均分开；0—3 秒短停顿保留线性坐标，连续录音仍按 20 分钟上限切分；旧映射解析、任务去重/复用/迁移、清理与纠错测试继续执行。LLM 回归验证窗口交集、保留物理证据、越界发布拒绝，5 类坏输出均第 3 次终止且不改变任务 ID。
- `cargo test -p hear-core -p hear-worker --no-fail-fast`：268 项通过、0 失败；6 项 opt-in 测试默认跳过，其中新原请求查询已单独实跑。`cargo check/build` 重验 Core 生产代码，前端/worker 未发生 Batch 2 实现变更，先前对应检查继续有效。
- 采集历史：关联冻结库中的失败 session 和 `pmset -g log` 的显示器开关状态；851/34 为 system/microphone 失败总数，818/30 为**结束时**关屏，810/27 为**开始时**关屏。另查 04:15:15—35 的 WindowServer/replayd 原生日志确认关屏与停流的先后关系。相关性不能证明剩余 33/4 次或关屏期间全部失败的具体原生原因；未修改采集策略。

原请求查询入口为忽略测试 `diagnostic_tests::query_original_failed_asr_without_resubmission`，需 `HEAR_QUERY_ORIGINAL_ASR=1`、`HEAR_TEST_DB`（只读冻结快照）、`HEAR_TEST_RESULTS`（私有输出目录）及项目签名身份。它只能查询原 ID，Meter 拒绝提交；原响应以 0600 新文件写入，包含私人转录，禁止提交仓库。没有同签名身份或已有 ACL 时直接失败，不放宽 Keychain 权限。

## 本地复现入口

先通过 SQLite backup 保存一致的数据库快照。`HEAR_TEST_DB` 指向冻结快照，**不要把活跃数据目录复制成开发服务目录并启动 serve**。测试在临时数据库副本写入，只读原音频，不启动采集、云队列或媒体删除器。

```sh
HEAR_TEST_DB=/private/path/snapshot.sqlite cargo test -p hear-core diagnostic_tests::snapshot_retention_and_event_projection -- --ignored --nocapture

HEAR_TEST_DB=/private/path/snapshot.sqlite \
HEAR_TEST_DATA=/private/path/hear-data \
HEAR_TEST_WORKER="$PWD/target/debug/hear-worker" \
HEAR_TEST_BLOCKS=block-uuid-1,block-uuid-2 \
cargo test -p hear-core diagnostic_tests::replay_saved_vad_through_production_decoder_and_publication -- --ignored --nocapture
```

VAD 回放使用生产的 Opus 解码、Sherpa 重采样、前块上下文、coarse 入队和发布事务。本次两例原来都没有 flush；入口按同样条件回放。它不重建历史进程的全部内存状态，但两例已在这个可恢复上下文下稳定复现并修复。

SCK 隔离检查（需已有屏幕采集授权；无授权退出 77，不发起授权请求）：

```sh
clang -fobjc-arc -fblocks -mmacosx-version-min=13.0 \
  scripts/test_sck_audio.m crates/hear-core/native/system_audio.m \
  -framework Foundation -framework ScreenCaptureKit -framework CoreMedia -framework CoreGraphics \
  -o /tmp/hear-sck-probe
/tmp/hear-sck-probe
```

随后用 macOS Console 或 `log show` 按该 probe 进程名查看是否出现丢帧错误；不要把正在运行的旧 hear 采集子进程日志混入新 probe。测试结果只报告帧数/峰值，无声环境峰值为 0 是允许结果。

## 2026-10-20 合入 dev 前复核

- 独立需求与对抗审查发现：升级重试块后接已完成块时，原调度仍按连续输入等待，可能遗留尚未输出的语音尾段。现仅在相邻块仍为可重试的 pending 时延后 flush，沿用原有 worker 收尾和发布流程；journal 就绪条件不变。
- 新回归覆盖后续块已完成、失败、重试耗尽和仍待处理四种情况，通过实际迁移、调度与发布检查粗转区间持久化和媒体保留。测试显式模拟 worker 的开放尾段契约，不声称新增真实音频模型验收。
- 本轮 Rust 测试 266 项通过、0 失败、5 项 opt-in 跳过，core/worker check/build 通过（含原生桥接编译）；前端 typecheck、lint、build 与 3 项状态测试通过。上文私有音频、冻结库及 SCK 实测记录属于 10/19，本轮未重新执行，也未操作日常客户端或活跃数据库。

## 安装发布前仍须执行（mandatory）

Batch 2 已在保留上述 dev 收尾修复后提交 [PR #10](https://github.com/hAcKlyc/hear/pull/10)，实现提交 `d6ca2cb`。最终组合 269 项测试通过、0 失败、6 项 opt-in 默认跳过；core/worker check/build 通过。需求、对抗及最终架构均 PASS，rebase 后新增 flush 生命周期的定向组合复核 PASS。PR #9 与 #10 已合入 dev（#10 merge `07b966b`）；都不能替代以下安装发布验收。

1. 将同一构建的 core 和 worker 一起安装，观察历史失败本地块只进行一次升级重试，已提交云任务仍使用原请求 ID；检查原声可播放和事件结果。升级后的恢复尚未在用户活跃库执行。
2. 持续双路录制，覆盖真实睡眠/唤醒、来源切换、许可撤回/恢复；检查封块连续性、真实缺口和新错误码。短探测不能替代这项验收。
3. 新产生的稀疏事件验证连续 ASR 分窗、跨请求人物合组及用量；旧接缝失败由用户明确重新精转才能生成新输入，不能把升级等同于历史云任务已恢复。LLM 检查坏输出的有限重试及最终失败可见；#5 按新子码继续定位，仍不能关闭。
