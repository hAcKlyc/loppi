---
status: in-progress
updated: 2026-11-02
---
# 人物库与词库双 Tab

现有人物库页面顶部增加「人物库 / 词库」Tab，默认人物库。词库提供规范名称、类型、别名、启停、新建编辑删除与三个用途开关；复用现有 vocabulary 协议。侧栏仍为人物库，不新增导航页。所有编辑通过 Core 保存，不自动重建历史、不变更模型或云端策略。

## 执行台账

### 开发契约
- 必赢场景：从人物库切到词库，创建「看山」及误写「开山」，保存后列表可见；修改、停用、删除、开关持久保存；切回人物库后原有人物管理正常。
- 原子交付：一个前端批次交付；复用已发布协议，无迁移。
- 必须保证：默认人物库；Tab 键盘可用；人物详情离开时停止播放；加载/失败/空列表区分；提交不重复、错误保留草稿；失败不伪造成功；旧读取不能覆盖写入结果；自定义类型可保留。
- 不做：批量导入、自动发现词条、历史批量替换、云端准确率承诺、安装发布。
- 负责问题：本次新增页面与表单的交互、请求状态与人员页回归。
- 状态来源与生命周期：Core vocabulary_entries/settings 唯一持久事实；GUI 仅保留页面快照与编辑草稿；页离开淘汰读取；mutation 返回完整词库作为最新快照。
- 复用抽象：PeopleView、core、Tabs/TabPanel、Button、Modal、Field、Switch、SettingRow、LoadBoundary；Vocabulary Action 与 Response::Context 的 result 内层 success/error。
- 新增架构机制：无；新增词库组件与协议调用函数，无新 store、协议、后台队列、自动重试。
- 架构不变量：Core 唯一业务写入；共用 token 和控件；只发已存在协议；不读真实录音或凭据。

### 验收与结束条件
- 静态检查、前端 build；真实组件合成 IPC 交互和截图验证默认 Tab、切换、CRUD、重复提交、错误保稿、开关、迟到读取、长内容和窄窗口；现有人物回归。
- 事实依据：浏览器真实组件与合成数据验证 UI；现有 Rust vocabulary 测试与调用点证明持久协议，原生 bridge 无变更。
- 改动影响：组件变更重跑静态/交互/build；CSS 变更重跑视觉；协议 owner 未改不重做录音/模型/签名验收。
- 结束：三路 fresh-context review 完成，范围内 blocker 修复且适用检查通过；主观视觉为 advisory，无新增 OS 集成 mandatory。

### 开发批次
- Batch 1
  - [x] 页面与词库实现
  - [x] 静态、构建、合成交互/截图与人物回归
  - [x] cross-review 与修复
  - [ ] 显式范围提交与交付台账

### Review 基线
- Batch base：eeef7110aa6d43051975b0da02f8f51eaacdc0a9
- 范围：src/PeopleView.tsx、src/VocabularyView.tsx、src/vocabulary.css、scripts/test_vocabulary.html、本 devplan。PeopleView 起始无他人改动，其余仓库改动均不属于本任务。

### 待用户决策
无。

### 进展日志
- 2026-11-02：确认现有 vocabulary 协议与存储已实现，前端无入口；确认共享工作树大量其它改动，限上述文件交付。
- 2026-11-02：实现双 Tab、独立词库快照、完整 CRUD、用途开关与错误保稿。附加保护新建同名不得覆盖既有条目，CLI 自定义类型原样保留。现有人物回归选择器改为定位返回按钮，避免新增同名 Tab 后歧义。
- 2026-11-02：WebKit 真实组件隔离交互 28 项通过，人物回归 21 项通过；1120×760 / 760×540 无横向溢出，编辑弹窗在窗口内且可滚动；Node 前端回归 114/114 通过；静态与 build 已通过。Chromium 缓存缺可执行文件，不将其记录为通过；正在补用本机 Chrome。
- 2026-11-02：两引擎（WebKit、系统 Google Chrome）均通过键盘方向键/Enter Tab 激活、真实鼠标切换、Escape 关闭与真实表单填入保存。
- 2026-11-02：requirements PASS；adversarial 发现同名停用 CLI 条目无法编辑，接受为 blocker，重复名称保护限制为新建场景，已有条目仍遵循 Core 启用冲突规则。修复后 typecheck/lint/build PASS；两浏览器词库 30 项 + 人物 21 项全部通过。架构审查待完成。

- 2026-11-02：adversarial 定向复查 RESOLVED；final architecture PASS，无新增 owner/protocol/store/重试。已重读完整契约，所有范围与适用验收完成；无 mandatory 遗留。最终源码组件未再变更，单批 review 覆盖整体。
