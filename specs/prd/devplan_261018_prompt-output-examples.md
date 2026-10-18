---
status: implemented
updated: 2026-10-18
---
# LLM 输出要求与 JSON 示例

用户检查全部产品 Prompt 后指出：JSON schema 应在输出要求处给出例子。本次仅将三组生产提示词统一为明确字段约束与合法 JSON 示例，便于模型理解和人工审阅。

## 执行台账
### 开发契约
- 必赢场景：事件整理、截图理解、Key 连通性验证各自的输出要求均紧邻合法 JSON 示例，字段类型与实际解析器一致，明确示例内容仅示意、实际值来自当前输入。
- 原子交付：三组提示词与必要格式验证一起交付。事件/截图现有业务行为与持久输出字段保持兼容。
- 必须保证：保留全部原有事件取舍、来源、时刻、摘要保真、合并边界与图片转录规则；示例覆盖空事件、null 摘要及无文字图片；Markdown 换行在 JSON 字符串中正确转义；验证 Prompt 仍只返回 ok=true。
- 明确不做：修改 provider 请求参数/模型/温度、引入 API json_schema 模式、改业务策略/队列/存储、重跑历史、发送真实用户数据做测试、此次重启用户正在运行的 App。
- 负责问题：提示词更改带来的输出形状与既有契约不一致、示例误导。
- 状态来源与生命周期：现有常量/调用点是 Prompt 来源，编译后供后续调用；不改变持久状态或任务版本，不因文案改动重跑历史。
- 复用：FIT_SYSTEM/FitOutput/fit_summary、IMAGE_SYSTEM/publish_image、cloud::verify_llm/chat/parse_chat_content。
- 新增架构机制：无。仅将验证 System 文本提取同模块常量以便直接核对示例，不新建 Prompt owner。
### 验收与结束条件
- 直接提取生产 Prompt 中示例，解析合法 JSON 并通过相关现有消费路径；字段、nullable/空结果和 Markdown 换行符合现行约束。
- Rust check、针对性测试、core build；无前端/原生 UI 变化，不新增 UI/OS 验收。完整 cross-review 三路 PASS 后提交并列出新增示例供用户审阅。
- 只跑受改动影响检查，不做真实云模型质量评测。真实模型是否更好遵循属于 advisory。
### 批次与行动清单
- Batch 1：三组 Prompt 输出说明与示例，一个内聚批次。
  - [x] 提示词与示例合法性/消费路径验证
  - [x] 静态/构建、cross-review、提交
### Review 基线
- Base: 1bd8805（动代码前 HEAD）；范围：crates/hear-core/src/{events,moments,cloud}.rs 和本 devplan。
- 其它工作树文件全部排除。
### 待用户决策
无。
### 进展日志
- 2026-10-18：确认事件目前使用伪 JSON 类型说明；图片缺少对象示例；Key 验证已有单行示例但未按输出要求归组。

- 2026-10-18：三个实际 Prompt 示例消费测试 PASS；cargo check/build PASS；不更改运行中的 App。新增示例保留合并/空事件/null 摘要/空转录差异。

- 2026-10-18：requirements / adversarial / architecture 均 PASS，无阻塞项；adversarial 独立复跑 3 个示例测试 PASS。三路审查后代码未改动，既有检查仍有效。

- 2026-10-18：提交 a22cef8。最终逐项核对原子契约完成，Prompt 原文与消费测试同一基线，未引入新 API 模式或运行状态；无必做真机项。仅源码及 core 开发构建更新，当前 App 未重启加载。
