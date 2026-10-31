---
status: implemented
updated: 2026-10-17
---
# 扩展主窗口拖拽区域

顶部横条（含页边留白）与左侧栏非交互空白可拖动；日期、文字、日历、录音开关、导航不触发拖拽。扩展到队列/人物/设置顶部，保留布局、滚动及现有业务。只调整 DOM 拖拽标记与命中区域，无状态/协议/权限变更。

## 执行台账
- 复用安装的 Tauri 2.12.0 data-tauri-drag-region self 模式：仅直接命中标记元素；子级文字与控件不继承。
- 头部背景扩展到既有外边距，侧栏和导航间隙使用 self 标记；不使用覆盖控件的透明全局层。
- 验收：lint/typecheck/build；真机拖动头部和侧栏、导航/日历点击、文字区域不拖动；录音保持。已有可逆 UI 改动无需实现镜像单元测试。
- 审查：requirements + architecture；无新状态/IO/网络路径，不需要 adversarial。
- Batch base: f598471；范围 App.tsx/App.css/TimelineView.tsx/WorkspaceViews.tsx/DESIGN.md。
- [x] 实现与构建
- [x] 交叉审查
- [x] 签名替换、真机验收、提交
- 待用户决策：无。

## 验证记录
- requirements / architecture PASS；修复 responsive gutter / wide edge geometry 与日历 stacking context 两类审查问题。复用实际布局变量与 layer-calendar，无新机制。
- lint、typecheck、生产构建、Developer ID 签名及 strict 验签通过。最终桌面构建已运行，双路录音继续。
- 原生窗口命中：头部空白双击触发 Tauri 原生窗口最大化，CGWindowBounds 1120x760 → 1920x1050；侧栏下方空白双击恢复 1120x760。文字仍可选，日历正常打开，导航正常进入队列。该路径与 start_dragging 共用 isDragRegion。
- advisory：CUA 连续 drag 手势未可靠移动窗口（原生旧标题区亦如此），未声称实际按住移动已通过；已用原生双击及窗口尺寸变化验证两个区域的系统命中，连续拖动手感留给用户实操。
