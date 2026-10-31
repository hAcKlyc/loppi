---
status: implemented
updated: 2026-10-18
---
# 人物选择与同场匿名说话人归并

## 目标
面板置于说话人按钮右侧、搜索行对齐点击标签，下方不足时整体上移并限制在视口内；删除截图标记的三处说明；人物库优先，然后新建人物并建立声纹，最后本场匿名说话人。保留逐句显示与单句/整组范围。无合格声音仍保存人物和归属，显示待补充声纹（用户已确认）。

## 执行台账
### 开发约定
- 核心场景：打开某句人物面板，选择已有姓名、明确新建、或本场匿名说话人，确认后按范围持久生效。
- 必须保证：取消不写入；两端版本检查；合并不创建全局人物；刷新、重启、自动声纹重算不撤销当前精转版本的人工匿名分组；无样例不谎称声纹成功；分页不遗漏本场候选。
- 明确不做：改变逐句展示、自动识别阈值、云 API；匿名归并不自动继承到重新精转生成的新句段（云编号没有跨版本身份意义）。
- 本次负责：定位稳定、冗余文案、完整候选、真正持久的匿名纠错、缺声纹状态。
- 状态来源：Core SQLite 持有身份与本版本人工匿名分组；Renderer 仅草稿；同场为同一 cloud target kind/id/version，不能跨场操作。
- 复用：current_scope_segments、precise_speaker_projection、object_revision、mutate_response、bind/unbind_speaker、maybe_add_sample、Person.sample_count、SpeakerPicker。
- 必要新增：按 precise segment 保存人工匿名 group 覆盖；与自动 speaker_groups 分离，避免自动重建覆盖人工判断。统一有效分组视图服务展示、范围、版本检查；新增同场候选查询和匿名归并命令。无新进程或 owner。
### 验收与结束条件
- Rust 回归覆盖单句/整组、跨场拒绝、版本冲突、自动重算持久、清理、无样例建人、已有声纹。
- 浏览器交互覆盖稳定右侧定位、隐藏文案、候选顺序、创建入口、无样例提示、局部选择与范围、取消/失败/重试；保留既有回归。
- 类型/lint、相关 Rust 测试、GUI 构建通过；按 cross-review-code 完成需求/对抗/架构只读审查后提交。
### 批次与行动清单
- 批次 1（base b86327d）：Core 分组覆盖与 IPC、UI、测试、技术文档、交叉审查、构建。
  - [x] Core 与协议
  - [x] UI 与缺声纹状态
  - [x] 验证、文档、审查、提交
- 用户追加：人物列表单行、头像缩小至文字行高、无第二行说明；待补充声纹同行显示。
### 待用户决策
无。无合格样例时保存人物并标记待补充声纹。

### 进展日志
- 2026-10-18：Core/协议与 UI 完成。用户追加搜索行对齐、单行列表已纳入。Core 全测 207 passed / 1 ignored，protocol 7 passed；前端 typecheck/lint、timeline 9 项通过。首次浏览器 37 项通过，新增 3 项定位/密度检查待最终运行结果。局部分组覆盖另有 3 项回归。

### Review Contract
- Mode: repair。需求：本台账 + 用户最后追加搜索行对齐、下方不足整体上移、头像与文字等高、姓名单行。先前三处说明移除、全局人物→创建→本场匿名、单句/整组、无声纹保存并标记待补充；逐句展示不变。
- Baseline b86327d；scope 为 git diff --name-only 的 13 个 tracked 文件（crates/hear-{core,protocol,cli}，src UI，scripts/test_speaker_picker.html，ARCHITECTURE 与 speaker-identity 文档），不审查 preexisting untracked。
- Guarantees / non-goals / defects / allowed mechanisms 同上开发约定。允许 Core 内按具体句段持久保存人工本场分组，统一有效投影、查询与归并 IPC；无新 owner。匿名归并不承诺跨新精转版本继承。
- Required lenses: requirements + adversarial，修复后 architecture。只读，fresh context，不再派生。

- 2026-10-18 审查裁决：requirements PASS；adversarial 发现 publish_match 沿旧声学组扩散姓名（blocking），新增背景匹配回归先失败、再改为共享 confirmed_group_person 有效分组入口，21项知识测试通过，定向复核 PASS。architecture 发现候选只读查询占写入锁（blocking），改用既有 readers.snapshot（3秒 deadline），待定向复核。
- 最新浏览器 40项 PASS，含用户追加定位和紧凑密度；真实隔离 Core IPC smoke PASS（建人无样例、单句归并、重复请求回执、重启保留），不涉及真实数据/云调用。

- 2026-10-18：架构定向复核 PASS。最终 debug IPC smoke 再次通过。macOS release 构建及 Developer ID 签名校验通过；已更新 GUI/Core，原双路记录开关保持，原生界面确认恢复正在记录。

- 整体验收：已完成所有功能与确定性验证。GUI 打包/启动和双路记录恢复已实测；人物浮层完整交互在隔离浏览器 40项通过。原生 CUA 在详情内无法定位坐标（windowNotFoundAtPosition，既有环境限制），未声称原生浮层点击验证完成；非阻断观感检查由截图/浏览器覆盖。
- 验证汇总：Core207通过1既有ignored、protocol7通过，变更后的identity21项再次通过；timeline9项，typecheck/lint、clippy（既有warning）、真实隔离IPC；macOS签名构建通过。未提交用户已有untracked素材和prd草稿。

- 提交：419b0f2。
