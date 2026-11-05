---
status: in-progress
updated: 2026-11-06
---
# 唤醒通知遗漏后的采集恢复

修复 macOS 已收到 WillSleep、却遗漏 HasPoweredOn 后永久暂停采集的容错缺口。完整唤醒恢复原录制意图；DarkWake、早期唤醒、单纯灭屏不能触发兜底。没有用户现场日志，不认定此缺口就是反馈根因。

## 执行台账
### 开发契约
- 必赢场景：遗漏 HasPoweredOn，独立 OS 事实证明本轮真实睡眠后完整唤醒，则既有 power generation 推进一次，交由 capture_supervisor 自动重启。
- 原子交付：原生适配、故障模拟、文档、三路审查一并完成；不发布安装版。
- 必须保证：DarkWake 不恢复；旧的醒着状态不能撤销刚收到的 WillSleep；查询失败保守保留睡眠；并发通知不可被旧查询覆盖；不改变手动/计划/容量/配置的采集门禁。
- 不做：改 Windows、GUI、协议、数据库、重试 owner；不以超时、屏幕亮灭或进程运行推断完整唤醒。
- 状态来源：IOKit 通知仍是主路径；macOS 原生 power snapshot 内新增只读补偿，以 kern.wake_abs_time 的本轮内核唤醒时间及 powerd 发布的 State:/IOKit/SystemPowerCapabilities（CPU/Video/Audio）为联合证据。后者是 Apple 开源实现的状态键而非公共稳定 schema；缺失/类型错误/读取失败不解除暂停。
- 复用：loppi_power_snapshot、power_changed、capture_supervisor、reset_capture_retry；不增加线程/周期任务/持久状态。允许新增本轮睡眠的单调时间边界及互斥快照保护，避免旧结果覆盖新睡眠。
- 架构：Core 唯一业务 owner，平台电源事实封装在 native/resources.m，Rust 接口和门禁不变。
### 验收与结束条件
- 模拟必须执行生产原生通知与快照逻辑：正常/遗漏通知/短睡眠/DarkWake/旧能力/失败/重复/竞争；既有 Rust 睡眠、重试、来源监督、计划测试。
- 静态、原生编译、workspace 构建、适用 verify、清醒实机只读快照；三路独立审查通过。
- mandatory 发布门槛：同一构建真实合盖完整唤醒、DarkWake 与声音落盘。用户当前要求代码修复、模拟和审查，本轮不主动休眠电脑或操作日常录音；未实测保持 in-progress，不声称可发布。
- 生产代码变更使相关原生/构建/测试失效，纯台账不重复构建。
### 批次与 Review 基线
- 单批；base: 6bd8f19679e72de6ae320f1f1e516401575b7480。
- [x] 原生修复与模拟
- [x] 验证与独立 requirements/adversarial/architecture 审查
- [ ] 真机发布门槛
### 待用户决策
无。
### 进展
- 核实 Apple PowerManagement PMConnection.m：能力值在真实 CPU 睡眠的 SYSTEM_WILL_SLEEP 分支写入动态能力 0；SYSTEM_DID_WAKE 根据完整/DarkWake 发布能力。S0DARK 分支仅移除通知能力，不直接更新动态状态键；不能用该分支宣称即时状态键变更保证；XNU IOPMrootDomain.cpp 在内核唤醒更新 gIOLastWakeAbsTime。联合证据防止入睡通知后读到上一轮 full 能力就提前恢复。
- 实现与原生模拟通过；本机只读 smoke：电源通知注册可用、当前 powerd 能力=31、历史唤醒时间不能越过当前睡眠边界。完整 npm run verify 进行中。

