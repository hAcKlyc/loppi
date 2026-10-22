# Batch 2 Review Contract

1. Mode repair；reviewers只读，不派遣或编辑。工作树固定。
2. Requirement：specs/prd/devplan_261022_design-system.md Batch2、specs/DESIGN.md 第8节已采用规则。用户要求成品视觉品质；既有灰绿方向、业务与信息架构不变。
3. Target当前工作树；exact base 28d3ae3。Batch1已三路PASS并提交，不把此前已修复/已裁决事项重新当作本批遗漏。
4. In scope batch2-files.json及本contract/manifest；其他未跟踪playground/旧specs/素材均不属于任务。
5. Required：时间线/详情在760、1120、1440与850/1100边界可读可达；长文/长姓名/跨日/录音/空/错误/busy/reduced-motion；正文/焦点清晰；卡片hover无位移，内容与动作层次稳定；规则回写DESIGN。
6. Owned defects：本批引入/显著放大或上述明确要求。非目标：业务算法、真实数据/云/录音、主题引擎、全应用VoiceOver/发行认证、其它OS。不能把审美个人偏好自动升级blocker，但具体层次/溢出/不可达问题须报告。
7. Architecture：无新增机制。只有token、既有CSS owner与详情元信息分组div；生产状态/请求/生命周期不变。
8. Required lenses requirements/adversarial后最终architecture。只读检查实际最终diff及已有截图，不必重复已证明的运行；工具不可用如实说明。
9. Evidence：implementation.md Batch2、polish-{browser,layout,visual,audit}。11组UI、18组视觉与reduced模拟、9尺寸、51逻辑、静态build、隔离签名native760/1120均通过。CSS/token变更重验相关视觉/配对；详情markup变更重验人物/折叠/修正键盘与定位；不因无关文档重跑高成本构建。

结论请简洁列出PASS或具体可达blocker/requiredgap，给文件与触发条件。无问题即结束本轮，不扩张额外保证。
