---
status: implemented
updated: 2026-10-17
---
# 录音详情第一轮布局优化

需求源：2026-10-17 对话确认。保留浅色绿色风格与左右分栏，优化顶部布局、转录密度、播放器固定、播放位置反馈、笔记对齐。

## 执行台账

### 开发契约
- 必赢场景：打开未整理或命名事件，尽早看到文字；滚动长转录时播放控制和标签仍可用；播放/定位后对应来源、时间区间的句段有明确反馈；笔记区独立滚动、输入和按钮右侧对齐。
- 原子交付：一个前端批次，无存储或外部协议变化。
- 必须保证：真实钟点（含跨日日期）、粗精转来源、说话人/修正/精转/导出/清理入口保持；不强制自动滚动；窄桌面窗口仍保留左右分栏。
- 明确不做：精转流程重构、笔记锚点逻辑改变、自动跟随、波形、搜索、后端/原生入口变化。
- 本次负责的问题：头部和工具条过高、正文密度、播放器滚出、缺少当前位置反馈、笔记输入固定避让导致错位，以及改动引入的布局/定位回归。
- 状态来源与生命周期：复用 DetailDrawer 的 playhead、playSource、playing 与已有 ContinuousPlayback 回调；高亮按句段 [started_ms, ended_ms) 与播放来源派生，不创建新状态或计时器。
- 复用既有抽象：DetailDrawer、bestSegments、wallClock、ContinuousPlayback、现有 CSS tokens。
- 新增架构机制：无。
- 架构不变量：本机核心管理数据，React 只负责呈现/动作；不改变录音或笔记真实时间。仓库未找到适用 AGENTS.md/CLAUDE.md。

### 验收与结束条件
- 紧凑标题、播放器与标签/操作工具栏；转录可见空间明显增加。
- 长转录与长笔记独立滚动，播放器与笔记输入仍可操作；760px、1024px 和宽桌面不出现横向裁切。
- 播放/拖动/点击时间有来源正确的句段高亮，结束边界与无句段区间不误高亮；暂停保留当前位置。
- 粗转、精转编辑、跨日、空态和笔记提交基本功能保留。
- 证据：类型检查、lint、前端构建、现有 playback 测试；真实组件的隔离浏览器合成数据烟测及截图（不声称原生音频/真实数据烟测）。
- 改动影响：TSX/CSS 修改重跑静态与浏览器相关场景；播放引擎未改，仅运行既有回归。
- 结束条件：上述证据和三路 cross-review PASS，无强制真机验收缺口；原生窗口主观观感检查为 advisory（无 OS 代码变化）。

### 开发批次与行动清单
- Batch 1：详情布局与位置反馈
  - [x] 实现布局/反馈
  - [x] 自验证、浏览器烟测与需求核对
  - [x] requirements/adversarial + final architecture review
  - [x] 提交与最终台账

### 当前批次 Review 基线
- Batch base：407575e3130e7af21570ebefb8fcd56aa430669b
- 范围：src/DetailDrawer.tsx、src/App.css、specs/DESIGN.md、本 devplan。
- 排除：已有 untracked 的 design/playground/specs/research 文件。

### 待用户决策
无。

### 进展日志
- 2026-10-17：确认单批次范围，读取页面、样式、播放实现及产品时间语义；建立契约。

- 2026-10-17：实现紧凑头部、浅绿播放条、合并标签/精转工具栏、独立正文滚动、来源/时间区间派生高亮、笔记输入右对齐；保留所有已有入口。
- 2026-10-17：typecheck、lint、build 全通过；既有 playback 8/8 通过。真实组件 + 隔离合成 IPC/静音 WAV 的浏览器烟测：1280×720 首句 y=221、单行粗转行高约67px；正文翻页 scrollTop=483 时播放器 y=98 不动；笔记流 scrollTop=425 时正文不动，输入底部 y=700、按钮和输入右缘均 x=1260。提交笔记成功并清空，精转文字修正成功，标签切换滚动归零。
- 2026-10-17：760×600 精转、1024×650 跨日 iframe 视口截图检查通过，左右分栏/完整操作可见；自然播放推进并更新高亮，暂停保留位置；切换来源不匹配高亮数0、混合1；精确定位句段结束+4000ms时高亮0、整段End高亮0。无自动滚动。未连接真实本机数据或原生音频。
- advisory：安装版窗口的主观观感与外置悬浮兔子人工位置遮挡可再体验；本次未修改 OS/播放引擎/数据存储，无新增 mandatory 原生验收。

