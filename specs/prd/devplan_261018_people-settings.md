---
status: implemented
updated: 2026-10-18
---
# 人物详情与设置控件统一

## 开发约定
- 人物库纯信息列表，整行进入独立详情视图；详情名称行内编辑、确认弹窗删除人物，展示全部有效声纹，支持播放/停止与删除；新建改为模态表单。
- 组合样本只播放真实入库片段；原音不可用明确说明；切人/离页/删样本停止播放，旧异步结果不得串到另一人物。
- 设置布尔项使用标准 switch（保存中禁用，失败保持事实状态），互斥项使用 radio 分段选项，执行动作使用按钮；沿用 C 色块设计与已有 tokens。
- 密钥已保存时显示固定长度圆点，真实密钥不读回 UI；选择删除可输入新值，未编辑/空值不提交，替换失败保留原配置，两个供应商不会串值。仍按回车/离焦验证保存。
- Core 是人物/样本/凭据持久 owner，前端只调用已有 mutation/playback 接口；VoiceSampleView 增量附加片段和可播放性，复用原音保留检查。无新服务、通信模式或持久 owner，不改变声纹入库策略。
- 不测试修改真实用户人物/密钥，不新增依赖，不修改逐句转录。

## 验收与结束条件
- 类型/格式/前端构建、受影响 Rust 测试、隔离 mock IPC 交互回归；签名桌面视觉验证；按 cross-review-code requirements/adversarial/final architecture 审查，修复 blocker 后提交。
- 验证创建/重命名/删除确认、复合片段播放范围与停止、加载竞争和错误、开关/选项语义、圆点不提交与替换失败保留。
- 真实云 key 不更换，真实人物不删除；用 mock 回归验证 mutation。

## 执行台账
- Batch base: bf8e40d
- 单批次；待用户决策：无。
- [x] 人物详情与样例播放
- [x] 设置控件与安全密钥编辑
- [x] 文档、回归、审查、签名验证与提交
- Review contract: repair，范围仅本任务人物/设置 UI、共用必要控件、VoiceSampleView 投影及对应测试/设计文档；不扩展旧播放/清理/识别策略。

### 审查约定与当前证据
- 基线 bf8e40d；当前变更：src/{PeopleView,Controls,CloudKeyInput}.tsx、sample-playback.ts、people-settings.css、WorkspaceViews.tsx、App.tsx、App.css、api.ts；Core knowledge.rs/events.rs、enrollment/tests.rs；protocol/lib.rs；scripts/test_people_settings.html；DESIGN.md、speaker-identity.md。
- Guarantees/非目标按上方开发约定；新 View 字段是增量序列化字段，不新增 IPC 请求或持久结构。前端状态只拥有编辑草稿和播放生命周期。
- requirements/adversarial 先审，修复后 final architecture；审查只读，不修改、不再派发。
- Core229+protocol8通过（2项忽略），typecheck/lint通过；隔离浏览器29项交互回归通过，包含人物确认删除。未用真实数据执行 mutation。
- requirements / adversarial / final architecture 均 PASS，无 blocker；对抗补充验证原播放器8项与组合播放边界、停止后晚回包、清理失效和资源释放。
- Clippy完成，保留既有警告；签名构建通过，视觉发现详情返回按钮受旧 margin-left:auto 影响，已限定覆盖并验证左侧位置，最终构建及桌面检查中。
- 最终签名构建及 deep/strict 校验通过；桌面检查人物纯列表、详情、行内编辑取消、新建弹窗聚焦/取消、设置 switch/radio 和圆点密钥均通过。真实人物和密钥未修改。更新后 GUI PID15584，Core重启；status确认麦克风与电脑声音 recording、blocker none。
- 原子提交 ff772e0；最终实现与已审查候选一致，无待决事项和未完成 mandatory。原有未跟踪设计/研究/PRD文件保持原样。
