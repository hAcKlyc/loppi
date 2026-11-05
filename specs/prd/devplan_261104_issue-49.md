---
status: in-progress
updated: 2026-11-04
---
# Issue #49：睡眠恢复与事件覆盖

目标：已有音频自动处理，处理状态不被历史缺口覆盖；覆盖按事件范围和声源独立展示，保留真实原因、精确时长，不伪造音频或强拆事件。完整需求见 https://github.com/hAcKlyc/loppi/issues/49 。

## 执行台账
### 开发契约
- 必赢场景：全部有效任务完成的跨睡眠事件显示精转完成，同时显示历史缺口；混合空结果、失败、裁剪保留自身状态。
- 原子交付：整个 issue 验收；真实 OS 验收未完成前不得宣称可发布。
- 必须保证：同源范围裁剪、区间并集计时、清理与失败可见，历史数据即时投影无需重转。
- 不做：补造睡眠音频、强拆语义事件、新增恢复 owner、重试循环或持久状态。
- 状态来源：Core SQLite 的 media_gaps/audio_blocks/clear_ranges 和 transcript_tasks；GUI 只展示派生结果。
- 复用：capture_supervisor、capture_retry_delay、source_process 耐久进展监督、record_sleep_gap、ASR gap guard、TimelineItem。
- 新增机制：仅可选覆盖协议投影，无 schema/调度变化。
- 架构：Core 唯一写入者，采集 session 与语义事件解耦。
### 验收与结束条件
- 单测验证完成/空结果/失败不被覆盖信息遮蔽，重复交叉缺口合并、来源隔离、毫秒裁剪、清理可见。
- 静态检查、相关 Rust/Node 测试、开发构建及三路 review。
- mandatory：合成音频真睡眠/唤醒、短睡眠/重复/DarkWake/灭屏、设备暂不可用、策略阻断、崩溃重启实机检查；未获得证据保持 in-progress。
- 代码改变后重跑对应层验证；文档不影响二进制证据。
### 开发批次
- Batch 1：状态与覆盖投影、UI、回归及既有恢复链路核查。
  - [x] 实现与测试
  - [x] 最终开发包构建与三路审查
  - [ ] 实机门槛
### Review 基线
- base: 0d66d497af04ca9c733d34297f8fab5ea9c60f59
- 范围：本分支所有改动；初始工作树干净。
### 待用户决策
无。
### 进展日志
- 已确认 event_state 将任意缺口置于 complete 之前；已有睡眠监督、退避重试、落盘看门狗和故障/睡眠切分，优先验证复用。

- 完成：Core 同快照派生每声源覆盖并集，协议可选字段，列表与详情独立展示。处理状态仅由当前任务结果决定，历史事件无需重转。
- 三路独立 review：requirements/adversarial 发现清理未来区间误计与 callback_gap/interrupted_seal 标签缺失；根因修复并补回归，两路定向复查 PASS；architecture 最终 PASS，无新增 owner/state/retry/schema。
- npm run verify 完整通过（Core 404 passed / 21 ignored；含 workspace Rust、前端、Python、原生电源策略、构建、隔离 Core/CLI/诊断/owner 烟测）。最终修复另跑覆盖回归、前端 2 项回归、typecheck/lint 通过。
- 真实组件合成页面已通过浏览器查看：历史事件同时显示“精转完成”与“录音有缺口”；非真实录音/截图。临时页面验证后移除。
- 既有恢复证据：capture_supervisor 每 250ms 根据当前意图/计划/资源/声源条件调度；source_process 监督首帧及耐久落盘进展；capture_retry_delay 为 2–30 秒有界退避；record_sleep_gap 保留睡前故障且重复通知幂等；启动 recover/recover_tasks 复用既有恢复。现有自动测试通过不能替代下面实机门槛。

### 尚未完成的 mandatory 实机验收
使用本次候选包、独立 LOPPI_DATA_DIR 和公开合成音频，不能连接日常录音库或把真实内容上传：
1. 真正睡眠后完整唤醒：核对新 session 的有效 frames/blocks 和持久音频；睡前/睡后任务自动完成，事件缺口与真实范围一致。
2. 短睡眠、重复睡眠、DarkWake、清醒灭屏分别执行；DarkWake 不循环启动不可用采集。
3. 设备暂不可用后恢复，核对有界重试与有效采集推进；手动暂停、睡眠跨计划结束、声源关闭、空间限制分别不误启动。
4. 睡眠/恢复期间退出或崩溃重启，核对无重复 session/精转、无悬挂任务与重叠计时。
5. 如需真实云端自动精转验收，须使用用户确认的测试凭据和合成输入；当前离线验证未调用付费服务。

