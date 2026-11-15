---
status: in-progress
updated: 2026-11-16
---
# 人物归属立即保存与后台声纹学习

用户确认本次对话方案并授权实现、独立测试验收及提交 PR。人工归属必须立即持久化；声纹学习和同场声学分组计算不得占着共享 Store 锁；撤销、重转、删除和重启不能让过期学习覆盖人工事实。

## 执行台账

### 开发契约
- 必赢场景：长场景和四万余历史句段下，创建人物并指定整组能快速返回；后台学习时其他编辑、查询不被声学重算阻塞。
- 原子交付：Core 调度、持久任务、GUI 回执、回归和文档统一交付；保持现有持久人工绑定和已发布数据语义。
- 必须保证：人工优先，任务持久且可合并，计算不占共享写锁，发布校验输入版本；重试操作幂等，学习失败保留姓名。
- 不做：改变声纹阈值、云 API、模型、自动继承重转后的不确定身份、发布安装包或替换日常应用。
- 负责问题：同步整场重建和声纹候选准备拖慢人工保存；后台声学 SQL/计算持锁；超时重复建人；学习状态反馈。
- 事实与生命周期：Core 是唯一写入者；people/speaker_bindings/local_speaker_assignments 是人工事实；voice_enrollments 是学习事实；speaker_groups 是可重建投影。GUI 只读/提交命令。
- 复用：voice_loop、voice_enrollments/parts、VoiceMatcher、identity_state.generation、mutation_receipts、readers.snapshot、人工绑定失效触发器。
- 允许机制：在现有 Core/SQLite/voice_loop 内增加持久待处理的分组与学习准备工作；不增加服务或通用调度框架。
- 不变量：短事务、Core 唯一写入、原音不外发、人工确认优先、清理和过期结果不复活。

### 验收与结束条件
- 合成大库验证确认延迟不随无关历史显著增长；后台计算期间可提交其他编辑。
- 验证迟到结果、撤销/改绑/删人/重转、重启恢复、重复操作 ID 与短句组合。
- GUI 验证确认后归属保存与学习状态分离，失败重试保持同一操作 ID。
- npm typecheck/lint、cargo check、受影响回归、适用 verify、Mac 开发构建、合成运行时烟测；未覆盖平台如实标记。
- 三路 fresh-context cross-review 全部完成且架构 PASS 后提交、推送并创建 PR。

### 开发批次与行动清单
- Batch 1（一个原子批次）
  - [x] 读取 owner 文档并隔离工作树
  - [x] 实现短保存路径与持久后台工作
  - [x] 实现锁外分组计算和版本化发布
  - [x] 稳定请求 ID 和 GUI 状态
  - [x] 大库/并发/恢复/失效与 GUI 验证
  - [x] 更新架构文档，构建烟测及 cross-review
  - [ ] 提交并创建 PR

### 当前批次 Review 基线
- Batch base: df1b132
- 工作树：/tmp/loppi-speaker-enrollment；分支 fix/speaker-enrollment-responsive
- 原工作树 .gitignore 与未跟踪 skills 不属于本任务。

### 待用户决策
无。

### 进展日志
- 2026-11-16：已核实既有学习队列、generation 校验和人工证据撤销机制；同步 rebuild_speaker_groups 会逐观察重放，同场安全合组存在复杂 SQL，日常进程采样约一核满载。仅将确认中的重建搬走不足以解决全局写锁阻塞。

- 初步验证：cargo check workspace 通过；knowledge 回归 47 通过、1 个既有 ignored；TS/lint 通过；WebKit 合成人物选择回归 47 项通过，截图在 /tmp/loppi-speaker-evidence/picker-regression.png。
- 全 workspace 回归发现原短标签测试确认后仍要求无分组；本次显式成员固定使人工确认后的分组已知，已更新对应断言，待最终 verify。
- 临时操作 ID 与既有云配置操作一样保留在当前 WebView 内存，不跨应用重启；持久学习请求与候选跨重启恢复。未知提交可在同意图重试时复用原数据包及回执，修订刷新不改变其身份。

### 最终验证与审查证据
- `npm run verify` 全入口通过：Core 441 passed / 21 个既有 ignored；前端 139 passed；兔子资源 3 passed；Python 12 passed；原生策略、全 workspace 二进制、隔离 Core、登录交接、诊断、测试 owner 烟测通过。
- 最后增加长场景测试并优化单句候选读取顺序后，`cargo test -p loppi-core --locked knowledge::` 48 passed / 1 个既有 ignored；`cargo check --workspace --locked` 通过。优化只将已有向量/实测质量的主键检查移到证据验证前，不改变证据与阈值。
- 近三小时 1000 个场内成员 + 45000 条无关历史片段：确认 223.42 ms；准备阶段在 3 秒后台读取预算内完成。性能数字是本机合成回归观察，非跨机器延迟保证。
- 真实 Core/CLI 隔离进程：创建与绑定 25.38 ms；重复 request ID、Core 退出重启后再次提交均返回原回执，人物数保持 1。无凭据、采集、私有素材或云调用。
- WebKit 合成人物选择 47 项交互检查通过；pending-learning 状态可见性检查通过；截图仅包含合成数据。
- Cross-review：独立 requirements、adversarial 无阻塞发现；最终 architecture PASS。唯一要求补齐的证据缺口（最终验证、真实文件重开）已关闭；开发包构建仍待结束。
- 尚未覆盖：Windows/Intel 构建和新安装包在这些平台的真机验收；本次无新增平台桥接、权限或采集实现。没有替换日常安装版。

- ARM64 `./build_dev.sh --target aarch64-apple-darwin` exit 0；Core/CLI/worker/GUI 优化构建、四组件身份、原生运行时、模型哈希及 App/DMG 包内模型自动就绪/重试烟测通过。开发包为 ad-hoc 签名、未公证；源码身份 df1b132 + 本批 dirty 工作树，四组件一致。产物 `build/dev/0.1.28/macos-arm64/loppi_0.1.28_macos-arm64-dev.dmg`，185995941 bytes，SHA-256 `ba3f962a646dbbcd0d13137cb97943e0804451af4ea6521a75dbd40c5a88acfc`。
- 三路审查后的唯一源码整理为恢复 main.rs 中无关的格式化，voice_loop 逻辑未改变；不再扩大范围。全部本次必做实现、自动验收及本机包体验证已闭环，待提交和 PR 创建。