### Review Contract
- mode: repair；base: 6bd8f19679e72de6ae320f1f1e516401575b7480；单批工作树 diff。
- scope: crates/loppi-core/native/resources.m、crates/loppi-core/build.rs、scripts/test_native_power.m、scripts/test_native_resources.py、scripts/verify.py、specs/ARCHITECTURE.md、specs/tech_docs/diagnostics-and-verification.md、本 devplan。无其它 session 改动。
- guarantees/non-goals/allowed architecture: 本文开发契约；修复本轮内核实际睡眠后遗失 HasPoweredOn 的情况；不承诺 OS 状态键不可用时也恢复，不处理全部电源通知同时丢失。
- required: requirements、adversarial、最终 architecture，fresh context，read-only；不得自行修改或派遣。
- acceptance: 生产原生模拟、本机只读边界 smoke、workspace 验证；真实合盖/DarkWake 音频落盘仍为独立发布门槛，未执行，不将模拟写成真机通过。

- 完整 `npm run verify` exit 0（/tmp/loppi-wake-verify.log）：workspace check/tests、Core 404 passed / 21 ignored、前端/Python/原生测试、workspace build、构建身份及隔离 Core/登录接管/诊断/owner smoke 全通过。既有编译警告及前端测试的开发端口提示未导致失败。
- requirements PASS（发布实机门槛 UNVERIFIED）、adversarial PASS，无阻塞发现；唯一非阻塞文档精度修正已完成，不影响代码验证。最终 architecture PASS：补偿属于既有原生电源事实边界，无新增业务 owner、协议、线程或重试器；没有必要修改项。

### 当前交付状态与发布验收
代码修复、生产原生模拟、完整离线验证和三路审查完成。真实 OS 发布门槛未执行，status 保持 in-progress；依 start-dev Step 5 的“验收要求的证据缺口已关闭”提交条件，本轮保留工作树，不提交、不发布。用户没有被要求提供密码或配合立即休眠。

待在同一构建、独立数据目录和合成音频下完成：
1. 双路采集后合盖再打开（含短睡眠）：无需手动继续；核对新 session、有效 frames/blocks 和实际落盘音频，不能只看 UI。
2. OS 日志确认 DarkWake：期间不新增采集 session；之后完整唤醒应恢复。清醒单纯灭屏保持原采集。
3. 分别在手动暂停、跨定时停止点、容量暂停下睡眠/唤醒：保持相应门禁，不误恢复。

本次只读 OS smoke 未记录或上传私人录音；临时 smoke 源码/二进制清理，验证日志保留在 /tmp/loppi-wake-verify.log。

### 2026-11-06 用户验收与发布决定
已将当前唤醒修复及 UI 改动构建成 0.1.22 dirty Apple Silicon 开发包，随包模型、构建身份与 DMG 检查通过。已退出旧安装版并启动该开发包，GUI/Core 进程路径均为本次产物；手动暂停保留。用户随后明确反馈“我测试了，没有问题了”，并要求“版本号更新并走发布流程直接上线”。据此授权提交、合并及 0.1.23 正式发行，覆盖此前等待提交/发布的限制。没有将该概括验收扩写成逐项 DarkWake、睡眠音频落盘或跨平台升级证据；历史证据边界仍保留，完整实机矩阵状态保持未验证。

本次同时纳入用户验收的三项 UI 修正：关闭开关去描边；所有统计金额两位小数；服务卡片开关左/齿轮设置右。前端 typecheck/lint/build、11 项统计与真实组件导航测试、8 项设计资源测试通过；原生修复三路审查与完整离线 Verify 已通过。发行准备使用 0.1.23，同步所有版本源，不改业务协议或升级依赖。

- 0.1.23 最终组合态完整 `npm run verify` exit 0（/tmp/loppi-0.1.23-verify.log）；版本元数据校验与 diff --check 通过，准备提交并推送 dev。

### 0.1.23 正式发行完成
用户验收与直接发布授权已执行：dev `5579ea8634614a4ff891c317407b48eda322f4f8`、main `5a04a9de12c1e343373af073afaf5a035ba2f7a4` 的 Verify 均成功；Release 42072696406 attempt 2 成功，三平台完整附件已公开为 Latest，固定更新地址已匿名复核。首次发布草稿回读失败，仅重跑 publish 后成功。来源、签名与摘要见[发行记录](../research/2026-11-06-github-release-0.1.23.md)。用户请求的开发与发行已完成；本台账仍保留细分实机矩阵未验证状态，不将发布成功替代 OS 场景证据。
