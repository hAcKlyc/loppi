---
status: in-progress
updated: 2026-11-07
---
# 核心慢操作隔离与详情流畅度

用户要求架构正确、简单可依赖，不增加防御性机制。执行对话中收敛的第一批四项：慢操作锁范围、首页主内容独立展示、播放与静态正文隔离、详情重复读取削减。其余审查清单不在本次范围。

## 执行台账

### 开发契约
- 必赢场景：导出写目标盘、截图/OCR、清理等待云请求时不占媒体读取/入库锁；时间线成功即展示；播放进度不重算静态正文；相同数据库快照的深分页刷新不重复传输所有页。
- 原子交付：以上四项同一批完成。沿用现有清理、导出、截图、播放、分页与修订语义。
- 必须保证：清理不能被迟到截图/导出绕过；事件切换不显示旧结果；文字纠错和人物变化不能因只比较事件 revision 而被忽略；键盘、时间定位和正文编辑保持。
- 明确不做：新服务、持久状态、重试/恢复/补偿体系、全局缓存、列表窗口化、更新器或搜索改造。
- 本次负责的问题：四项已确认路径及本次修改引入的问题，不扩大至全项目审查发现。
- 状态来源：Core 数据库仍唯一业务事实源；沿用 reads::Snapshot 的 SQLite data_version，仅在详情响应提供该快照身份用于省略不变页重读（包含 Core run 与连接身份，不当成事件修订）；GUI 只持有已加载详情与该读结果身份。
- 复用：cleanup_gate、clear_ranges、export_jobs、moments::capture/save_capture、Store::mutate_response、reads::Snapshot、timelineRequest/detailRequest、ContinuousPlayback。
- 架构变化：无新 owner/store/queue/retry。详情响应添加可选快照标识，旧客户端忽略，缺标识沿现有读取行为。截图计算暂存归调用栈所有，发布归原 moments owner；复用临时文件生命周期。
- 不变量：Core 唯一写入者；媒体发布与清理互斥；外部 IO 不属于实时媒体锁；事件编辑 revision 不冒充正文版本。

### 验收与结束条件
- Rust 合成并发验证：慢导出写不占媒体锁；清理后迟到截图拒绝；截图发布与现有清理/恢复一致；快照标识随写入/连接/Core 实例变化。
- React 验证：辅助查询延迟/失败不阻塞时间线；过期日期请求不继续分页；静态正文计算不随播放重复；深分页不变快照仅取首个检查页，有变化仍刷新完整已加载范围。
- 静态检查、适用项目 verify、开发包构建、合成数据浏览器交互及 fresh-context 三路审查通过后提交。
- 本次不修改原生捕获/播放实现、权限、安装和签名逻辑；真实设备长时耗电/慢盘表现为 advisory，合成并发与实际组件证据为本次功能验收依据。不触及用户录音库/凭据。
- 改动按影响重跑验证；单批 review 已覆盖整体后不重复完整审查。

### 开发批次与行动清单
- Batch 1：现有 owner 内缩短慢路径、减少重复读取/渲染。
  - [x] 核心锁范围与截图发布
  - [x] 首页读取与详情刷新
  - [x] 播放正文隔离
  - [x] 回归、构建与合成浏览器验收
  - [ ] cross-review、提交、整体闭环

### 当前批次 Review 基线
- Batch base: 0a5c972851ac0a8dbf8823a9e412c79f6c5e5500
- 分支：codex/smooth-core-and-detail
- 预期范围：core main/archive/moments/reads、协议详情响应、App/DetailDrawer/MomentList、相关测试和架构说明。

### 待用户决策
无。

