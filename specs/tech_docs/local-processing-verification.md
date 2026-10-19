# 本机处理可靠性：修复与验证

2026-10-19，基于 `a926d39` 与本机冻结诊断材料。仓库只保存代码、复现入口及脱敏结果，原数据库、日志、音频、转录与凭据不入库。本轮交付 PR；没有替换正在录音的客户端，也没有调用云端重跑。

## Issue 处置

| Issue | 本次处理 | 证据与剩余边界 |
| --- | --- | --- |
| #1 VAD 保留状态残留 | 已关闭区间可结束前块等待；无法再补证的旧状态保守保留媒体并解除阻塞 | 冻结库 42 个完整待判定块中 41 个收敛，19 个阻塞事件降至 8；3,820 个媒体引用与语音标志不变。活动尾块继续等待。保守留存可能增加少量噪声音频和后续 ASR 输入。 |
| #2 ASR 时间映射失败 | 细分词时间、文本覆盖、接缝、原声覆盖等子码；严格校验不变 | 历史供应商返回未保存，无法确定当时分支；保持开放，不自动付费重送。 |
| #3 LLM 拟合失败 | 分开边界倒序、越界、同源重叠及 JSON 错误类别 | 历史输出未保存，无法恢复具体提案；保持开放，未放宽范围或增加云重试。 |
| #4 本地语音分析失败 | VAD drain 不返回零样本空片段；旧失败块一次本地升级重试 | 两个真实失败块：同一生产解码/重采样+旧 worker 均产生 `start=end`，整块发布失败；新 worker 均通过发布，每例补回 1 个当前结果粗转任务，并断言全部返回区间有持久覆盖、对应原媒体仍在。粗转按实际覆盖去重，避免后面的成功水位吞掉较旧重试。核心仍拒绝非法区间。迁移只触及保留媒体的 pending_vad 本地失败块，重试仍有 3 次上限。 |
| #5 采集中断 | 保留原生阶段/整数错误、子进程退出/信号、心跳/启动/进度/停止超时及回调错误类别 | 不能用 SCK 日志噪声解释所有历史中断；保持开放，需新运行错误码及睡眠/唤醒验收。 |
| #6 错误不可定位 | VAD 日志关联 block job_id + session，标明解码/回放/调用/发布阶段；ingestion_retry 写具体步骤；采集/云错误细分 | 只写固定子码或整数，未知自由文本不进入日志。 |
| #7 卡片失败范围不一致 | 卡片与精转调度共用下一同源事件截断的稳定性范围和阻塞查询 | 回归覆盖范围内/边界上/范围外失败与不同来源；冻结库 18:58 卡片从不相关的拟合失败投影恢复为等待精转。 |
| #8 SCK 丢帧日志风暴 | 采集和探测同时注册 Audio/Screen 输出；非音频回调直接忽略，不保存图像 | macOS 15.7.1 arm64，同机 3 秒原生对照：旧/新均收到 144,000 音频帧，旧实现 4 条 `stream output NOT found`，新实现 0。短探测不能证明全天采集或睡眠稳定性。 |

## 已执行验证

- `cargo test -p hear-core -p hear-worker --no-fail-fast`：265 项通过、5 项 opt-in 测试跳过。下面两项本次新增 opt-in 测试已使用私有夹具分别实跑通过；另外 3 项是既有手动/规模测试，不在本次变更验收范围。
- `cargo check -p hear-core -p hear-worker`、`cargo build -p hear-core -p hear-worker`：通过，包含原生桥接编译。
- `npm run typecheck`、`npm run lint`、`npm run build`、`node --experimental-strip-types --test scripts/test_cloud_task_status.mjs`：通过。Vite 仍有大于 500 kB chunk 的体积提示，与本修复无关。
- 对抗审查发现历史重试被 session 全局 coarse 水位吞掉、可能删除句首的缺陷，已在 coarse owner 改为按已有区间覆盖去重。回归覆盖 A 句首 / B 失败 / C 已完成后重试 B、多个覆盖区间之间补洞、重复回放、旧版本毫秒精度和清理墓碑；真实回放断言持久区间及媒体保留。
- 原生 SCK probe：旧/新同机对照及系统日志确认；只短时读帧计数，不保存录音或图片。

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

1. 将同一构建的 core 和 worker 一起安装，观察历史失败本地块只进行一次升级重试，已提交云任务仍使用原请求 ID；检查原声可播放和事件结果。升级后的恢复尚未在用户活跃库执行。
2. 持续双路录制，覆盖真实睡眠/唤醒、来源切换、许可撤回/恢复；检查封块连续性、真实缺口和新错误码。短探测不能替代这项验收。
3. 在新失败样本中读取 #2/#3/#5 子码，补充最小脱敏复现再修根因；不把本次诊断改进称为这些历史失败已修复。