以上未执行，不关闭 issue、不标 implemented、不提交或发布候选；依据 start-dev Step 5/5.5 的验收证据门禁。

- 最终修复后 cargo check --workspace --locked 通过；Apple Silicon 开发包重新构建成功，随包模型 readiness/retry 检查通过。候选为 build/dev/0.1.21/macos-arm64/loppi_0.1.21_macos-arm64-dev.dmg，dirty 开发构建，未替换已安装应用。完整日志 /tmp/issue49-final-build.log，验证日志 /tmp/issue49-verify.log；没有真实睡眠或真实云端验收，故保留 in-progress 和未提交工作树。

### 2026-11-04 独立实机补验
用户要求独立完成，不再等待人工配合。本轮使用最终候选包的真实 Core/CLI、自动销毁的独立目录、生成语音；`LOPPI_TEST_PARENT_PID` 禁止凭据与正式云端访问。未改已安装应用或日常数据库。

- 实际麦克风采集：1 个活动 session、2 个落盘块、480000 frames；状态 recording，不以启动返回值冒充采集成功。
- 手动暂停：最终 3 块、711168 frames；open session=0，后续观察 session/block/frame 数不增长。
- 声源禁用后 resume：没有新增 session 或音频。
- 重新启用后强制终止隔离 Core，使用 login 模式重启：状态 recording、4 块/951168 frames；随后暂停后 open session=0。保留 interrupted_seal/interrupted_tail 故障原因，未误标为睡眠；无活动 ASR 任务（本测试云队列禁用，不能据此证明付费任务恢复）。
- 候选包定时 IPC 实测：跨真实分钟边界执行；查询不消耗计划边界；重启后宿主刷新恢复处理；已消费边界不覆盖后续手动暂停。初次直接复用 debug 脚本因硬编码 model_not_ready 失败；该脚本明确假设没有随包模型。临时副本改为同时断言候选 models=ready、capture_blocker=setup_incomplete 后通过，未放宽产品检查或修改仓库原测试。
- 所有临时录音随隔离目录清理，测试 Core 已退出。脱敏结果 /tmp/issue49-runtime-report.json，定时结果 /tmp/issue49-schedule-candidate.log。
- 环境阻塞：sudo -n true 明确返回需要密码，无法安排有保证的自动唤醒；为避免电脑无法自动恢复，没有执行真实睡眠。也未执行设备拔插、DarkWake 或真实云端调用。本轮结果仅关闭对应子项，不能替代完整 mandatory 矩阵。保持 in-progress、未提交、未关闭 issue；不再向用户索取密码或要求立即配合。

### 用户交付决定（2026-11-04）
用户在知晓上述未验证项后明确要求「提交代码，关闭 issue」。据此覆盖本台账此前的提交/关闭等待门槛，提交已经实现、验证并完成三路审查的修复，并关闭 #49。此次决定不代表真睡眠、DarkWake、设备拔插或真实云端验收已经通过；这些证据限制继续保留，因此文档的完整验收状态仍为 in-progress。没有新增业务代码变更，不重复无关检查；提交前 diff --check 通过。本次不合并 dev/main、不发布安装版。

### 0.1.22 发布决定（2026-11-04）
用户进一步明确要求更新版本号并发布，授权将本次已审查修复纳入 0.1.22，经 dev Verify 后合入 main，由既有三平台 Release 工作流发行。此前不合并/不发布的限制由本次指令覆盖；真实睡眠/DarkWake、设备暂不可用与真实云端收尾仍未验证，发布不改变这些证据事实。本次改动未修改采集生命周期、OS 权限或签名机制。

### 已发布（2026-11-04）
0.1.22 已按用户要求正式发布，源码/tag 为 7cc63e9e98f3785802f96fa15ef2c6eba05c1185。dev/main Verify 通过，Release 41688013693 attempt 3 全链路成功，13 个附件与匿名三平台更新清单复核通过。见[发行记录](../research/2026-11-04-github-release-0.1.22.md)。台账中未执行的实机矩阵继续保持未验证，不因发布改写为通过。