### 进展日志
- 2026-11-07：契约建立，工作树起始干净。确认事件 revision 不涵盖正文全部变动，采用已有 SQLite 快照版本，避免新增内容哈希或派生状态。
- 2026-11-07：实现四项；相同数据库快照省略后续详情页，不新增内容哈希。持续写入时保持全量刷新，明确为收益边界。
- 2026-11-07：针对性 React 测试通过（辅助查询延迟/失败、90 条深分页、后页外部纠错、旧响应无标识、播放不重算）；Core 406 项通过、21 项显式忽略。新增截图发布/清理代次和快照身份测试通过，导出真实写路径验证另一线程可取得媒体锁。
- 2026-11-07：WebKit 长事件定位、加载边界、页面独立加载通过。旧脚本中的加载更多按钮已被当前无限滚动替换，改为驱动现有 observer；日期选择属性断言兼容 WebKit 的前缀读值，不改产品样式。
- 2026-11-07：默认打包输出正在运行旧 App，改用隔离源码与独立克隆的构建目录，不覆盖运行中应用。全量 verify 与隔离开发包构建执行中。

### 本批次 Review Contract
- mode: repair；base: 0a5c972851ac0a8dbf8823a9e412c79f6c5e5500。
- requirement source: 本文开发契约与对话前四项；用户强调架构正确、简单可靠，不新增防御性机制。
- in-scope: git diff 的 core archive/main/moments/reads、protocol lib、App/DetailDrawer/MomentList、ARCHITECTURE、本 devplan；新增 test_detail_refresh.mjs/test_timeline_independent.mjs；更新 test_gui_read_boundaries.html/test_date_navigation.html。
- out-of-scope: 其余原有性能清单、既存不相关缺陷、用户数据与运行中安装版。
- 允许变化: 复用 SQLite 快照身份的可选响应字段（不是内容哈希/持久版本）；调用栈私有截图临时目录；已有组件 memo；没有新增 owner/store/queue/retry/recovery。
- 保证/非目标/证据: 按上文；原生设备长时体验为 advisory，不改原生 capture/playback 实现。
- required lenses: requirements、adversarial；修复与受影响验证后全新 architecture。
- 缩锁后必须保持原清理与回执语义，不能通过取消一致性约束换性能；仅本次根因与引入问题可阻塞。Phase 1 只读，主 Agent 裁决。
- 2026-11-07：全量 verify 通过：Rust 445 项通过、21 项显式忽略；Node 135 项、兔子资源 3 项、Python 12 项通过；原生 policy/capture/playback/power、隔离 Core/CLI/登录交接/诊断/父进程退出检查通过。真实隔离 IPC 验证不变快照同标识、写入后变更。
- 2026-11-07：Phase 1 requirements：仅要求补齐过期 moments 满页停止分页证据，已补并独立复核 PASS。adversarial：发现同快照 early return 可取消在途人物列表；已把 snapshotVersion 的保存移到同请求完整加载成功后，原 token/ref 不变。延迟交错回归、人物选择可用及原深分页测试通过，原 reviewer 定向复核 PASS。没有新增防御机制或产品状态。
- 2026-11-07：隔离 arm64 开发包完整构建通过，离线随包模型/Unicode路径/组件准备验证通过。最后前端修复后的检查、浏览器详情回归和最终开发包正在重跑；Rust生产代码未再变化，不重复无关Core验收。

- 2026-11-07：requirements/adversarial 定向复核均 PASS，最终 fresh architecture PASS，无新增 owner、持久状态、独立恢复或重试机制。最后前端修复后 typecheck/lint/build 与全体 135 Node 测试通过；WebKit 详情定位/详情加载/读取边界复跑通过，现场记录 Markdown 展开/复制/后台刷新回归也通过。六组浏览器场景均通过。
- 2026-11-07：确认隔离构建目录为真实独立目录（非 symlink）；最终候选的全部改动生产文件与工作树逐字节一致。没有替换运行中 App。

- 2026-11-07：最终修复后的 arm64 开发包完整构建与随包离线模型验证通过，候选位于 build/dev/0.1.23/macos-arm64-smooth/。开发候选基于本批 base + 已验证工作树，dirty=true，未发布/未替换安装版。全部必做验证完成；Windows 真机、长时录音耗电与任意异常挂载时延未在本次测量，不将其冒充实测结论。
