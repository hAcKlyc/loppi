---
status: implemented
updated: 2026-11-02
---
# 人物词库标题式 Tab

用户要求顶部「人物库 / 词库」Tab 放大到页面大标题并充当标题；内容区不重复显示「词库」，改为「应用范围」分组标题，下方为「词条管理」。同步移除人物列表重复主标题，保留所有操作。

## 执行台账
- 契约：顶部 Tab 使用既有页面标题字号/字重；保持共享 Tab 键盘/选中语义与列表页可访问主标题。词库下方「应用范围」+三开关，再「词条管理」+数量/新增/列表。人物列表移除重复标题，人物详情原行为保留。
- 原子范围：PeopleView.tsx、VocabularyView.tsx、vocabulary.css、test_vocabulary.html、test_secondary_design.html，本计划、新 evidence/2026-11-02-library-title-tabs。起始以上源码均无他人未提交变更；所有其它共享改动排除。
- 基线：666f86277cfec73984737384d5fa78db9fa9a457。
- 事实/复用：Core vocabulary状态和原组件生命周期不变；复用Tabs/TabPanel、Button、既有token、Heading/section语义，无新store/protocol/重试/owner/状态。
- 保证：默认人物库、Tab可点击/键盘切换、控件/CRUD/错误/加载不变、标题与分组清楚、1120/760无溢出。
- 不做：底层/桥接/录音/模型/云策略/安装发布。
- 验收：typecheck/lint/build、设计8项、浏览器词库/人物回归、Tab键盘/大字号/分组/无重复可见标题与两尺寸截图。修改按影响重跑；无OS mandatory，新观感仅advisory。
- 结束：范围检查、三路fresh review通过且适用验证通过后提交收尾。

### 清单
- [x] 实现与验证
- [x] 三路审查与提交

### 待决策
无。

### 日志
- 2026-11-02：按用户具体版式要求调整标题层级，不增加解释文案或改变业务行为。
- 2026-11-02：标题Tab使用共享26px/semibold token；为读屏保留视觉隐藏的当前列表主标题；应用范围与词条管理使用h2。移除人物列表重复标题并保留新增按钮。共享设计验收选择器支持标题式Tab。
- 2026-11-02：typecheck/lint/build及设计8项PASS；WebKit/Chrome各词库36+人物21项PASS；两尺寸标题26px、无横向溢出，分组正确；键盘ArrowRight/Enter切换成功。三路审查待完成。
- 2026-11-02：共享设计验收people/person×macOS/Windows呈现×760/1120共8组PASS，合成数据预览已保存。requirements/adversarial均PASS，无需修复；最终架构审查待完成。

- 2026-11-02：final architecture PASS，无发现；三路完整审查结束。最终源码与通过的验证快照一致；已重读全契约，无遗漏范围、新机制或mandatory遗留。

- 2026-11-02：实现提交fda52b8470b46b542a7afff77ed309d33228fa61；原子交付门禁完成，状态implemented。仅显式本任务文件提交，其余共享改动保留。主观advisory验收：顶部标题式Tab点击切换、应用范围三开关、词条管理新增/编辑；相应行为已由合成组件和双浏览器验证，无新增OS/native发布声明。
