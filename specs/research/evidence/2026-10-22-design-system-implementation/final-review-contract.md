# Final Review Contract

1. Mode repair；reviewers 只读、不改文件、不派遣。Phase 1 固定快照；主 Agent 裁决后才修复，最终 architecture 审完整候选。
2. Requirement source：`specs/prd/devplan_261022_design-system.md` 整份契约与全部验收点，`specs/DESIGN.md` 第3–10节（F1–F8、非模态人物浮层及已采纳的两个视觉样板）。用户要求以高视觉品质实施基础规范、公共组件、时间线和详情。
3. Exact target：`0666e02..854b7b9`，全次交付组合态。此文件为唯一新增 review contract；最终状态文档将在通过后另行提交。
4. In scope：该范围的全部 tracked diff（可用 git diff --name-only 0666e02 854b7b9 获取），加本文件。其余未跟踪 playground、素材 ZIP、历史 devplan / draft / research 不属于任务。
5. Required：760×540 仍可读可操作；普通 Modal、详情、图片具备初始/循环/返回焦点、最上层 Escape、背景隔离、busy 与嵌套；人物浮层非模态离开；Tabs 手动激活/ARIA；功能字号、实际对比度与 token 有效。时间线/详情在三主尺寸及850/1100边界稳定，长中文、长姓名、跨日、录音、空、错误、busy、减少动态不退化；设计规则回写。
6. 保持既有录音、播放、分页、修订、未知请求ID、收费、密钥和compact首次设置契约。只处理本次引入/显著放大或明确要求的缺陷。非目标：重做信息架构、Rust/IPC/持久层、主题引擎、真实录音/云调用、全应用VoiceOver、其它OS、完整发行安装认证。不能把个人审美偏好升级为 blocker。
7. Allowed architecture：受控展示原语、原生 dialog 内局部焦点/反馈、SettingsView 当前只读预览 generation。业务 owner 不变；无全局modal管理器、新store/protocol/retry/poll/theme、批次过渡兼容层。
8. Required lenses：fresh requirements + adversarial 并行，主 Agent裁决，最后fresh architecture。重点核对跨批组合是否仍满足同一契约；不是机械要求重新跑已证明的所有原生检查。
9. Evidence：`specs/research/2026-10-22-design-system-implementation.md`、foundation/polish 结果与截图。最终组合态已有11组UI、18视觉+reduced模拟、9尺寸、51逻辑、lint/typecheck/build；隔离签名Tauri760/1120检查，修正键盘独立补验。最终HEAD共同静态门禁再执行。所有生产代码与这些运行时证据一致；仅文档改动不使运行证据失效。
10. 影响规则：公共组件行为改变重跑相关使用页/焦点；CSS/token重验相关布局/配对；详情markup重验人物/折叠/修正/定位。不改业务/Rust不扩张真实云/录音验证。报告必须区分 blocker、required evidence gap、additional coverage、pre-existing / out of scope；给出具体可达路径与文件行。

前置结论：Batch1 `28d3ae3` 三路PASS，已修复summary焦点、人物加载焦点、secondary hover对比度和可见busy反馈。Batch2 `854b7b9` 三路PASS，唯一修正键盘证据缺口已关闭；文档旧扫光表述已同步。此前每项裁决见实施证据。不把这两个局部PASS直接替代本次整体审查。
