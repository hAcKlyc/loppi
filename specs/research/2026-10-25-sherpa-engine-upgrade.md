# sherpa-onnx 2.0.0 升级与审查记录

需求源：[执行台账](../prd/devplan_261025_sherpa-engine-upgrade.md)。2026-10-25 官方 [Latest 稳定发行](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v2.0.0) 为 v2.0.0，published_at=2026-10-23T12:49:58Z，draft=false、prerelease=false。实际更新仅为 Core/worker 精确 crate 版本和必要锁文件依赖；上游 zip 2→9 及其传递依赖变化随 sherpa-onnx-sys 构建依赖引入，没有广泛执行 Cargo 更新。

前序同权重/同适配基准位于平级工程 `hear-asr-benchmark/reports/engine2-v2`：SenseVoice/X-ASR 各自 310 段原始文字/状态零变化，成本小幅变化不稳定。本次保留 SenseVoice、VAD、CampPlus 权重、模型安装与哈希、识别参数、资源治理和 worker 帧协议；不声称依赖升级提升准确率或省电。

## 验证与已知工作区边界

- 隔离工作树基线 `e9463dc`，`scripts/verify.py all` 的 14 个门禁通过，包含完整静态、Rust/前端/Python/native 回归、全工作区构建、四组件身份、隔离 Core 和诊断。
- 三个依赖文件的补丁合入原工作区；原 Cargo.lock 五个 workspace package 的 0.1.6 版本差异仍为用户的 unstaged 变更，原 Cargo.toml 等其它文件保持原状。
- 组合态静态检查通过。默认 Rust suite 有一项既有 `prompt_output_examples_follow_event_contract` 失败：当前提示词修改只匹配到一个 JSON，原测试期望两个。隔离旧 1.13.8 + 同一当前提示词复现相同 left=1/right=2。该函数仅解析内置提示词，与 sherpa 调用无关；本次不修改测试或提示词，也不把默认 all 记为全绿。
- 排除上述明确旧失败后，其余组合态 Rust 回归、Node/Python/native 回归、完整构建/身份、隔离 Core/诊断均通过。实际 Core 在隔离临时数据副本完成 VAD/SenseVoice 模型加载，状态 ready，无录音意图、采集、业务数据或云调用。
- Core 的真实 Opus 重建/逐块重采样回归由 `coarse::tests::recognition_uses_exact_vad_resampling_across_independent_opus_blocks` 覆盖；其它 coarse 和模型安装回归同时通过。
- 实际新构建的生产 worker 对原冻结 310 段全部识别成功，与 `runs/quality-v1/sensevoice-int8/predictions.jsonl` 的原始文字/状态零差异；10 事件各选一段的 VAD 输出与旧 worker 完全一致。CampPlus 同一段音频得到 192 维有限向量，最大逐值绝对差为 0。模型权重、冻结索引/参考、未改动的关键源码和 workspace manifest 共 10 个文件保全指纹一致。

### 可追溯验收快照

构建使用官方 arm64 静态发行库 `sherpa-onnx-v2.0.0-osx-arm64-static-lib`（归档 SHA-256：`5e5bdce1d7fc834857278d8bb26f878de6a55c7085e589ed1e41a00baca643ec`），通过 `SHERPA_ONNX_LIB_DIR` 复用前序基准已验证的 2.0.0 库。Cargo 解析的 sherpa-onnx 和 sherpa-onnx-sys 均只有 2.0.0。两版静态包使用同一 ORT 1.28.2，本次不把 ORT 算作升级变量。

| 本次候选/运行产物 | SHA-256 |
| --- | --- |
| staged Cargo.lock（不含其它 session 的 0.1.6 变更） | `ab699d36f49d6ec58fcb540f08501ca278bc27744ad89be72b6162190b970c69` |
| crates/loppi-core/Cargo.toml | `90fa358eed2ca216d0de268432dc51394cba80b719791da2f2b01d37c6756944` |
| crates/loppi-worker/Cargo.toml | `7abb307328f5f5d83a3792b69639d439e6eb724af06c5c0602047e5f6b9faf4a` |
| 完成 310 段/VAD/CampPlus 验证的 worker | `d17be2d6c03ee46b18fabd47d96b02bb4f5ebc66a88f9e40fe3c3efb5ea94d26` |

