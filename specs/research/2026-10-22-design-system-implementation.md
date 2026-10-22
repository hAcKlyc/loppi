# 设计系统落地证据

## Batch 1 · 基础收敛

基线 `0666e02`，分支 `codex/design-system-polish`。以 `specs/DESIGN.md` F1–F8 与非模态浮层补充项为范围。

- 已完成 Button / IconButton / Field / Tabs / TabPanel / SettingRow，以及供 Modal、Drawer、图片共用的原生 Dialog。共有样式归入 controls.css。
- 对比度、功能字号、medium、图表 / entry 角色与控件尺寸归入 tokens。app-shell 不再强制 600px。
- 只读清理预览可取消；view generation 丢弃已取消的返回。写入期间才锁定关闭，业务请求 / 修订仍由原 owner 负责。
- 原图异步加载会使触发按钮暂时 disabled，因此显式传递返回焦点引用；不能只在图片打开时读取 activeElement。
- WebKit 原生 Tab 受系统偏好影响，局部 Dialog 管理完整可见控件序列，native dialog 仍负责背景 inertness。SpeakerPicker 不循环 Tab，Esc 阻止父层默认 cancel。

### 可执行证据

[浏览器结果](evidence/2026-10-22-design-system-implementation/foundation-browser.txt)：11 组全部通过；公共组件 32（含普通 / compact Key 组合输入与 busy 宽度）、App 表单 9、设置 39、人物选择 47、人物库 21、首次设置 4 项，另含日期、详情定位、加载与现场记录场景。`scripts/test_design_suite.html` 以 1120×760 子窗口顺序执行，模拟请求只作用于内存。

`node --test scripts/test_*.mjs`：51 / 51。`npm run typecheck`、`npm run lint`、`npm run build`：通过；构建保留既有大于 500 kB chunk 提示。

[静态 / 对比度审计](evidence/2026-10-22-design-system-implementation/foundation-audit.json)：未定义 token、生产 CSS 裸色值、裸字号均为零。focus 对 surface / canvas / sidebar / recording 最低 4.53:1；danger 对这些表面最低 5.04:1。未将装饰图形、禁用项或角色素材一律当作正文对比度。

### 原生验证

编译同一仓库的 Tauri 宿主，临时配置使用独立 bundle id `app.hear.design-review`，Developer ID 签名并通过 `codesign --verify --deep --strict`；数据位于 `/tmp/hear-design-review-data`。用户安装版 `/Applications/hear.app` 保持运行且未改动。原生业务桥为只读属性，业务样例在填满 WebView 的同源 iframe 内使用 mock IPC；公共 Dialog 样例直接运行于顶层 WebView。未使用真实录音、密钥、云请求或系统权限操作。

真实原生按键验证：短表单进入输入；Tab / Shift-Tab 返回预期控件；危险子弹窗聚焦取消；Escape 仅关子层并返回子层入口，再 Escape 回到主入口。详情进入标题，Escape 后返回时间线原卡片。

[1120×760 时间线](evidence/2026-10-22-design-system-implementation/native-foundation-1120.png)、[760×540 时间线](evidence/2026-10-22-design-system-implementation/native-foundation-760.png)、[760×540 详情](evidence/2026-10-22-design-system-implementation/native-foundation-detail-760.png)。最小尺寸通过临时 Tauri 配置直接启动，截图 2× 为 1520×1080；主区域滚动，详情双栏、输入、关闭与标题栏留空可达。截图中的顶部屏幕控制标记来自系统测试界面。

这些证据覆盖本批前端 / WebView 行为，不代表整应用麦克风、原生兔子、发行公证、跨平台或 VoiceOver 全面认证。

### Batch 1 审查修复与补验

requirements / adversarial 两路都复现了自定义 Tab 序列遗漏 summary，裁决为本批引入的 blocker；已加入原生折叠入口，并在真实详情用实际 Tab → Enter 展开 → Shift-Tab 返回截图入口复验。SpeakerPicker 未就绪时改为聚焦可用关闭入口；加载、失败、恢复不抢焦点、Escape 仅关浮层及 Tab 离开均有真实 DetailDrawer 组合回归。

