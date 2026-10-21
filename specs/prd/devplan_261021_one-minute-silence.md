---
status: implemented
updated: 2026-10-21
---
# 精转已知静默连接上限改为一分钟

> 后续变更（2026-10-22）：本文保留当时的交付契约与测试事实。现行文字发布已删除原声音频覆盖/纯静默拒绝规则，按用户确认优先保留完整文字；当前行为见 [精转发布修复计划](devplan_261022_asr-publication.md)与 [算法文档](../tech_docs/algorithms.md)。

用户要求将上一轮三分钟无人声容错改为一分钟。语义仍为连接同事件相邻保留音频之间、已有完整同源无人声证据的缺口：≤60 秒按真实时长填静默，>60 秒拆请求。不额外添加一分钟尾巴，不压缩时间，不裁剪仍保留的媒体内部静默。

## 执行台账

### 开发契约
- 原子范围：ASR planner 上限、策略版本、既有边界/编码回归和当前算法文档一次完成。
- 必须保证：60 秒可连接、60 秒加 1ms 拆开；未知/采集/清理缺口规则不变；上传真实时间与映射不变，已持久输入和远端 ID 不重写。
- 不做：VAD/原声留存、事件粒度、尾部填充、历史自动重跑、模型/prompt 调整、安装或重启。
- 状态来源/复用：Core `asr::plan` / `confirmed_silence` 读取 audio_blocks 保留事实、media_gaps、clear_ranges，复用不可变 asr_inputs 和原编码/映射；新增机制无。
- 本次负责的问题：仅一分钟阈值调整引入或放大的缺陷。
- 架构不变量：Core 唯一事实 owner；真实时间不压缩；已有物理请求不可变。

### 验收与结束条件
- mandatory：既有 ASR 回归覆盖 60 秒两侧、真实 Ogg 时长/静默/后段映射/幻觉拒绝、旧输入不变；cargo check/build；三路独立审查；提交后最终静态检查。
- 修改阈值影响 planner 与编码场景，重跑 ASR 回归；prompt 未变，不重新付费运行模型；文档变更只检查内容和 diff。
- advisory：实际语音准确率/能耗/费用，不由阈值推断。
- 三路 PASS、验证与 scoped 提交完成后结束，不扩大范围。

### 行动清单
- [x] 调整阈值与版本、既有测试、当前算法文档。
- [x] 验证、review、实现提交和最终门禁。

### Review Contract
- mode: repair；required lenses: requirements、adversarial、architecture。
- base: `0b69be7617c90a8efb4ce897e5137294a14ffa7e`。
- requirement source: 本计划与用户最新要求；保证、非目标、允许机制见上文。
- in-scope: `crates/hear-core/src/events/asr.rs`、`crates/hear-core/src/events/asr/tests.rs`、`specs/tech_docs/algorithms.md`、本计划。
- out-of-scope: 上一轮已交付逻辑、其他所有共享工作树修改。旧计划作为历史交付记录保留。
- 结束门槛及证据影响见上文。只审本次差异，不回卷上一轮。

### 待用户决策
无。已解释一分钟是连接上限，不是每个请求附加的尾巴。

### 进展
- 2026-10-21：确认原上限 180 秒，复用现有策略入口；准备只改为 60 秒并更新当前文档和既有回归。

- 2026-10-21：阈值改为 60 秒，策略版本 v4；既有边界回归覆盖 60000/60001/180000ms，实际编码 5s+60s+5s=70s 并核验后段映射及纯静默虚构文字拒绝。ASR 20 PASS / 1 paid probe ignored；check/build 和 diff check PASS。进入三路审查，代码冻结。

- 2026-10-21：requirements/adversarial/architecture 三路独立审查均 PASS，无阻塞发现或修复；证据齐备，进入作用域提交。

- 2026-10-21：实现提交 `a7c004e`；重新核对完整契约及非目标，最终 HEAD check PASS，单批次审查覆盖最终代码，无未决 mandatory 项。安装版未替换，当前录制未重启；所有无关共享工作树修改保留。
