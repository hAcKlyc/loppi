---
status: implemented
updated: 2026-10-23
---
# 设计系统落地与时间线 / 详情打磨

## 需求与范围

用户在 DESIGN.md 完成后明确要求先修复基础规范和公共组件，再打磨时间线与详情。本计划按已接受的 `specs/DESIGN.md` 第 3–10 节执行，以第 9 节 F1–F8 和说话人浮层补充项为基础范围。审美调整由同数据样例比较决定，并回写已采用的规则；不改录音、云任务、人物、保存和费用业务契约。

## 执行台账

### 开发契约
- 必赢场景：760×540 仍可浏览和操作；普通弹窗、详情、图片预览有完整模态键盘行为；说话人可按非模态规则离开；详情 / 队列 Tabs 可键盘切换；功能文字与焦点清晰。时间线与详情在常规、窄和大窗口有一致阅读层次。
- 原子交付：基础收敛与样板打磨完成后统一交付；两批只形成内部 review / commit 边界，不增加兼容壳或开关。保持现有本地数据、IPC、收费调用及已发布客户端契约。
- 必须保证：已确认值与草稿分离、未知请求复用原 ID、业务 owner 不变；不借样式变更更改录音状态、片段时间、内容真实性和云重试规则。保留首次设置 compact Key / 顶部评论等交互例外。
- 明确不做：重做信息架构、主窗深色 / 移动端、角色换图、Rust / IPC / 持久层重构、全局主题引擎；不操作用户录音、密钥或真实云请求。不将既有全应用 VoiceOver / 跨平台历史验收扩成此批工程承诺；改动涉及的键盘 / 可访问语义必须验证。
- 本次负责问题：DESIGN F1–F8、非模态 SpeakerPicker 焦点、共享组件改动引入或放大的邻近回归、时间线 / 详情视觉密度。
- 状态来源与生命周期：core / EntrySnapshot 决定业务事实，App 管理会话级录音控制，useSettingMutation 管理既有修订请求，ContinuousPlayback 管理播放资源；公共组件只拥有短期焦点、展开、呈现状态。
- 复用抽象：Controls.Icon / Switch / SegmentedOptions / Modal，LoadingState.LoadBoundary，useSettingMutation，Rabbit，DetailDrawer，SpeakerPicker，现有页面 CSS 与 tokens.css。
- 新增架构机制：无业务机制。扩展既有原生 dialog 基础以供 Modal / Drawer / 图片预览共用；Button / Field / Tabs / SettingRow 是受控展示原语，非新业务 owner。
- 架构不变量：参考 ARCHITECTURE 第 2、4–6 节；GUI 不写数据库，不增轮询，不根据回执伪造录音状态，不在公共组件内重试。仓库没有 CLAUDE.md / AGENTS.md，采用现有 architecture / DESIGN / skills。

### 验收与结束条件
- 静态：typecheck、lint、build；前端针对性回归及现有设置 / 日期 / 详情 / 人物 / 说话人 / 准备样例。
- 公共交互：新增可执行浏览器回归覆盖原生 dialog 的焦点进入、Tab、Escape、busy、嵌套和恢复；Tabs 手动激活；非模态离开；IME 不误提交。DOM 单测不冒充原生焦点验收。
- 视觉：真实组件 + 隔离数据，760×540、1120×760、1440×900；850 / 1100 边界检查，长文 / 空 / 错误 / busy / reduced-motion。对比度检查实际 token 配对。
- 原生 mandatory：涉及窗口布局与 modal WebView 行为，在隔离签名 macOS 窗口检查小窗口、焦点、标题栏避让。若环境不可用明确保留缺口，不宣告已通过。
- advisory：主观审美微调、额外硬件 DPR / 全应用 VoiceOver 历史覆盖；不把未做的检查记为通过。
- 影响关系：公共组件变化重跑交互和使用页；token / CSS 变化重跑相关视觉 / 对比度；业务逻辑未变不重复全量 Rust / 云集成。review 修复后只重跑受影响检查。
- 结束：两批、组合检查与 required fresh-context requirements / adversarial / architecture review 均通过；没有 blocker 或 mandatory 证据缺口，文档更新并显式提交本任务文件。否则记录未完成项。

