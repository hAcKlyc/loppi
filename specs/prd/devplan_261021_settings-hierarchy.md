---
status: implemented
created: 2026-10-21
updated: 2026-10-21
---
# 设置项标题层级修正

用户明确要求设置项采用「主标题 + 状态文案」同一行、说明下一行，避免标题/说明/状态三行纵向平铺。本次统一声音来源、定时录制、本地语音、云服务、登录启动与容量行；沿用即时开关与独立编辑行为。

## 执行台账

### 开发契约
- 必赢场景：常态设置行标题是视觉重心，状态为旁边弱化标签，副标题紧随下一行；右侧仍为原开关/操作。
- 来源状态简短但不夸大授权/实际可用性；必要解释放副标题或既有异常帮助。长状态自然换行，不截断、遮挡或横向溢出。
- 原子范围仅 JSX/CSS/文案与对应规范，不改事件处理、Core/OS、状态 owner、提交/权限语义或首次引导 compact 分支。
- 复用 setting-row、设计 tokens、既有状态数据；无新组件状态、协议或依赖。
- 非目标：改录制/云处理逻辑、安装替换用户客户端、重新设计其它页面。

### 验收与结束条件
- typecheck/lint、生产构建；既有设置/定时/人物/compact 浏览器回归。
- 真实浏览器截图检查正常宽度和 760px 窄视口，签名客户端隔离数据检查最终排版；更新可下载 dev 包。
- 按 start-dev / cross-review-code 执行 scoped requirements、adversarial、最终 architecture，全部通过后提交并关闭台账。
- 仅文档更新无需重跑运行测试；布局修改重跑受影响视觉和静态检查，不重复后台录制测试。

### Review 基线
- base: fd6bc682c247e21304c933fd905e3db3bb6c1d6e
- 范围：src/{App.css,SetupGroups.tsx,RecordingScheduleSettings.tsx,CloudServiceSettings.tsx,AudioStorageSettings.tsx,WorkspaceViews.tsx}、specs/DESIGN.md、本台账。
- 排除：README、打包脚本、其它未跟踪文件及其它任务提交。

### 行动清单
- [x] 实现/视觉验证
- [x] 静态/回归/三路审查
- [x] 签名 dev 包及提交闭环

### 待用户决策
无。

### 验证记录
- 2026-10-21：typecheck、lint 无警告通过，前端生产构建通过（既有 chunk 大小提示）；真实浏览器设置 32、定时 21、人物 21、compact 引导 4 项回归全部通过。
- 正常 1280px 和真实 760×540 iframe 视觉检查通过；覆盖下一次定时动作长状态、声音权限、模型/云服务、容量状态。证据 /tmp/hear-settings-evidence/hierarchy-detail.png、hierarchy-narrow-schedule.png、hierarchy-narrow-services.png。
- requirements/adversarial/final architecture 均无阻断；主流程关闭所需视觉证据缺口。没有修改事件处理、状态或业务协议，无新增机制。

- 签名 release App 构建通过、build identity 一致、codesign deep/strict 与 zip 完整性通过。原生隔离数据界面已查看，标题旁标签与下一行说明清晰；未改用户真实配置。证据 /tmp/hear-settings-evidence/hierarchy-native.png，预览已退出。
- 更新 dev 包：build/dev/hear-0.1.2-dev-20261021-002937-arm64.zip；对应目录内 hear.app。既有包保留，未安装覆盖。

- 交付提交 20f08aa；最终 HEAD typecheck/lint 再验通过。逐条核对契约、范围及非目标，单批次整体门禁通过，所有 mandatory 项完成；无待用户决策。仅状态与证据追加，不重复运行无影响的业务测试。
