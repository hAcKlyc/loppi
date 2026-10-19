---
status: in-progress
updated: 2026-10-19
---
# 本机处理可靠性修复与 PR

用户要求基于 2026-10-19 全天诊断（issues #1—#8）直接修复并提交 PR。本次交付为经过测试和独立审查的代码与 PR，不安装到日常录音客户端、不主动重跑付费云任务；未复现的历史根因保留在原 issue 中，不以增加日志冒充根因修复。

## 执行台账

### 开发契约
- 必赢场景：完成的历史语音保留状态最终收敛，稳定事件可以规划精转；卡片报告的阻塞原因与调度一致；失败可追到阶段和对象，关键校验不会悄悄放行坏时间。
- 原子交付：一次分支/PR；保留已有 SQLite 数据、不可变 ASR 请求/时间映射、用户修订和清理墓碑。
- 必须保证：不删除未确认音频，不伪造/裁剪坏识别时间，不自动付费重送已提交请求，不让处理失败阻塞采集；日志不写秘密/正文。
- 本次负责：#1/#7 的状态与投影缺陷，#4 已复现的零样本 VAD 区间，#8 的 SCK 输出配置，#6 的分阶段诊断；对 #2/#3/#5 尽可能复现并修根因，无法复现则只提交必要诊断并保留 issue 未关闭。
- 状态来源与生命周期：Core 唯一写入者；audio_blocks/coarse_jobs/segments 为 VAD 保留依据，event_asr_state/refs 与 candidate_windows/cloud_tasks 为规划事实；UI 读取同一投影，不另存状态。
- 复用抽象：retention::resolve_analysis/resolve_block，events::asr::refresh，event_state，LogEvent 的既有 ID/error_code 字段，task_failed_with_retry，source_process::monitor。
- 允许机制：在既有 owner 中修正判定与恢复；白名单错误子码；共享稳定性范围/阻塞查询；有界历史保留状态检查；在既有 retention_decision 字段记录无法补证的保守留存，原始 speech_active 保持不变；复用 core_migrations 给受 VAD 缺陷影响、仍保留媒体的失败块一次本地重试（仍有原 3 次上限）。暂不建立第二份状态库、通用重试框架或替换时间坐标模型。
- 明确不保证：本次 PR 不代表部署或全天真实录音/云端质量验收通过；没有供应商原响应时不臆测映射具体失败分支。

### 验收与结束条件
- 核心 Rust 单测/回归、cargo check/build；前端 typecheck/lint/build 与相关状态测试。
- 用冻结本机数据库的隔离副本验证状态恢复和投影，不写活跃数据库；针对音频问题在隔离进程复现，原音频不入库。
- 原生 SCK 编译与可用的隔离 OS 检查；无法自动完成的发布检查保留 mandatory，不宣称可发布或安装验收完成。
- 需求/对抗/最终架构三个 fresh-context review 完成；修复本次范围内阻塞项，最终架构 PASS 后提交和创建 PR。
- 结束条件是可审阅 PR 与准确的 issue 处置表；未复现的原有问题清楚列明，不为了关闭 issue 改变原有安全/数据契约。

### 开发批次
- Batch 1：处理状态、原生输出、诊断与可复现根因；一个组合验证和 review 范围。
  - [x] 精确复现与核心实现
  - [x] 数据/时间/日志回归和隔离验收
  - [x] 文档与 issue 处置表
  - [x] 三视角审查
  - [ ] 提交、推送与 PR

### Review 基线
- Batch base: a926d39c8cb06be9d1bb281a2af6185f922f8cb0
- 分支：fix/local-processing-reliability
- in-scope：实际变更的 crates/hear-core、crates/hear-worker、必要前端状态文案/测试和原生探测测试、相关架构/算法文档、本台账。
- out-of-scope：用户日常客户端/活跃数据、其他会话与项目改动、工作台私有诊断原料。

### 待用户决策
无。现有授权覆盖本次修复和 PR；部署不在本轮交付承诺中。

### 进展
- 10/19：完成只读诊断并创建 #1—#8；已在新分支开始实现。

- 10/19：用户强调“架构正确、不过度设计或过度防御”；实现沿用 Core / VAD worker / 既有维护与迁移 owner，不增加通用重试或平行状态。
- 10/19：生产解码/重采样+旧 worker 精确复现两个失败块均返回零时长片段；新 worker 两例均成功发布。冻结库 42 个 complete/pending 中 41 个安全收敛，19 个阻塞事件降至 8，3,820 个保留媒体及原语音标志不变。
- 10/19：Rust 265 项通过、5 项 opt-in 跳过（其中本次 2 项已单独实跑）；core/worker check/build、前端 typecheck/lint/build 与状态测试通过。SCK 同机原生 3 秒 A/B：两者 144,000 帧，旧版 4 条 dropping-frame 错误，新版 0。详见 `specs/tech_docs/local-processing-verification.md`。

### 发布验收（与本轮 PR 交付区分）
- mandatory：将同一构建的 core/worker 一并安装后，核对两个历史失败块自动重试与已有云任务身份保持，持续采集跨睡眠/唤醒、授权撤回/恢复及系统来源切换。这些会影响正在录音的客户端，本轮不执行；台账保持 in-progress，不能据 PR 宣称安装验收或全天稳定性已通过。
- #2/#3/#5 的历史供应商原响应/采集中断分支已丢失，新增诊断仍需新运行样本；问题保持开放。

- Phase 1 裁决：requirements 的恢复证据缺口与 adversarial 的 P1 为同一根因，采纳为阻塞项。历史迁移暴露 coarse::enqueue 的 session 全局水位假设，可能吞旧区间并回收句首。已在原 coarse owner 使用既有区间覆盖去重，不加状态/队列/模式；原“更早起点也被整个前缀吞掉”测试改为实际已覆盖样本不重复，允许补未覆盖前缀。新增两项回归；真实回放每例补回 1 个任务并核验媒体不丢。原 SCK、前端和映射验证不受影响。
- 三视角结果：requirements 与 adversarial 定向复核 PASS，原恢复证据缺口与 P1 均关闭；唯一非阻塞算法文档残留已修正。全新上下文 architecture 审查完整最终 23 文件候选后 PASS：复用原 owner/事务、维护与迁移，无新增进程、表、协议、队列或通用重试框架，复杂度与本次保证相称。发布前安装与 OS 长时验收仍待执行。
