---
status: implemented
created: 2026-11-01
updated: 2026-11-01
---

# 设置页显示云处理状态

用户反馈今天下午没有云整理或精转。只读调查确认两云队列已配置但保持暂停，LLM 有 72 项待办。用户明确授权恢复后，根 Agent 通过公开 CLI 恢复两队列；本项修正设置页仅显示“已配置”、没有表达暂停及待办的缺口。

## 开发契约

- 必赢场景：设置页同时表达配置与当前处理事实；已配置但暂停时明确写“已配置 · 已暂停”，并提示“在处理队列中继续”。
- 范围：CloudServiceSettings 及 SettingsView 的调用、合成 SSR 回归、本台账；根 Agent 负责日期改动、整体验证、独立审查、提交和安装。本子任务不提交 Git。
- 状态来源：既有 Core CloudQueue 快照；SettingsView 已持有 queues，组件直接接收完整对应项。配置结果待确认仍来自既有 cloudOperation，不新增状态、请求、轮询、持久字段或路由。
- 必须保证：未知不冒充运行；未配置、配置待确认、阻断、暂停、处理中、等待处理及空闲分别表达。阻断/暂停优先于正常进度；现有密钥模态、按钮、配置及写入行为保持。
- 明确不做：自动恢复、云重试、新队列动作、修改 Core/协议、传播私人正文或凭据、将已配置等同连通性实测。
- 文件归属：只编辑 src/CloudServiceSettings.tsx、src/WorkspaceViews.tsx 中该组件调用，可新增 scripts/test_cloud_service_status.mjs 与本台账；共享工作区其它更改全部保留。

## 事实与限制

- 恢复前：LLM configured=true、paused=true、pending72、processing0、failed0；ASR configured=true、paused=true、pending0、processing0、failed0。
- 完整保留日志显示 10/31 18:58:48 两次成功 CLI set_cloud_paused 命令；日志没有 provider/paused 参数、操作者或理由，暂停来源无法确证。更名验收的“云处理保持暂停”是吻合背景，不当作命令身份审计。
- 恢复后约 18:34:56：LLM pending68、processing1；ASR processing1；两项 paused=false、failed0。随后观察到 LLM 3 complete、ASR 1 complete，均无 provider/credential 错误。
- 当前源码自动精转等待 LLM 发布事件及稳定观察；FIT_SYSTEM 已没有独立 transcribe 字段，不应沿用旧文档的额外判定描述。
- 只读详细报告位于 /tmp/loppi-cloud-queue-investigation.md，不包含真实转录、音频、密钥或供应商响应正文。

## 验证与结束条件

- 最小 SSR 测试覆盖未知/未配/待确认/阻断/暂停/处理中/待处理/空闲及 SettingsView 两项透传；渲染不产生配置、恢复或其它请求。
- 运行 typecheck、lint 与新测试；若共享改动造成既有失败，记录具体原因，不改归属之外文件。
- 代码完成后停止写入，由根 Agent 将本改动与日期修正一起交一轮独立审查。

## 执行台账

- 2026-11-01：完成只读调查，获根 Agent 授权最小设置页修正；开始实施。
- 2026-11-01：CloudServiceSettings 改收既有 CloudQueue；SettingsView 对 llm/asr 透传各自快照。设置页显示未读/未配/待确认/余额阻断或其它阻断/暂停/处理中数/待处理数/等待任务；暂停及异常的辅助文字指向现有处理队列导航，没有新增按钮、路由、请求或写入。
- 2026-11-01：新增 SSR 测试 7/7 PASS（含父测试），覆盖两服务、阻断/暂停优先级、真实计数、SettingsView 透传、配置结果待确认保留“确认配置”及纯渲染不发请求。npm run typecheck PASS；npm run lint PASS（50 个源文件，无自动修复）。首次测试缺少合成 Tauri 环境标记，已修正测试夹具后重跑通过；生产配置行为未改。
- 2026-11-01：本子任务实现和最小验证完成，停止写入。状态保持 in-progress，根 Agent 负责合并日期改动后的独立审查、视觉/整体验证及提交安装，不把当前 SSR 验证冒充完整交付。
- 2026-11-01：根 Agent 纳入同次日期/本机更新交付，需求/故障/最终架构审查均PASS，完整11组交互与64组辅助页面检查通过；设置阻断状态复用既有琥珀Token。最终本机0.1.18 ARM预览（构建时间戳1793529791）已签名、正常替换与重启；真实设置页显示“已配置 · 1项处理中”/“已配置 · 等待任务”。两云恢复状态在重启后保留，LLM积压从72降至14且仍处理中、ASR也处理中，失败数为0。旧暂停的操作者与理由仍不可证明，未改写成确定结论。来源、安装与状态证明见日期更新台账；整体Windows/公开发布仍不在本项完成声明内。
