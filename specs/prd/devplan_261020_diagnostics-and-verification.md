---
status: in-progress
updated: 2026-10-20
---
# 运行诊断与开发验证

把本机失败变成可定位、可复现的问题：运行组件准确报告构建身份；日志记录安全的失败上下文；CLI 导出时间范围内的脱敏诊断；本地和 CI 使用同一验证入口。延续“架构正确、不过度设计或过度防御”。

## 方案与归属

1. 构建身份在 hear-protocol 提供统一结构，构建时生成 git SHA、dirty、构建时刻、版本和协议版本。Core、CLI、worker、GUI 报告自身编译身份；打包脚本给整包统一构建时刻并校验组件，不能用磁盘新文件冒充运行中旧进程。
2. Core 是处理状态与诊断查询 owner。新增只读 diagnose IPC，通过现有 reads::Pool 的短快照查询原始任务、封块和缺口；日志独立读取并标记自身覆盖范围。报告不是第二个状态库。CLI 不启动 Core、不触发录音、重试或付费请求；输出只包含明确允许的字段，不带音频、文本、路径、凭据或供应商正文。
3. LogEvent 增加定型数值上下文。VAD 与粗转事件按真实阶段命名；正常高频 VAD 完成按分钟汇总计数，失败保留逐条记录。日志时间、job/event/session 过滤复用 LogFilter。JSON 解析位置、任务范围、尝试次数由最接近故障的代码产生，不从自然语言猜。
4. 健康投影统计指定时间窗内封块覆盖和明确缺口、任务状态、队列年龄、完成耗时及重试后成功。区分已保留音频覆盖、静音/已清理/未知区间和物理漏录，不能把进程存活或文件存在当召回正确。
5. scripts/verify.py 是本地与 GitHub Actions 共用入口：静态检查、Rust/前端/工具测试、构建与隔离 Core 烟测。模型/签名/OS 测试采用显式入口，默认不读真实用户库、不访问付费云端。历史故障以合成最小样本固定回归，复用已有 ASR/VAD/候选测试。
6. 文档给出分支、构建、安装、真机验证的独立状态与证据，保持正在运行 bundle 不被覆盖的既有约束。LLM evidence ID 替代时间边界会丢失片段内部切分精度，本轮保留现有毫秒契约并记录推导；不未经实验切换生产协议。

## 执行台账

### 开发契约
- 必赢：一次 `hear diagnose --from-ms ... --to-ms ...` 导出可归因、无正文的报告；能区分 VAD、粗转、LLM、ASR；一个验证入口在隔离目录回归生产故障。
- 原子交付：一个 PR 包含实现、测试、CI 和文档。新增 IPC 与共同协议版本同步；旧日志缺字段仍可读。
- 必须保证：Core 唯一状态 owner；诊断有界只读；输入 ID 过滤准确；错误上下文无自由正文；构建身份来自编译产物；不宣称当前安装版已更新。
- 不做：新增持久状态、通用重试框架、云日志服务、隐式模型下载、自动上传报告、改变采集/LLM 策略；不安装或重启用户客户端。
- 复用：reads::Pool、UniLogs/LogWorker、LogEvent/LogFilter、cloud::Failure、cloud_tasks、audio_blocks、media_gaps、coarse_jobs、现有 scripts/smoke_core.py 和生产回归测试。
- 新机制：只读诊断请求/响应、编译元数据；不新增业务 owner 或数据库。
- 不变量：采集回调不做日志 IO；报告不得持有 Store 写锁；队列重试及发布仍由原任务实现负责。

### 验收与结束条件
- 报告：时间/ID 筛选、空库、旧日志、截断提示、字段脱敏、只读性均有测试；CLI 实际导出权限 0600 且不覆盖已有文件。
- 构建：Core/worker/CLI build-info 可执行，GUI 编译并包含同一身份；统一打包路径校验身份。
- 验证：统一入口本机通过，新增 CI 同入口；历史核心回归默认无需云服务；额外真实模型和签名场景显式注明条件。
- Review：新上下文 requirements/adversarial 后修复，再 final architecture；最终 PASS 和对应复测后提交。
- 发布硬门槛：安装签名包后的关屏/睡眠/设备切换与真实长时覆盖，沿用现有发布门禁，不由此 PR 的单元测试代替。该范围不修改原生采集策略。

