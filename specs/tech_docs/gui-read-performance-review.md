# GUI 详情加载与查询错误归因修复

基准为 2026-10-28 的冻结数据库：7 个事件、3834 条精转、3031 段保留音频；全库 40095 条精转、153417 个音频块。SQLite backup SHA-256：`a564fbcb351b9147949de55e8e31132e71dd4ed2bf70d07ece100e246bf5543d`。生产库、录音状态和 `/Applications/hear.app` 均未改动。数据副本留在本机临时目录，仓库证据只含计时、SQL 模板、计划和元数据。

## 结论与版本对照

PR #19 修好了 Agent context 的主要查询放大，但并未覆盖 GUI 的整个详情链路。先完成同一副本、配套 CLI/Core 的对照，再改查询代码。

| 操作 | 现装 0.1.10 / p21 | 原 0.1.11 候选 b4301d1 / p22 |
|---|---:|---:|
| 全天 timeline | 3047 ms，失败 | 1448 ms，成功 |
| context list 全天 | 3178 ms，query_timeout | 1239 ms，成功 |
| 170 分钟事件 read，limit 50 | 3068 ms，query_timeout | 387 ms，成功 |
| 12:00–15:00 read，limit 50 | 3038 ms，query_timeout | 301 ms，成功 |

原始记录：[首次 CLI 对照](evidence/gui-read-performance/initial-cli-comparison.json)。这轮机器有其他编译负载；不把它与后续低负载测量拼成受控加速倍数。

## 根因与修复证据

1. **完成度查询缺少任务索引。** `transcription_progress` 为每个 ASR 任务检查片段，计划选择 `precise_segments_time`，按时间扫描其他任务的片段。添加 `(task_id, started_ms, ended_ms)` 索引，查询能按当前任务和边界读取。
2. **每批版本计算重新物化全库分组。** `revision_parts` 左连接 UNION 视图 `precise_group_membership`，计划含 `MATERIALIZE precise_group_membership / SCAN p`。每批扫描约 4 万精转，首屏共 64 条 SQL、444644 次 full-scan step。改为按片段主键连接人工分组与自动分组；人工覆盖优先和 revision 的编码语义保持一致，既有 revision/重转/绑定测试通过。
3. **说话人列表先连接两个复合任务视图，再过滤事件。** 旧计划扫描历史精转。现在先解析 seed 的当前 target/version，再查询该目标；返回候选仍覆盖所有分页，不用当前首屏推断身份。
4. **失败统计扫描全部历史音频。** 为终止失败的 audio/coarse 记录增加小型 partial index。索引在各表就绪后创建，新库和旧库路径均跑回归。
5. **正文被说话人请求阻塞。** 原前端必须等待 `event_detail → session_speakers` 才设置 `detailReady`；说话人超时甚至把成功返回的文字当作失败。现在详情返回即展示文字并启动现场记录，说话人结果和失败独立处理。
6. **每次翻页重复取前面的所有页。** 默认首批由每种转录 100 条减为 30 条，后续分页仅追加新页。后台刷新仍重新读取已加载范围，页移动产生重复时保留既有一致性重读逻辑。时间点定位仍会读取到覆盖目标的页，未加入新的时间索引协议。

没有新增结果缓存、自动重试或延长 3 秒 deadline。没有逐条读取音频/截图，首屏请求也不包含媒体二进制。

同一 debug 构建口径，直接调用实际 Core 查询函数（不是 UI 或网络计时）：

| 查询 | 修复前 | 修复后 | SQL 次数 |
|---|---:|---:|---:|
| timeline | 2803 ms | 883 ms | 117 → 117 |
| event_detail，100 条 | 1626 ms | 229 ms | 64 → 64 |
| session_speakers | 5086 ms | 482 ms | 129 → 130 |

收益来自消除扫描放大，不是把 SQL 调用次数包装成较小数字。详情 JSON 序列化约 1.8 ms。SQLite profile 对仍打开的结果游标计时含内部嵌套查询，不能将各条耗时直接相加。完整 SQL 模板、调用次数和查询计划见 [SQL profile](evidence/gui-read-performance/sql-profile.json)。

## 实际点击与 payload

使用已封装、身份核验为 **b4301d1 / dirty=false** 的原签名候选包，与本次独立 release 构建比较；相同数据、事件、点击操作。浏览器为 Playwright WebKit，React development 构建，连接真实 Core Unix socket。每轮先用不同分页键淘汰现有单页结果缓存。**测试桥代替了 Tauri invoke，不能声称测到了原生宿主 IPC；最终安装包仍需装机验收。**

| 指标 | 原候选 | 本次修复 |
|---|---:|---:|
| 点击至正文可读，5 次 | 失败、3317、3524、3271、3255 ms | 588、146、139、136、145 ms |
| 成功样本中位数 | 3294 ms（4 次） | 145 ms（5 次） |
| 详情响应 | 68994 bytes | 22663 bytes（减少 67.2%） |
| 粗转数组 | 22332 bytes / 100 条 | 6770 bytes / 30 条 |
| 精转数组 | 42559 bytes / 100 条 | 11790 bytes / 30 条 |
| 摘要 | 3520 bytes | 3520 bytes |
| session_speakers 是否阻塞正文 | 是；约 2.1–2.3 s，另 1 次 3 s 超时 | 否；与已可读正文/现场记录并行 |