### 开发批次与行动清单
- Batch 1：基础与公共组件，可单独验证 / 回退的交互规范修复。
  - [x] 补 token 角色、功能字号、对比度、控件尺寸、最小窗口。
  - [x] 受控 Button / Field / Tabs / SettingRow，扩展原生 Modal 生命周期。
  - [x] 普通弹窗、Drawer、图片预览迁移；SpeakerPicker 非模态与正确 portal 层。
  - [x] 修复日期等失效夹具，添加交互回归并验证相邻页面。
  - [x] 更新 DESIGN 实现状态；三路 review、修复与 commit。
- Batch 2：时间线 / 详情审美打磨，不改变业务状态和信息架构。
  - [x] 同数据比较排版、对齐、空间、表面与动效；落实选定方案。
  - [x] 三尺寸与边界 / 长内容 / 错误检查，记录证据。
  - [x] 同步 DESIGN；三路 review、修复与 commit。
- [x] 最终组合门禁、跨批次 review 与交付台账。

### 当前批次 Review 基线
- 批次：最终组合门禁（完成）
- Batch base：0666e02；最终生产候选：854b7b9
- 预期范围：src 相关 React / CSS，scripts 浏览器与公共组件验证，specs/DESIGN.md，本计划及本任务证据。
- 前置已有改动：DESIGN.md 为本对话上一轮产出，可纳入本任务；设计研究报告 / 预览同属本对话，验证期间保留。其他未跟踪 playground、旧 PRD / 研究与素材 ZIP 不属于本次，禁止批量 stage。

### 待用户决策
无。按已确认规范处理常规实现和视觉细节。

### 进展日志
- 2026-10-22：读取 start-dev / cross-review-code、DESIGN、架构和主要公共组件，建立台账。工作分支 codex/design-system-polish，保留原工作区资料。

- 2026-10-22 Batch 1：基础原语与调用方迁移完成。10 组浏览器回归全部通过；51 项前端逻辑测试通过，lint / typecheck / build 通过。焦点 / danger 配对已复算。签名 Tauri 隔离窗口验证 1120×760 与 760×540、原生按键、嵌套与详情焦点恢复；范围与方法见实施证据。进入三路 fresh-context review，尚未提交。

- 2026-10-23 Batch 1 修复：采纳 summary 焦点、加载中人物浮层焦点、secondary hover 对比度、写入反馈四项 blocker；补齐 9 尺寸边界及组合输入事件验证，进入原 reviewer 定向复核。

- 2026-10-23 Batch 1 门禁：requirements / adversarial 定向 PASS，完整最终 architecture PASS；静态 / build、51 项逻辑、11 组浏览器与 9 尺寸布局证据通过。基础收敛进入独立提交。

- 2026-10-23：Batch 1 已提交 `28d3ae3`。Batch 2 开始，以真实同数据截图比较和长转录 / 跨日 / 录音 / 空态 / 错误样例落实成品视觉品质；保留灰绿体系和业务 owner。

- 2026-10-23 Batch 2：采纳卡片空间 / 时间轴对齐、阅读15/1.75、长姓名组与独立操作、轻表面与稳定动效。18组视觉+减少动态模拟、9尺寸、11组组合回归、51逻辑与静态构建通过；隔离原生760/1120复验。进入固定快照review。

- 2026-10-23 Batch 2 门禁：requirements / adversarial / 最终 architecture 均 PASS；补齐修正入口 Tab → Enter → 编辑 → 取消的运行证据，关闭唯一 required gap。同步 DESIGN §6.2 旧扫光文字，无生产代码修复。静态 / build 与51项逻辑复验通过，进入独立提交。

- 2026-10-23：Batch 2 已提交 `854b7b9`。最终 `0666e02..854b7b9` 的 fresh requirements / adversarial / architecture 全部 PASS，无 blocker、required evidence gap 或待决策。最终 HEAD lint、typecheck、build 再次通过，51 项逻辑通过；audit 与已审定 polish 证据完全一致。跨批复验另有32项公共组件及实际子层 Escape / 非模态反向 Tab 通过。
- 2026-10-23：全部本轮必做范围完成，状态改为 implemented。原生 mandatory 已在隔离签名 macOS 窗口完成；全应用 VoiceOver、其它硬件 DPR 与发行安装认证保留既定边界。未修改用户录音、密钥、安装版或无关工作区资料；关闭独立验收 app，恢复浏览器临时视口。最终契约、结果及此台账纳入独立收尾提交。