### Review Contract
- mode: repair；requirement source: 本 devplan 与本轮用户明确的五项要求。
- target: 工作树；base: 407575e3130e7af21570ebefb8fcd56aa430669b。
- in-scope: src/DetailDrawer.tsx、src/App.css、specs/DESIGN.md、本 devplan；其它 untracked 文件全部 out-of-scope。
- guarantees/non-goals/in-scope defects: 严格采用上述开发契约，不扩大到精转流程、笔记锚点、自动跟随、后端、原生入口或既有缺陷。
- allowed architecture: 无新增；UI 位置反馈仅派生自既有 playhead/playSource，CSS 调整滚动容器所有权。
- required lenses: requirements + adversarial 并行，裁决后 fresh architecture。
- acceptance evidence: 上述静态、构建、playback 回归、真实组件隔离浏览器烟测；TSX/CSS 改动重跑受影响检查，主观真机观感 advisory。

- 2026-10-17：requirements/adversarial/final architecture 初轮均 PASS，无阻塞发现。随后用户要求选中样式去掉左侧深色线条、播放器去掉线框轨道；在同一批次调整为浅绿背景选中和实心进度填充。同步 DESIGN 的新分栏/滚动/笔记对齐规则，并为全宽窄窗顶部保留 macOS 按钮空间；针对性复查新增差异。

- 2026-10-17：用户进一步明确：笔记输入采用完整外框，发送按钮在框内右下角，去掉“关联 xxxx 的录音”和上方分隔线。按指示移除时间提示，仅隐藏呈现，保留既有 anchor 保存逻辑；统一输入框焦点反馈。此偏好纳入本轮验收。

- 2026-10-17：最新样式验证通过：当前句段左边框0px，range appearance:none，Home/End 实心填充0%/100%，真实拖动得到49.886875%并同步17:30:39位置。760×540（比原600更低的原生最小高度）精转视口截图及翻页检查：顶部按钮净空、播放器固定、正文可读可滚动、完整笔记框和发送按钮均可见。新框内发送成功且清空/禁用恢复；选定时间锚点逻辑未改。
- 2026-10-17：最新 build/typecheck/lint 通过。最终预览（合成数据）：<USER_HOME>/.codex/visualizations/2026/10/17/01a14936-c656-7503-b05f-6efbc017a5ad/detail-layout-after.png。

- 2026-10-17：用户追加样式后的定向 requirements/adversarial/final architecture 均 PASS；窄窗旧截图被新760×540截图及翻页检查替代，唯一证据缺口关闭。无阻塞发现、无新增架构机制。
- 2026-10-17：整体范围复核完成：五项目标与两次样式追加要求均覆盖，原有数据/时间/操作语义保持，无临时产品分支。静态检查、构建、针对性浏览器验证与playback回归证据均有效；本轮前端交付无未关闭mandatory验收项。

- 2026-10-17：Batch 1 提交 83fb54a；最终HEAD typecheck/lint再次通过，与已构建/浏览器验证/review候选的产品代码完全一致。标记implemented。本轮仅前端源码交付，未重新安装桌面应用；预览为实际组件配合合成数据，不代表原生录音验证。
- 用户当前正在查看临时浏览器预览，保留其本机测试服务器与隔离样本供继续调整；未将测试桥接或样本写入产品源码。既有其它未跟踪文件保持原样。
