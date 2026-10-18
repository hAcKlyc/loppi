---
status: in-progress
updated: 2026-10-18
---
# 首页记录分布悬停时间光标

用户追加要求：鼠标在首页记录分布轨道移动时，显示顺滑跟随的竖线和当前位置 HH:mm，方便判断并点击对应记录。现有概览色块点击保持打开原记录；不新增自动播放、修改事件边界或缺口造记录。

## 执行台账
### 开发契约
- 必赢场景：鼠标横向移动跨双路声音/现场记录时，一条对齐轨道的竖线同步显示当前时间；左右端点不溢出；离开即消失；点击原有记录正常打开。
- 原子交付：单批 UI/映射回归/视觉验证/审查/构建重启；前一批自动整理修复保持。
- 必须保证：时间映射使用已有 dayBounds 的本地日实际起止；左端 00:00、右端 24:00，不冒充次日；宽度变化及切日不留旧光标；装饰层不挡原点击/焦点；尊重减少动态效果；每帧最多一次光标更新，不驱动整页高频重渲染。
- 明确不做：新建时间选择状态、seek/自动播放、新的后端 API、空白轨道点击创建记录、触屏拖动选择。
- 状态与生命周期：鼠标位置是组件内短暂装饰状态，日范围来自 TimelineView；unmount/date-change 清理事件/rAF，指针退出或窗口失焦清除。
- 复用：TimelineView day-map-plot、day-map-now-track 的轨道对齐，timeline-view.ts 的日时间工具，现有 onSelect(item)、tokens.css。
- 新增机制：独立 DayMapCursor 展示组件与一个纯坐标映射函数，无持久状态/协议/第三方依赖。
- 不变量：时间轴仍是真实墙钟；前端只读呈现，点击由既有详情流处理。

### 验收与结束条件
- 测试：左右边界、半日/17:54、无效尺寸/输入、不同本地日长；DOM 事件实测光标跟随、离开/切日/resize、点击穿透。
- 静态：typecheck/lint、相关 Node tests、frontend build；不重复无关 Rust tests。
- mandatory：浏览器真实渲染与窄窗检查、最终签名 App 重启并保持双路录音；前一批积压发布证据保留。
- required: requirements/adversarial/architecture fresh reviews，修复相关阻塞后提交并标记完成。

### 开发批次与行动清单
- [x] 光标与映射、样式
- [x] 单测/浏览器实测
- [ ] 三路审查、提交
- [x] 签名构建重启

### 当前批次 Review 基线
- Batch base：56e2ed6（自动整理修复已验证并提交）。
- 预期范围：src/DayMapCursor.tsx、src/timeline-view.ts、src/TimelineView.tsx、src/App.css、scripts/test_timeline_view.mjs、浏览器测试夹具、specs/DESIGN.md、本台账。
- 排除：此前未提交的 README.md/build_release.sh/package_macos.sh 和无关文件。

### 待用户决策
无。

### 进展日志
- 2026-10-18：用户在自动整理恢复过程中追加；先完成前批运行恢复，再实现本批并最终重新构建。
- 2026-10-18：实现 DayMapCursor + dayMapPoint；rAF 合并指针移动，仅更新装饰层，不设置页面 React state。双路/现场记录对齐既有轨道宽度，标签夹紧两端，原点击透传。
- 2026-10-18：typecheck/lint/frontend build 通过；合并前批状态回归共 13 项 Node 测试通过。真实浏览器运行 scripts/test_day_map_cursor.html，时间 17:54、左右端点、跨轨道线、点击透传、离开/切日/缩放/失焦/触摸全部 PASS；截图视觉确认标签、竖线与轨道对齐。只有原有 bundle chunk size 提示。

### Review Contract
- Mode repair; base 56e2ed6。
- In scope: src/DayMapCursor.tsx (new), src/timeline-view.ts, src/TimelineView.tsx, src/App.css, scripts/test_timeline_view.mjs, scripts/test_day_map_cursor.html (new), specs/DESIGN.md, 本 devplan。
- Out of scope: 已提交的整理修复；README.md/build_release.sh/scripts/package_macos.sh 的此前 DMG 变更及所有其它未跟踪文件。
- Guarantees/non-goals: 同本台账契约；hover 显示时间与保持现有点击，不新增从空白处 seek 或自动播放。
- In-scope defects: 本批引入/显著放大或用户明确要求的光标问题。
- Allowed mechanisms: 单个装饰组件的短暂 pointer/rAF/ResizeObserver 生命周期；一个纯坐标映射函数。无新后端、持久状态、依赖。
- Required lenses: requirements/adversarial/architecture all required。上述静态、单测、真实浏览器回归与最终签名 App 更新/双路录音为验收依据；最终构建重启在审查后、提交前完成。

- 2026-10-18：requirements/adversarial/architecture 三路 fresh-context 审查全部通过，无阻塞和返修。adversarial 额外独立验证纽约真实 23/25 小时日的本地标签；architecture 确认装饰组件持有短暂状态、复用日边界和点击 owner，没有额外持久机制。

- 2026-10-18：最终签名 App/DMG 构建通过，Apple 公证 Accepted（e991caeb-4a1b-4b34-93c2-ea97040ec049），staple/Gatekeeper 通过。旧进程退出后替换 /Applications/hear.app，revision 50→51 恢复原意图；CLI 与首页均确认双路正在记录。实际桌面轨道移动显示 17:54 竖线/标签，所有 mandatory 已关闭。用户随后追加点击位置传入详情的独立需求，将在下一批 devplan 实现，不修改本批契约。
