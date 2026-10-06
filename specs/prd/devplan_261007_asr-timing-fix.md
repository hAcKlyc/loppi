---
status: implemented
updated: 2026-10-07
---

# 精转时间映射失败修复

用户要求修复最近大量「处理失败 · 云端服务异常」。只读诊断显示 14 个 provider_unmapped_timing 和 1 个 provider_invalid_utterance；当前不保存返回内容，必须先用既有远端请求查询确认拒绝条件，不能把猜测当根因。

## 执行台账

### 开发契约
- 必赢场景：合法且有原声音频依据的识别结果能发布，不因编码静音或合理句段边界误判而整任务失败；真实缺失音频、越界、无音频依据仍拒绝。队列明确显示映射/格式问题。
- 原子交付：处理逻辑、错误提示、回归测试、文档及独立审查一次完成；持久任务及既有原声兼容。
- 必须保证：墙钟映射不变，事务原子性不变；不记录文本/密钥/响应正文；先查既有远端结果而不重送付费音频。
- 明确不做：自动重送历史失败任务、修改原数据、扩大云上传范围、增加新队列/存储/时钟或恢复协议。
- 本次负责的问题：时间校验和供应商分句解析导致的合法响应拒绝、错误标签误归服务异常。
- 状态来源与生命周期：cloud_tasks 与 audio_blocks 是持久事实；供应商返回相对毫秒在 publish_asr 映射，核心是唯一写入者；QueueView 只展示。
- 复用：query_audio、publish_asr、utterance_has_encoded_audio、assemble_ogg、taskStatus、既有重试语义。
- 新增架构机制：无。
- 架构不变量：真实 UTC 毫秒轴、缺口不伪造原声、云端凭据只由核心读取，日志不写内容。

### 验收与结束条件
- 查既有响应并明确失败原因；对已确认原因补回归，同时保留真实 media_gap / 无覆盖 / 越界拒绝。
- Rust 与前端静态检查、针对测试、构建；真实队列文案烟测。
- 三路独立审查通过后结束。原工作树依赖若阻止独立提交，保留明确基线差异，不能提交无关变更。

### 开发批次与行动清单
- Batch 1
  - [x] 查询已有远端任务并确定根因
  - [x] 修复与回归测试
  - [x] 静态检查、构建与烟测
  - [x] requirements / adversarial / architecture 审查
  - [x] 隔离暂存仅本次修复，完成提交前验证

### 当前批次 Review 基线
- HEAD: 178213144487542cd326f1cf03f4ceeb10413f50
- 工作树基线：/tmp/hear-asr-fix-baseline（events/cloud/main/WorkspaceViews/algorithms）
- 预期范围：上述处理及提示代码、必要测试/文档；兔子和 native 未提交改动均不在范围。

### 已确认根因与实现范围
- 只读查询已有15个远端响应：最新请求把 26180–29795、97720–99160、370635–370755 合成一句；更多旧响应把首字拉长到几十秒甚至5分钟；另一个供应商响应含大量零时长重复句，仍应拒绝。
- 根因 owner 在 queue_asr_with_processor：只有15分钟上限，把稀疏原声之间的长静音也拼入请求。改为在超过既有3秒可映射静音上限时分开物理任务（复用既有分窗/多任务事件）。短暂停顿和墙钟起点保持。
- 适配器对完整可对齐的词级元数据按>3秒停顿拆句，帮助既有响应恢复；不猜词时刻、不放松音频覆盖校验、不接受零时长重复。
- 持久任务兼容：既有远端ID、自动恢复和手动重试查询优先语义不变；历史任务不自动重排/重送。

### 待用户决策
无。

### 进展日志
- 2026-10-07：创建基线；生产库仅只读，不自动重送请求。

- 2026-10-07：输入分窗与响应规范化完成；Rust121、TS/lint、队列copy2、核心check/debugbuild、前端build通过；signed DEBUG bundle与strict codesign通过。原队列实际显示映射/整理边界具体错误，截图playground/hear-asr-queue-fixed.png；未点击重试或发起submit。
- 2026-10-07：requirements/adversarial发现规范化可能隐藏原始越界包络，确认为本次阻塞。保留首片original start与尾片original end，新增parser->publisher越界/真实gap及事务回滚回归，两个原reviewer定向复查PASS；fresh architecture最终PASS。
- 本机加载修复版后台：原暂停revision26不变；重启后既有媒体扫描需要时间。为避免debug后台开销，另构建release核心搭配DEBUG GUI；完成在线验证后关闭台账。

- 2026-10-07：release核心/worker/CLI构建完成，搭配DEBUG GUI重新签名并strict验证；本机后台已加载新二进制，状态读取成功仍为paused_intent=true、revision26、双路idle；首页仍显示「已暂停」。云队列原计数15 ASR/4 LLM失败保持，未自动重试/重送。
- 提交前从index导出独立快照，TS/lint/队列copy2均PASS；WorkspaceViews仅暂存本次映射函数提取，未纳入此前rabbit/settings/入口改动。core/cloud/events/algorithms基线均确认与HEAD相同。
- 整体门禁：根因证据、输入/输出回归、严格拒绝与事务测试、类型/lint/build、native copy/状态、三路审查全部覆盖。没有新状态/存储/重试机制；无必做未完成验收。旧供应商首字拉长或零时长重复响应属于明确保留的拒绝边界，需要用户在事件内显式重新精转，新分窗策略才会重建付费请求。
- 普通观察项advisory：下次用户正常录制时观察新的短窗口识别质量与请求数；本次没有为测试重新上传个人音频。供应商返回词级字段事实参考官方查询文档：https://docs.volcengine.com/docs/DoubaoVoice/result-query-http-1?lang=zh
