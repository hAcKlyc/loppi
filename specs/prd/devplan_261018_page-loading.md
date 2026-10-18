---
status: in-progress
updated: 2026-10-19
---
# 页面统一骨架加载

首次数据尚未返回时，首页记录分布与列表、队列状态/任务/待核对、统计、人物库/声纹、设置的远端状态、日历和说话人选择均使用同一骨架视觉。成功才显示空态/零计数/未配置/未授权；失败显示可重试或明确恢复入口。背景刷新保留已加载内容，新日期/统计范围不能展示旧范围为新数据。

## 执行台账
### 开发契约
- 单批次原子交付；不改 core、协议、存储和发布打包。
- 必须保证：加载/成功空/失败分离，刷新保持内容、选择和阅读状态；局部失败不遮蔽其它区域；减少动态效果关闭骨架动画。
- 本次负责：页面初始空态和假默认状态，以及相关请求的失败、过期结果覆盖。
- 复用：现有 App refreshTimeline/refreshContext/refreshTasks、组件局部请求 generation、detail skeleton CSS token。
- 新增机制：共享纯展示 LoadingSkeleton/LoadBoundary；加载事实仍归各请求 owner。为数组增加 ready，错误保留在所属区域；不新增 store、缓存、网络框架、自动重试协议。
- 架构约束：core 仍唯一数据事实源；UI 不将未加载转换为合法空值；所有样式复用 token。
- 不做：操作提交/录音运行中状态不替换为骨架，不扩张到后台处理算法。
### 验收与结束条件
- 受控浏览器夹具覆盖主要页面延迟/成功空/失败重试/刷新和局部完成；原有详情定位、加载测试回归。
- typecheck/lint/build、相关纯函数/现有 UI 测试通过；三路 fresh review；签名 App 重建重启并恢复原记录意图。
- 单批次完成后结束；无 Rust 变更不运行全套 core 测试。主观动画微调 advisory。
### 开发清单
- [x] 实现统一展示与逐区域真实加载状态
- [x] 夹具、静态和构建验证
- [x] requirements/adversarial/architecture 审查
- [ ] 重建重启、提交和收尾
### Review 基线
- Base e2c09e4
- 范围：src 内相关前端页面及新 LoadingState.tsx、scripts/test_page_loading.html、specs/DESIGN.md 和该台账；不包含 README/package 脚本和无关未跟踪文件。
### 待用户决策
无。
### 进展
- 2026-10-18：审计确认 App arrays 默认空，context 请求失败被吞；设置 null 被解释为未开启/未授权/未就绪。详情先前已修复。本批保持请求 owner，统一展示原语。

- 2026-10-19：typecheck/lint/build、29 项纯逻辑测试通过；真实 WebKit 整页加载夹具（含说话人选择）PASS、人物设置29项交互 PASS、日期导航 PASS、详情加载和定位 PASS。独立请求完成、首次/后台失败、手动重试和新范围骨架均覆盖。截图 /tmp/hear-page-loading.png。
- 2026-10-19：Phase 1 两路指出状态快照内 error 未接入设置读取边界（blocking）；adversarial 另指出日历背景失败覆盖成功日期（blocking）。均属契约内：使用 snapshot.error 与 IPC 错误合并传入原边界；日历既有月份结果保留成功 days 并记录 error。增加正常 resolve 错误快照、异步错误事件、日历成功后背景失败夹具。页面加载及日期导航再次 PASS；typecheck/lint 与 entry/capture 14 项通过。没有新增 owner/协议。

- 2026-10-19：两路 focused recheck PASS；最终 architecture PASS，无剩余阻塞。最终签名构建与严格签名校验通过；退出旧进程后替换 /Applications/hear.app 并重启，按原意图和 revision guard 恢复记录。revision 55：麦克风/电脑声音均 recording；主窗口首页、队列和设置内容核对正常。