### 开发批次与行动清单
- 单批次（共享诊断契约）
  - [x] 构建身份与打包校验
  - [x] 失败上下文、日志语义与聚合
  - [x] 只读诊断与健康统计、CLI 导出
  - [x] 统一验证、CI、回归说明与工作流
  - [x] 自验证与三视角审查
  - [x] 提交 PR 与远端状态回读

### 当前批次 Review 基线
- Base：07b966bf14dfc4d96504bcaf2ed04b3ab3d21442
- 分支：feat/diagnostics-and-verification
- 范围：协议/Core/CLI/worker/GUI 构建身份、日志与诊断、scripts、CI、相关架构文档。
- 工作树无其他会话改动。

### 待用户决策
无。实现内取舍由本轮负责；不请求重复确认。

### 进展日志
- 2026-10-20：核实前一轮 PR #10 已合入 dev，从其 merge commit 开始本轮。

- 2026-10-20：统一入口已完整通过：Rust 274 Core + 9 desktop + 8 protocol、Node 43、素材 3、Python/native checks、四组件身份和两组隔离 smoke。补入缺失的 jsdom/sharp 测试依赖；无模型或云调用。
- 2026-10-20：新诊断投影在真实冻结库只读回放，0.17 秒完成；复现 10 个 ASR / 3 个 LLM / 2 个 VAD 最终失败，导出任务/覆盖/用量未运行服务或修改原库。原样本与 JSON 留在私有工作区，不提交。
- 2026-10-20：计数语义逐一回源：cloud attempts 为失败数，VAD 为领取数，coarse 为失败数；字段和文档已区分。Cloud 完成且当前失败计数非零才计入 completed_after_failure，正常 ASR 轮询不计失败。
- 2026-10-20：需求审查 PASS；对抗审查发现清理期间 `Ok(false)` 被误记为粗转完成，已区分 complete / not_published / deferred / failed，并在原真实发布与两种清理回归中验证。定向复审 PASS；workspace check/build、274 Core tests、四组件身份及两组隔离 smoke 全通过。待最终架构审查与 PR。
- 2026-10-20：最终完整候选架构审查 PASS：Core/read pool、usage owner、UniLogs 生命周期、组件边界与验证复用均符合契约；无新增业务状态/队列/恢复路径。代码审查闭环完成。签名安装与真实 OS/长时验收仍未执行，status 保持 in-progress；本轮交付 PR，未发布或替换安装版。
- 2026-10-20：实现提交 `33d82a3` 已推送，创建 [PR #11](https://github.com/hAcKlyc/hear/pull/11)，base `dev`、head `feat/diagnostics-and-verification` 已回读核对。单批次三视角审查及受影响复测完成；完整 14 项本机门禁通过（291 Rust、43 Node、3 素材、6 Python，以及原生策略/身份/smoke）。GitHub Actions 使用同一门禁，最终运行结果以 PR 最新 head 的 checks 为准。

### 当前交付状态与发布验收

实现、验证、review 与 PR 已齐备；这次未安装或发布，未修改活跃用户库。status 保持 `in-progress`，以下 mandatory 验收仍需在发布前完成：
- 在授权安装签名包后核对运行 Core、CLI、GUI 和 worker 的编译身份；不要用磁盘新文件身份代替运行中进程。
- 录制真实会话并执行关屏/唤醒、睡眠/唤醒及设备切换；核对实际音频、封块与缺口，不能只看 UI 状态或进程存活。
- 长时录制后检查连续覆盖与明确缺口，并确认失败诊断不含正文。采集中断 #5 仍开放，不由本 PR 的诊断能力或 CI 通过推定修复。