本机 `build/sherpa-upgrade/` 的证据：`verification-isolated.json`（all、14 项通过）；`verification-combined.json`（如实记录默认 all 在既有提示词测试失败）；`preexisting-prompt-old-engine.log`（旧引擎复现）；`verification-combined-remaining.json` 与 `verification-combined-smoke.json`（其余回归/构建通过）；`model-load.log`（真实 Core 加载 ready）；`production-worker/summary.json`（310/10/192 结果及保全检查）。组合态运行二进制带工作区既有 0.1.6 版本，干净隔离构建验证本次提交的 0.1.5 基线；两种构建均使用同一精确引擎依赖。

原始机器报告与模型验证保留在本机 ignored `build/sherpa-upgrade/`；不提交录音、逐字输出、向量、数据库或模型。当前安装版未重装，源码升级随后续完整打包安装生效。

## Review Contract

- mode：repair；base：`e9463dc`。
- in-scope：staged Cargo.lock 的依赖差异、crates/loppi-core/Cargo.toml、crates/loppi-worker/Cargo.toml、该 devplan 与本记录。
- out-of-scope：共享工作树全部其它 tracked/untracked 变更，尤其 0.1.6 版本与提示词/摘要修改；安装/发布、X-ASR 替换、模型训练或权重更新。
- 必须保证：统一解析 sherpa-onnx / sherpa-onnx-sys 2.0.0、现有推理 API/模型和状态 owner 不变、隔离验证、本任务补丁不覆盖或提交其它工作；原模型/参考保全。
- 允许新增机制：无；沿用现有 crate、模型管理、worker 协议和验证工具。
- in-scope defects：当前依赖升级引入或显著暴露的可达构建、推理或锁问题。已复现的提示词失败为 pre-existing，不能驱动修改无关功能或提高保证。
- required lenses：fresh-context requirements / adversarial，主 Agent 裁决后 fresh-context architecture；reviewer 只读，不再派 reviewer。
- 检查：锁/依赖图、干净基线全套门禁、组合态已知失败裁决与其余回归、实际生产 worker 冻结音频对照、VAD/声纹/重采样、本任务范围与保全证据。代码修复只令直接相关证据失效，不机械重复未受影响运行。
- 结束：范围内阻塞关闭、全部必需证据完成、最终 architecture PASS 后本地提交与状态收尾；没有用户人工听校或新增安装验收要求。

## 审查结果

实际模型对照及保全验证完成；候选为 Git index 中的五文件差异。Phase 1 期间候选保持冻结，三处功能文件指纹与上表一致。

- Requirements：fresh-context `prod_sherpa_requirements`，PASS，无阻塞或必要证据缺口。核对精确依赖/构建依赖闭包、最新稳定版、310/10/192 及模型保全证据，确认五个既有 workspace 版本差异未暂存。
- Adversarial：fresh-context `prod_sherpa_adversarial`，PASS，无阻塞。核对输入/VAD 会话与释放、识别/声纹包装、Core 重采样及进程边界；上游 VAD 起点扩为 i64 与既有转换兼容。
- 主 Agent 裁决：两路均无修复项；旧引擎同样复现的提示词测试失败为 pre-existing / out-of-scope。无需更改功能候选或使已有验证失效。
- Architecture：Phase 1 后 fresh-context `prod_sherpa_architecture`，PASS。最终候选只更新既有依赖，锁差异全部落在 sherpa / zip 构建依赖闭包；无新增 owner、状态、协议或兼容路径，无需调整架构/算法文档。

## 最终收尾

实现提交 `02013f9`，三路 required lens 全部 PASS、无修复项。提交后 `verification-final-static.json` 的 typecheck/lint/rust-check 三项通过；逐项重读执行契约，确认最终 HEAD 三处功能文件与审查快照相同、已验收 worker 哈希未变，前述实际模型回归仍有效。既有工作区版本/功能变更保留为 unstaged；本次没有安装或发布客户端。临时验证脚本归档为本机 ignored `production-worker/validation_recipe.py`，不提交音频或识别结果。全部本次验收已完成，后续完整打包安装时应用新引擎。
