---
status: implemented
created: 2026-10-25
updated: 2026-10-25
---

# 升级 sherpa-onnx 至稳定版 2.0.0

## 已接受的需求

用户明确确认升级引擎，并要求核实最新稳定版本。2026-10-25 官方 GitHub Latest API 返回 v2.0.0，2026-10-23 发布，draft/prerelease 均为 false。将项目 hear-core、hear-worker 的精确依赖统一从 1.13.8 升至 2.0.0，保留现有 SenseVoice、Silero VAD、CampPlus 权重与全部识别/分段/资源设置。前序独立 10 事件基准表明文字完全一致，不能把本次依赖升级宣传成准确率或稳定资源改善。

## 执行台账

### 开发契约
- 原子交付：版本确认、两个 Rust 依赖与锁更新、真实构建/本地推理回归、独立审查、必要文档及本地提交一起完成。
- 必须保证：不存在两套 sherpa 主版本；不改变模型文件、安装源/哈希、模型标签、worker 协议或业务状态 owner；VAD、粗转、重采样、声纹路径保持可用。
- 范围：crates/hear-core/Cargo.toml、crates/hear-worker/Cargo.toml、Cargo.lock 的本次依赖差异及本台账/审查证据。共享工作区既有 0.1.6 版本和其它功能改动不属于本次，不覆盖、不暂存、不提交。
- 隔离：以 e9463dc 为干净构建/审查基线建立独立工作树；最终仅合入本任务补丁到原工作区，组合态验证后显式提交自有差异。生产库只归 Core，模型验证使用隔离数据副本，不访问活跃业务库或云端。
- 复用：既有官方 Rust crate 与原生静态库；hear-worker vad/recognize/embed；Core LinearResampler；scripts/verify.py、smoke_model_preparation.py；独立 benchmark 冻结音频/答案及帧 runner。
- 新增架构机制：无；不增加 owner、协议、队列、fallback 或恢复路径。
- 不做：不替换 X-ASR，不更新模型权重，不调参，不重写历史结果，不发布或安装包含其他未提交功能的客户端。安装版生效随后续完整构建安装，不能把源码变更称为已运行安装版。

### 验收与结束条件
- 官方 Latest 稳定状态可追溯；Cargo 仅解析 sherpa-onnx / sherpa-onnx-sys 2.0.0；锁文件只变更必须的依赖。
- Rust check/test 和实际 worker/core 构建通过；静态门禁按仓库要求完成，失败先区分当前变更与既有工作区问题，不修无关功能。
- 实际生产 worker 在冻结 310 输入上的文字/状态与原基线比较；隔离 Core 模型加载、实际 VAD/重采样与 CampPlus 输出至少做直接烟测，不读取凭据或调用云端。
- 全部模型与原 benchmark 保全指纹一致；required fresh-context requirements/adversarial/architecture PASS 后提交。
- 单批次最终整体门禁通过后标 implemented；无需用户人工听校。硬件采集、跨设备、长跑、公证安装属于未改变的发行边界，不新增为本次用户验收。

### 行动清单
- [x] 核实最新稳定版及当前依赖、共享工作区边界。
- [x] 隔离更新两处依赖与锁，完成构建及静态/回归验证。
- [x] 完成真实模型/冻结数据对照和原模型保全。
- [x] 独立审查、合入自有补丁、组合态验证、提交及状态收尾。

### 进展
- 2026-10-25：用户确认升级。官方 Latest 为 v2.0.0（稳定），没有把 master 的后续未发行提交当成稳定版。生产工作区存在其他功能与 0.1.6 版本变更，已记录边界，建立隔离验证工作树。
- 2026-10-25：隔离基线 e9463dc 上两处精确依赖及必要锁更新完成，scripts/verify.py all 的 14 个门禁全部通过。仅将本任务三文件补丁合回共享工作区，Cargo.lock 原有五处 0.1.6 版本差异保持 unstaged，未暂存其它功能。
- 2026-10-25：组合态默认 Rust suite 出现既有 prompt_output_examples_follow_event_contract 失败（要求两个 JSON 示例，当前独立修改的提示词只匹配一个）；在隔离 1.13.8 依赖下配合同一提示词复现相同 left=1/right=2，确认与语音引擎无关。记录为 pre-existing / out-of-scope，不修提示词、不宣称默认 all 全绿；其余组合态回归和构建烟测继续执行。
- 2026-10-25：组合态其余 Rust/Node/Python/native 回归、完整构建、组件身份与隔离烟测全部通过；实际 Core 在临时模型副本上进入 ready。生产 worker 的冻结 310 段全部成功、原文/状态零差异；10 组 VAD 输出一致，CampPlus 192 维向量逐值一致，真实 Opus 分块重采样回归通过。模型、源数据/参考及关键未改源码共 10 个文件的保全指纹一致。
- 2026-10-25：固定五文件候选完成 fresh-context requirements / adversarial 审查，均 PASS、零阻塞；主 Agent 无修复项裁决后，最终 architecture PASS。两处精确依赖、锁与模型证据对应，无新增架构机制；仅剩本地提交和最终状态收尾。
- 2026-10-25：实现提交 `02013f9`。重读整份契约并完成最终组合态静态门禁：typecheck、lint、cargo check 全部通过；HEAD 的三处功能文件指纹与审查快照一致，实际验收 worker 指纹未变，已有 310/VAD/声纹/重采样结果继续有效。单批次无新增运行时状态或兼容残留，无未覆盖的本次 mandatory 验收项。保留已复现的既有提示词测试失败和全部非自有变更；安装版仍待下一次完整打包安装生效。验证材料保存在本机 ignored build/sherpa-upgrade/，临时脚本和缓存软链接已清理。
