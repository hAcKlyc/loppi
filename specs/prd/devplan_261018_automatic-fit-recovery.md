---
status: implemented
updated: 2026-10-18
---
# 自动整理触发、凭据恢复与状态可见性

用户要求一起修复：半小时触发延迟、Keychain 读取失败导致积压、实时卡片掩盖整理异常；查清本机凭据根因，构建重启并跑起积压任务。

## 执行台账
### 开发契约
- 必赢场景：候选起点满 30 分钟即进入既有 LLM 队列；继续录音时仍可见整理等待/异常；本机 LLM 凭据恢复且积压候选发布成功。
- 原子交付：单批完成逻辑、界面、Keychain 根因修复、回归、审查、部署验证；保留既有数据和持久任务身份。
- 必须保证：凭据只保存在 Keychain，不写入日志/数据库/安装包；不得扩大钥匙串访问权限；录音意图与来源开关重启后恢复；处理失败不假装正常。
- 明确不做：修改 ASR 对齐算法、重跑已完成历史、替换密钥为其他账号、增加凭据副本/云调度服务。
- 本次负责：当前三处故障及所需恢复路径；此前 DMG 构建改动保留；本批额外拥有 build_macos.sh 防止覆盖运行产物的修改（共用文件提交时包含其已授权的签名环境隔离）。
- 状态来源：candidate_windows/cloud_tasks 为整理事实，cloud_credential_refs 为 Keychain 引用，capture_state 为独立采集事实；Core 唯一写入者。
- 复用：events::refresh_candidates_in、task_failed_with_retry、set_configured_with_credential、retry_task_in、cloud::credential/save_credential、TimelineView、taskStatus。
- 新增机制：原则上无；仅在现有状态/错误投影与恢复入口修正，不建立第二份凭据或状态源。
- 不变量：IPC 写入由 Core 管理；后台 Keychain 读取不得等待交互；读取失败必须显式可见；人工暂停不被恢复覆盖。

### 验收与结束条件
- 回归：30 分钟差毫秒、正常静默触发、未到期限、重复扫描不重复入队；凭据错误与网络错误状态/恢复边界；录音与整理状态同时可见。
- 静态：Rust check/tests，前端 typecheck/lint，针对性 Node tests；签名构建。
- mandatory：读取实际 Keychain ACL/签名证据（不读出秘密）；修复后签名 App 真机凭据访问成功，积压任务至少出现实际发布；麦克风/电脑声音恢复。
- 按影响重跑对应检查；三路 fresh-context review 通过、验证与部署闭环后结束，不扩张其他任务失败。

### 开发批次与行动清单
- Batch 1：自动整理恢复
  - [x] Keychain 根因与恢复
  - [x] 时间触发与状态显示修复
  - [x] 回归与静态验证
  - [x] cross-review
  - [x] 构建重启、积压发布、提交

### 当前批次 Review 基线
- Batch base: f9845e710b829ac3652fe1748647a8b3b5232788
- 范围：core/events/cloud/main（按实际需要），协议与前端状态投影（按实际需要），回归测试、算法/架构文档及本台账。
- 排除：README.md、build_release.sh、scripts/package_macos.sh 的此前 DMG 改动及其它未跟踪文件。

### 待用户决策
无。

### 进展日志
- 2026-10-18：实际数据库确认 21:00 与 21:35 两个候选反复 keychain_read_-25293；后者跨度 1,799,993 ms，因此多等 15 分钟。运行中 core 的 SecCodeCheckValidity 返回成功，不能把失败直接归因于签名无效，继续核实 ACL。

- 2026-10-18：securityd 直接记录 running cdhash 30f2… 与磁盘 ffe0… mismatch，随后 ACL REJECTS；活跃两项凭据 ACL 均为 app.hear.core + 正确 Developer ID Team。根因是运行构建目录中的旧进程时反复覆盖 bundle。修复采用构建前检测运行产物并拒绝覆盖、独立安装到 /Applications 运行；不变更 Keychain ACL/密钥。重启与重新保存凭据时只唤醒凭据错误延期的现有任务。
- 2026-10-18：已修正墙钟截止、候选错误聚合、live 与处理状态并列、队列 pending/submitted 错误可见；复用 recover_tasks 与保存凭据入口唤醒凭据延期。253 项核心测试通过（3 ignored），12 项前端测试通过，typecheck/lint/cargo check 通过；构建覆盖保护实测返回 2 且未执行编译。ASR 恢复测试另验证不替换原 remote_id、不跨 provider 提前唤醒。

### Review Contract
- mode: repair; baseline f9845e710b829ac3652fe1748647a8b3b5232788。
- In scope: crates/hear-core/src/events.rs, scripts/build_macos.sh（含此前已授权的 Tauri 签名环境隔离）, src/api.ts, src/cloud-task-status.ts, src/TimelineView.tsx, scripts/test_cloud_task_status.mjs, scripts/test_timeline_view.mjs, specs/ARCHITECTURE.md, specs/tech_docs/algorithms.md, 本 devplan。
- Out of scope: README.md, build_release.sh, scripts/package_macos.sh 及其它未跟踪内容。
- Guarantees/non-goals: 同上契约；不扩大为自动安装/升级器、不允许构建与用户同时启动产物的原子互斥保证，不改 Keychain ACL；本次部署先停旧产物再构建、安装独立副本并恢复意图。
- Allowed mechanisms: 现有 task 状态投影增加错误标签；既有 restart/save 入口调整凭据失败的 next_attempt_ms；构建前检查运行产物，无新持久状态/协议字段/进程。
- Required lenses: requirements/adversarial/architecture all required。验收以回归/静态检查、securityd mismatch 证据、构建保护、最终签名 App 原凭据成功发布积压任务及双路录音为准。最终部署证据将在 review 后、提交前补齐，不把代码检查当作真机通过。

- 2026-10-18：requirements/adversarial/architecture 三路独立审查均无代码阻塞，最终 architecture PASS；浏览器实际渲染确认录音与钥匙串错误同时可见。开始签名构建与独立安装恢复。

- 2026-10-18 最终真机验收：签名构建通过，安装到 /Applications/hear.app，GUI/core 新 PID 58884/58893。原凭据未重存、未改 ACL；原两项 pending candidate 均 complete，分别关联 4/2 条事件，积压真实发布成功。revision 49→50 恢复原双路意图，CLI 与主窗均确认麦克风/电脑声音正在记录。mandatory 已全部关闭。代码审查与测试覆盖最终组合态；此前测试的代码无新增逻辑变更。
