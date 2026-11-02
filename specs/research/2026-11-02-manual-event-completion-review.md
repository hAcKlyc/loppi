# 人工确认结束：独立审查记录

Review Contract：repair；分支 `codex/manual-event-completion`；基线 `deb4ddf9efeca7062e7717fb26ae4715e280d541` 到当前批次最终候选。需求与完整验收见[开发台账](../prd/devplan_261102_manual-event-completion.md)。范围是该台账列明的Core完成/重整/自动发布/ASR、共享协议与分发、详情GUI、相关测试和文档；无已知共享工作树外部改动。允许的新增机制仅为完成时间、现有重整请求完成目标/修订、新IPC请求与协议26。无新增owner、队列、重试器或第二事实库。未承诺真实LLM每次语义正确，不改权限/采集、不发版安装。

## Phase 1：固定快照，独立只读

- requirements：完整逐条追踪两分支、参考时间来源、多输出全部结束、无变更也结束目标、失败保留旧结果、自动续接保护、ASR复用/晚到音频、详情/菜单、普通显式编辑、迁移/协议与验证，均PASS，唯独失败重试为PARTIAL（R1）。无独立必做证据缺口。
- adversarial：独立确认同一R1；事务/回执、旧输入校验、已结束事件保护、ASR引用与晚到音频、正常重整语义、不确定请求ID保持均通过。检查扩展上下文与清理路径，没有另一个已证实阻断：目标删除由active_event拒绝，保留证据变化由输入快照拒绝。

### R1 — 完成重整因目标修订变化失败后，显式重试仍永久使用旧修订

两个reviewer均报P2。路径：改时完成入队 → 自动或人工修订目标 → 旧输入正确拒绝 → 现有RetryCloudTask只重置任务、不刷新完成请求的expected_revision → 每次再次失败；详情失败只导向队列，无法通过受支持恢复路径完成。违反台账「失败可重试」，属于本次新增目标修订约束显著暴露的问题。

主Agent裁决：**blocking defect**。根因修复放在原 `retry_task_in` 的显式重试事务内，调用完成owner刷新存活目标的当前修订；正常自动执行绝不更新预期修订，发布仍比较完整输入快照，删除目标拒绝重试。没有新增重试机制或持久状态。GUI恢复入口不变。

新增 `explicit_retry_accepts_latest_revision_but_rejects_old_results_and_deleted_targets`：原快照在重试前后都不能发布；最新内容重试可成功完成；已删除目标保持failed、不能复活。完成专门回归现为8项PASS。受影响：Rust静态/回归/构建、隔离IPC运行检查，均需复验；未改GUI，已有组件/浏览器证据继续有效。

## 最终状态

- 原requirements与adversarial reviewer均定向复核R1 **PASS**：显式重试接受最新存活修订，普通重整不变，旧结果与已删除目标仍被拒绝；没有剩余R1相关问题。
- 修复后完整 `npm run verify -- --report /tmp/loppi-verify-completion-final.json` **PASS**（实际21个检查；含402项Core、14项协议回归与121项前端回归）。隔离真实Core接口再验证ConfirmEventCompletion、回执回放、EventDetail、改时入队、RetryCloudTask刷新修订与无效请求拒绝 **PASS**。
- 最终独立architecture reviewer结论 **PASS**。完成事实归属named_events，任务生命周期归属cloud_tasks，ASR仍沿既有规划/复用；音频触发器只在媒体写事务中失效原有规划标记，显式重试只在原事务刷新已获准的请求修订。没有额外owner、store、worker、queue、retry engine、fingerprint、fallback或兼容分支。
- 修复后完整macOS ARM开发包构建、四组件协议/身份一致性、包内只读模型烟测均PASS。无待用户决定项，无未解决阻断；提交由主Agent在台账收尾。
- 已审查生产候选提交为 `74d0c9d`；最终HEAD的typecheck/lint/workspace check再次PASS。后续收尾只更新台账及本记录，原运行证据与审查继续适用。三视角门禁与单批次原子交付门禁完成。