修复后第 2–5 次的分段：Core 请求执行约 **81–82 ms**（含序列化 0.016 ms），Unix socket 往返 **82–90 ms**，浏览器 React 累计提交时间 **28–30 ms**；点击至下一可见帧 **136–146 ms**。这些是嵌套测量区间，不能全部相加。额外时间包含桥接、调度与帧等待。

首个修复样本 **588 ms 未达到 300 ms**，其中 Core 为 455 ms、socket 为 469 ms。现有证据将该波动定位在 Core 执行/调度区间，尚未进一步区分 OS 调度和页读取；不称为已经解决的冷启动问题，也不承诺所有操作都小于 300 ms。点击很靠后的时间点需要多页读取，不属于上述默认首屏口径。

最终同组 CLI：timeline 1200→819 ms，context list 1133→425 ms，事件 read 320→296 ms，三小时 read 236→229 ms；均成功。单次计时有系统负载波动。记录：[原候选](evidence/gui-read-performance/release-before.json)、[修复版](evidence/gui-read-performance/release-after.json)。

构建隔离事故说明：原独立 source 的 `target` 是指向共享缓存的 symlink；最初 `cp -cR` 保留了链接，修复构建更新了共享 target 中的产物，已通知发布会话。原 source 和已封装签名 .app 没有改动。发现后排除混合基准，改用真实目录克隆，并用已封装候选重做上述对照；最终修复构建位于 `/tmp/hear-gui-review/isolated-target`。发布必须从最终 HEAD 重打包。

## 错误归因与日志

两条首页文案共用同一 `timelineError`；不是两个独立查询都判定日期无效。旧 Core 把 timeline 的所有 `Err` 改写为 `invalid_time_range`，把详情的所有 `Err` 改写为 `event_detail_unavailable`。`Response::Error` 被当作正常返回，只有向外传播的 `Err` 才记 `ipc_execute_failed`。现场记录的“暂未加载”来自前端在详情失败后不发起 moments 的依赖分支。

现将超时、忙、锁、参数无效、事件不存在、未知数据库错误分别映射为 `query_timeout / read_busy / db_locked / invalid_* / event_not_found / query_failed`。语义错误保持真实类别；未知底层错误不冒充用户输入错误。

所有 `Response::Error` 及 Agent context 失败都会记 warn。日志含 `request_id / correlation_id / context.command / error_code / context.cause / duration_ms / context.output_bytes / context.serialize_us`。向外传播的执行错误和写响应失败也记录命令与受控原因；可解码请求遇到执行错误会收到关联响应，不再直接断开。前端使用同一请求 ID，记录不含请求正文的失败元数据。

原因采用有限代码（如 `db_schema / db_constraint / db_io`），不把 SQL、用户转录或供应商响应写入日志。Core 不可达时只有前端控制台的 transport 记录，不能声称服务端仍能持久化日志。

## 验证与复现

- `cargo test -p hear-core -p hear-protocol --locked`：Core 361 通过 / 19 显式忽略；protocol 14 通过。
- `npm run typecheck`、`npm run lint`、`git diff --check` 通过。
- `node --test scripts/test_speaker_picker.mjs scripts/test_detail_entry.mjs`：5 项通过，包含协作会话的指派集成。
- WebKit：`test_detail_entry.html / test_detail_loading.html / test_page_loading.html / test_date_navigation.html / test_gui_read_boundaries.html` 全部 PASS。覆盖异步失效、定位、刷新不抢位置、首页/详情超时文案、说话人失败隔离、仅取下一页、重试与关联 ID。
- `scripts/smoke_diagnostics.py`：临时 Core、参数错误、缺表错误、真实 3 秒查询 deadline（timeline 与 event_detail）、所有失败日志的命令/原因/ID，以及 read/execute/write IPC 阶段回归通过。不触及生产库或云端。

可复现的性能命令（先提供关闭的 SQLite backup，安装版本对应的 Playwright/WebKit；不会自动安装浏览器）：

```sh
npm run dev -- --host 127.0.0.1 --port 5198 --strictPort
HEAR_PLAYWRIGHT_MODULE=/absolute/path/to/playwright-core/index.mjs \
  node scripts/measure_gui_reads.mjs CORE_BIN SNAPSHOT.sqlite OUTPUT.json
# CLI 不与 Core 同目录时指定 HEAR_BENCH_CLI；第五个参数可传旧版 DetailDrawer 的 Vite 路径。
HEAR_REVIEW_SNAPSHOT=/absolute/path/to/SNAPSHOT.sqlite \
HEAR_GUI_PROFILE=/tmp/gui-profile.json \
  cargo test -p hear-core gui_read_profile -- --ignored --nocapture
HEAR_TEST_CORE=/absolute/path/to/hear-core HEAR_TEST_CLI=/absolute/path/to/hear \
  python3 scripts/smoke_diagnostics.py
```

原生 Tauri 全链路时延、最终签名包安装及生产 GUI 验收未在本轮执行；不替换安装版。版本、打包脚本未修改。
