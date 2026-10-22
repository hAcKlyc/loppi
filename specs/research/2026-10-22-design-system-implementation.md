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

### 待完成

Batch 1 requirements / adversarial 定向复核 PASS，最终 architecture PASS。原四项 blocker 与两项验收证据缺口均关闭。下一步：Batch 2 时间线 / 详情审美样板及组合验收。