“新建事件”改用已定义的 secondary 按钮，删除页面重复覆盖；normal 6.07:1、hover 5.62:1（精确值见 audit）。Modal 与 Drawer 使用原 busy 状态展示“正在处理，请稍候…”，不新增写入 owner；延迟拒绝的评论 / 事件 / 管理 / 导出 / 清理 / 笔记验证守卫、反馈和失败恢复。带 busyLabel 的公共 Button 预留两种文字宽度。

[布局边界结果](evidence/2026-10-22-design-system-implementation/foundation-layout.txt)：760、849、850、851、1099、1100、1101、1120、1440 宽度全部通过，关闭 / 笔记 / 新建与侧栏可达，无主区域横向溢出。IME 使用 CompositionEvent 与 isComposing 的隔离事件验证，不冒充真实中文输入法候选窗测试；原生键盘路径另验。

### Batch 1 审查结论

Batch 1 requirements / adversarial 定向复核 PASS，最终 architecture PASS。原四项 blocker 与两项验收证据缺口均关闭。基础批次提交为 `28d3ae3`；后续视觉样板和整体结果见下文。


## Batch 2 · 视觉样板

基线 `28d3ae3`。先用同一三事件时间线和 14 段长中文 / 两个姓名 / 六条现场记录保存 before，再修改生产 CSS；没有把研究候选覆盖样式作为运行时主题。

采用更清澈的 canvas / sidebar，保留已达标的正文与 muted。常规卡片 16/20px 内边距、16px 间隔，窄窗回到 12/16px 与 12px；时间、圆点和首行元信息中心在 1120 窗口分别为 391.70 / 391.80 / 391.89px。日期 / 详情标题 600、普通导航 500。卡片不再悬停上移，实时卡片取消持续扫光，仍保留真实录音文案、绿底、状态点与 Rabbit 动效。

详情正文采用独立 reading 15px / 1.75；长姓名与来源在同组内换行，修正操作保持右侧，避免额外一整行空白。播放器间距收敛到 12px，按钮组 8px。静态标题的程序焦点不画默认蓝框，交互焦点与原生 summary 均保留清晰指示。按压反馈改为内描边：原 brightness(.94) 会使 quiet hover 的文字对比度由 4.53 降至约 4.38，现按压时保留原前景和背景。

### 同数据前后对照

| 场景 | 基础收敛后 | 视觉打磨后 |
| --- | --- | --- |
| 1120 时间线 | [before](evidence/2026-10-22-design-system-implementation/timeline-before-1120.png) | [after](evidence/2026-10-22-design-system-implementation/timeline-after-1120.png) |
| 1120 长文详情 | [before](evidence/2026-10-22-design-system-implementation/detail-before-1120.png) | [after](evidence/2026-10-22-design-system-implementation/detail-after-1120.png) |
| 760 长文详情 | [before](evidence/2026-10-22-design-system-implementation/detail-before-760.png) | [after](evidence/2026-10-22-design-system-implementation/detail-after-760.png) |

[1440 详情](evidence/2026-10-22-design-system-implementation/detail-after-1440.png)、[760 跨日](evidence/2026-10-22-design-system-implementation/crossday-after-760.png)补验长文和跨日时间列。浏览器截图使用固定逻辑视口的 full-page capture；原生证据另列。

### 验证结果

- [视觉场景](evidence/2026-10-22-design-system-implementation/polish-visual.txt)：760×540、1120×760、1440×900 × 时间线 / 长文 / 跨日 / 录音 / 空态 / 失败，共18组通过；长文和现场记录滚到底后输入区仍可达。另通过减少动态效果模拟：仅样例内将实际 CSS media 分支与 React matchMedia 偏好激活，未更改 OS，不能据此宣称全平台原生偏好认证。
- [9 尺寸边界](evidence/2026-10-22-design-system-implementation/polish-layout.txt)：含850 / 1100两侧均通过。长标题、关闭、新建、侧栏与笔记可达，无横向溢出。
- [11 组组合回归](evidence/2026-10-22-design-system-implementation/polish-browser.txt)通过，含日期布局、详情加载 / 定位、人物浮层、表单 busy / 错误、设置 / 准备；51项前端逻辑测试通过；lint / typecheck / build通过，仍只有已有大chunk提示。
- [Token / 配对审计](evidence/2026-10-22-design-system-implementation/polish-audit.json)：无未定义变量、生产裸色或字号；quiet hover最低4.53，secondary hover5.62，muted在录音hover表面4.53。本轮没有引入候选中不达标的浅色辅助文字。
- 同一隔离签名 Tauri 宿主复验760和1120，签名深层校验通过。前端由本地Vite加载，业务场景在完整WebView同源iframe内mock；不能混同发行安装包的真实IPC验收。原生实际操作：图片预览Escape返回截图入口，Tab到summary，Enter展开，Escape关闭详情回到卡片。

