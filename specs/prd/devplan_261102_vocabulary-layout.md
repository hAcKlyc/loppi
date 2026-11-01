---
status: in-progress
updated: 2026-11-02
---
# 词库页面精简

用户反馈词库页面混乱：去掉解释性文字，设置统一上置，下方展示词库列表。主页面只保留标题、三个带名称的开关、词条列表/数量/操作、必要状态反馈。设置采用一排紧凑分组，新增入口归词条列表。保留编辑弹窗必要字段提示、错误与删除确认。

## 执行台账

### 开发契约
- 必赢：进入词库，顶部直接看到摘要与理解/本地转录/云端精转开关，下方是词条和新增入口，无介绍段落、用途说明、示例段落；无别名时不显示占位说明。
- 原子交付：一个前端布局批次；已有协议/持久化/CRUD/Tab 不变。
- 必须保证：开关与 CRUD 原行为、加载/失败/成功反馈、键盘语义保留；1120×760、760×540 不溢出。
- 不做：底层/云策略/状态逻辑调整，安装发布。
- 负责：主页面文字与内容顺序和紧凑布局，不回审前批底层逻辑。
- 状态来源：仍为 Core vocabulary；GUI 状态生命周期无变更。
- 复用：SettingRow/Switch/Button/LoadBoundary，现有 vocabulary-settings/list；无新 owner/store/protocol/retry/state。
- 架构约束：Core 唯一写入、共享 token/控件。

### 验收与结束
- typecheck/lint/build、设计资产回归、词库合成浏览器回归；截图确认设置在列表上、无说明段落、默认/窄窗口布局。
- CSS/JSX 改动重跑上述检查；不重做未改动的录音/模型/桥接/签名。
- 三路 fresh review PASS 且适用检查通过后结束；主观观感仅 advisory，无 mandatory OS 检查。

### 行动清单
- [x] 布局与文案调整
- [x] 针对性验证与截图
- [ ] cross-review / 提交 / 台账收尾

### Review 基线
- Base：711079f2d67c315d1631ff454faae4177a39a7a9
- Scope：src/VocabularyView.tsx、src/vocabulary.css、scripts/test_vocabulary.html、本 devplan、新 evidence/2026-11-02-vocabulary-layout；起始 scope 源码无其它改动，其余共享改动排除。

### 待决策
无。

### 进展
- 2026-11-02：按用户明确反馈收敛布局；仅移动现有控件和精简主页面文字，不引入状态逻辑。
- 2026-11-02：完成精简，设置三列成组置顶；列表标题含数量和新增操作；移除介绍/用途/例子及缺别名占位，空态仅「暂无词条」。typecheck/lint/build、设计资产8项、词库32项浏览器交互初轮通过，正在完成最终CSS截图与三路审查。

- 2026-11-02：最终CSS双浏览器32项回归通过，1120/760两尺寸均 settingsAboveList/allSwitches/settingsFit/noOverflow=true，staticExplanation=false；最终typecheck/lint/build与设计资产8项通过。requirements/adversarial/architecture全PASS，无blocker、证据缺口或mandatory遗留。
