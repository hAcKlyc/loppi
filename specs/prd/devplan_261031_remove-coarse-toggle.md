---
status: in-progress
created: 2026-10-31
updated: 2026-10-31
---

# 移除时间线粗转切换按钮

用户已决定移除“显示粗转/隐藏粗转”按钮，列表内容展示策略保持。本批以dev 70490272ee9621d69a3994e71c907ce45d9aebe3为基线，移除交互及其可变状态，固定沿用生产链路原默认hideCoarse=true；仍只在dev交付。

## 执行台账

### 契约
- 必赢场景：当天记录旁仅保留新建事件操作；普通未整理历史素材继续收起，命名事件、手动范围、实时、异常、本地失败与现场记录保留；摘要/摘录、概览及录制主状态按原规则。
- 复用：TimelineView.visible/cardExcerpt/liveCard与既有只读hideCoarse投影参数；不重写筛选条件或Core。App不再持有可切换的筛选状态，直接传入原默认true；删除onHideCoarse回调链和按钮专属样式，清理调用夹具。
- 新机制：无新state/owner/store/协议/恢复或兼容层。保留组件既有只读投影参数用于预览和回归，不新增产品设置入口。
- 不做：展开所有素材、改数据/录音/计费、删详情中的“查看本地粗转”、合main、更新版本或正式安装包。原资料、归档和正式App不动。

### 验收
- typecheck/lint/build、相关及全部前端回归；以固定true的卡片矩阵核实显示集合/摘录/概览保持，且无切换按钮。
- mandatory：1120/760真实合成浏览器确认按钮移除、新建事件仍可操作、普通素材收起/实时与异常保留、暂停后原筛选继续生效。无OS集成变化。
- fresh requirements/adversarial并行审查后裁决，最终fresh architecture PASS才提交；Conventional subject+非空body；共同静态复核后推送dev。GitHub Actions目前有已确认的账户付款/支出上限阻断，不改账单/工作流、不把未执行CI当通过。

### 清单
- [x] 删除按钮与可变交互链，更新夹具/说明。
- [x] 原显示策略回归、静态/构建与浏览器验收。
- [ ] 三路审查、提交推送dev。

### Review基线
70490272ee9621d69a3994e71c907ce45d9aebe3；scope为本批显式diff，其他本机资料与旧devplan结论为历史记录。不扩大到新的列表产品策略。

### 待决策
无。用户明确只移除按钮，保留原策略。

### 进展日志
- 2026-10-31：删除按钮、App可变筛选状态、回调与专属样式；visible/cardExcerpt/liveCard条件不变。设计文档与调用夹具同步。
- typecheck、lint、前端build、全部66项前端测试通过。新增固定默认true矩阵覆盖实时、普通素材、命名事件、手动范围、整理失败、本地失败、现场笔记与摘录。
- Computer Use：1120/760宽度六种状态共12组均无切换按钮，保留新建事件；录制中2卡、暂停/历史/状态不可用1卡，概览始终3项，无横向溢出。局部整理浏览器回归PASS；完整App合成场景新建表单打开/取消正常。
- 证据：build/dev-remove-coarse-toggle/下static.log、build.log、tests.log、browser-matrix.json、partial-fit.txt、app-after-cancel.txt及截图；仅合成数据，无OS集成变更。
- fixed candidate tree 86d72b2092a824fb70c204da7f4c2f075a830e4c：fresh requirements/adversarial均PASS，无阻塞项或必需证据缺口；主Agent核实接受、无需修复。adversarial独立重跑时间线12/12通过；随后fresh architecture完整diff审查PASS，无新增机制、复杂度降低。评审期间代码未变。