原生截图：[760 时间线](evidence/2026-10-22-design-system-implementation/native-polish-timeline-760.png)、[760 长文详情](evidence/2026-10-22-design-system-implementation/native-polish-detail-760.png)、[1120 时间线](evidence/2026-10-22-design-system-implementation/native-polish-timeline-1120.png)、[1120 长文详情](evidence/2026-10-22-design-system-implementation/native-polish-detail-1120.png)。macOS顶部屏幕控制标记为验证环境UI。

### Batch 2 审查与补验

requirements 确认实现符合规则，指出 contract 第9条缺少独立“修正”键盘执行记录，裁决为 required evidence gap；adversarial PASS，无实现 blocker。2026-10-23 在 Chrome 的隔离 `test_design_scene.html?scene=detail`、760×540 视口补验：第一句人物按钮 Tab → 实际焦点为“修正” BUTTON；Enter 打开编辑器；Tab → “修正这句文字” TEXTAREA，原文完整；Tab → “取消” BUTTON；Enter 后编辑器移除、详情仍打开、第一句原文不变。没有点击保存或调用真实写入。此次仅补运行证据，生产代码未修改，不使已通过的视觉 / 原生 / 组合检查失效。

Batch 2 requirements 定向 PASS、adversarial PASS、最终 architecture PASS；已同步 §6.2 旧扫光文字，消除规范与实现冲突。未引入业务状态、资源 owner 或第二套样式机制。视觉批次提交为 `854b7b9`；未改录音、播放、修订、请求ID、持久化或费用算法。


## 最终组合交付 · 2026-10-23

按[最终契约](evidence/2026-10-22-design-system-implementation/final-review-contract.md)，从首批基线 `0666e02` 到最终生产提交 `854b7b9` 做完整跨批审查，未将局部 PASS 直接相加。

| 独立审查 | 结论与边界 |
| --- | --- |
| Requirements | PASS；F1–F8、人物非模态、两样板、业务边界和证据闭环，无 blocker / required gap。 |
| Adversarial | PASS；Dialog / Tabs / SpeakerPicker / 清理预览迟到结果及详情组合无可达回归。另在 Chrome 实跑32项公共组件，实际键盘确认子弹窗 Escape 恢复入口、人物浮层反向 Tab 自然离开且保留外部焦点。 |
| Architecture | PASS；完整 React / CSS diff 的 owner 与生命周期正确，复用原生 dialog 和现有业务抽象；没有全局弹层管理器、业务 store、重试、依赖或批次临时机制。 |

主 Agent 无待修复项。最终 HEAD 的 lint / typecheck / build 再执行通过，51项逻辑测试通过；静态配对审计与已审定 `polish-audit.json` 完全一致。[最终检查输出](evidence/2026-10-22-design-system-implementation/final-gate.txt)记录执行结果。组合态已覆盖11组界面、18组视觉与减少动态模拟、9尺寸和隔离原生760/1120；自这些运行后生产代码未变，文档收尾不使证据失效。

本轮必做范围及原生 mandatory 已完成。独立验收应用已关闭，临时浏览器视口已恢复；用户安装版及无关工作区资料保持原状。全应用 VoiceOver、其它 OS / 硬件 DPR、真实录音 / 云请求与完整发行认证不在本轮完成声明内。后续全页面审美推广继续沿用 DESIGN §8.3，不能把本轮两个样板扩大解释为所有场景均已精修。
