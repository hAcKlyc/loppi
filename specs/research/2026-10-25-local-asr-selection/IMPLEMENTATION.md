# X-ASR客户端采用与验收记录

需求：[执行台账](../../prd/devplan_261025_xasr-model-selection.md)。持续入口：[测试集、评测结果与具体选型](../../tech_docs/local-asr-benchmark.md)。2026-10-25用户明确确认采用X-ASR；本记录只描述本轮源码实现和实测，不推断安装版已经更新。

## 实现

- `model_pack`继续拥有下载、SHA、磁盘预算、暂存与发布。新包`speech-xasr-v1`包含已测HF固定revision的四个推理文件和作者固定提交LICENSE；每个文件均锁SHA/大小，沿用原download，不增加依赖或安装owner。移除本模块旧SenseVoice专用归档解包路径，不用同名但字节不同的官方tar。
- 原`speech-v1`保留；其哈希有效Silero可复用，不能当新ASR就绪。部分文件失败不发布，完整单文件可复用；旧下载无字节续传。Core仍经原PrepareModels/ModelState加载，不自动开麦或调用云端。
- `hear-worker recognize`改用离线Transducer、CPU/2线程、原有有界帧；VAD、CampPlus、重采样、Governor和IPC/schema不变。新`coarse_segments.model_version`为`x-asr-zipformer-transducer-zh-en-int8-2026-06-03`，历史记录不改、不重跑。

## 验证事实

- 干净基线`a498c65`隔离工作树的最终`verification-pinned-files.json`：`scripts/verify.py all`全部14门禁通过，包含构建、静态、Rust/前端/Python/native回归、组件身份与隔离Core/诊断。
- 真实固定资产的显式fixture测试通过：五份文件（含LICENSE）各自缺失/损坏均不ready；完整单文件暂存与旧VAD可安装，全程不下载、不删除旧包。
- Core模型烟测的完整包、split组件、旧VAD＋新文件缓存三种路径均到ready；旧包单独时为missing。未产生录音意图、采集、业务数据或云请求。
- 联网PrepareModels测试到ready：encoder借用已校验缓存，decoder/joiner/tokens/LICENSE实际下载，六文件（含VAD）全部SHA与固定值一致。首次零缓存大encoder下载为避免重复主动中断，残轮保留；**不声称已完成零缓存全量下载测试**，也不把中断轮计为成功。原始记录在本机`build/xasr-switch/network-*.json/log`。
- 实际新生产worker已完整尝试682段、零失败，原始文字/状态与`quality-selection20-v1/xasr-sherpa200`零差异；20事件VAD输出不变，CampPlus192维逐值相同。隔离worker物理足迹峰值615678816字节，低于既有640MiB预留/768MiB回收水位；未按RSS擅自抬高预算。
- 组合态最终worker再次完整682段，原文/状态零差异、零失败，20事件VAD与192维声纹完全一致，物理足迹峰值622265184字节，同样低于原预算。实际验收worker SHA256为 `8c7d0a7011a7be90d3d8d9dee4ab184ba7e56bb846ef5e7b63c8cf0d6a1e503a`；该二进制副本和全部原生输出/采样在本机`production-worker-combined`保留。
- 合回原客户端后，静态检查、完整构建/组件身份和隔离Core/诊断通过，实际新Core加载X-ASR到ready。共享默认Rust suite仍在既有`prompt_output_examples_follow_event_contract`失败（335通过、1失败、16忽略）；该纯提示词测试和提示词均非本次修改，前序旧引擎复现见[引擎升级记录](../2026-10-25-sherpa-engine-upgrade.md)。不将默认全套记为全绿。排除该已知例后Rust运行测试通过；一次并发构建期间的文档测试缺少临时rlib，构建结束后串行doctest通过。前端53项、素材3项、Python6项通过。
- 初始16份共享未提交文件按原SHA保全；两个共享技术文档只合入/暂存本次ASR文字。0.1.6版本、摘要/提示词/界面等外部改动保持原状，未纳入本轮提交。

完整私有证据在原客户端`build/xasr-switch/`，含固定源/模型身份、原生帧/预测/资源、校验配方、联网与缓存烟测、共享保全及机器回执。没有把音频、逐字输出、数据库、向量或权重放进Git。

机器回执：[acceptance.json](acceptance.json)，绑定本机artifacts-manifest的SHA；该清单包含校验配方、实际原始回执、初始保全基准和实际验收程序身份。原有benchmark正式343项也按冻结清单全量校验，未改写原始分数或答案。

## Review Contract

- Mode：repair；Batch base：`a498c65b01fc83e9663207544e4981e1c77a66a4`，单批次。目标是原客户端Git index的自有候选，以及本页明确引用的私有证据；不能把共享工作树其它差异一起审成当前改动。
- Requirement source：执行台账的完整原子契约及用户“更换X-ASR，并在客户端留存评测集、结果和具体选型”的要求。
- In-scope：README、model_pack.rs、coarse.rs、hear-worker/main.rs、smoke_model_preparation.py、verify_ingestion_quality.py、ARCHITECTURE/tech_docs的自有ASR差异、devplan、本目录和local-asr-benchmark.md。审查前以显式staged文件清单冻结。
- Out-of-scope：所有其它tracked/untracked变更，尤其0.1.6/摘要/提示词/UI；安装/发布、更多候选、旧参考重写、历史付费重跑、长期/跨设备保证。
- 必须保证：准确采用已测四文件与固定LICENSE、旧包不误ready且保留、复用既有Core安装/资源/worker边界、新结果准确模型归属、682输出复现、VAD/声纹/分段/预算不变、文档和数值可追溯、无私人正文入库。
- 允许新增机制：无owner/store/协议/队列/模型选择器/fallback/恢复层；新包版本只是固定资产身份，安装逐文件复用现有download/SHA/暂存。接受银标准同源偏向、目的样本、主机波动和进程能量限制，不提高为人工真值/电池续航保证。
- In-scope defects：本轮引入、显著放大或明确拥有的可达违反保证问题；已复现的提示词失败、外部UI/版本变更不驱动修复或扩张。
- Required lenses：fresh requirements/adversarial并行，主Agent裁决和最小修复后fresh architecture；只读reviewer不编辑/不派生reviewer，Phase1期间候选冻结。改安装器仅重跑安装/加载，改识别配置才重跑识别，纯文档/身份绑定不重跑ASR。
- 结束：所有范围内阻塞/必要证据缺口关闭，受影响检查通过、最终architecture PASS，源和实测身份对应，自有提交及最终台账收尾。无需用户听校或本次签名安装验收。

## 审查与收尾

- Requirements：fresh-context `xasr_requirements`，PASS，无阻塞/必要证据缺口。独立核对63份本机证据、343份benchmark冻结身份、两套682原生帧、源码/index对应、五配置摘要逐值一致及16份外部文件保全。
- Adversarial：fresh-context `xasr_adversarial`，PASS，无阻塞。追踪安装/失败/旧包/单owner/资源/发布链路，独立核对原生帧与实际二进制身份并重算两套物理足迹，所述保证成立。
- 主Agent裁决：两路无修复项；既有提示词失败、网络零缓存残轮和安装边界已如实记录，不提高保证或重跑未受影响ASR。功能代码、原始运行与证据清单保持冻结。
- Final architecture：fresh-context `xasr_architecture`，PASS，无范围内架构阻塞。确认模型准备、资源准入、worker计算和结果持久化沿用既有owner，固定资产身份集中于model_pack；未新增协议、选择器或恢复层。独立复核63项证据与三份生产源码身份，版本及文档义务齐备。
- 三路审查均通过，无需修复候选；进入自有提交与最终组合态门禁收尾，原评测报告保持冻结。

## 完成交付

客户端实现提交`17549c4`；提交后的`build/xasr-switch/verification-final-static.json`中typecheck、lint与Rust check全部通过。只读最终核验回执`build/xasr-switch/final-verification.json`确认63份本机证据、343份冻结benchmark文件、三份生产源码、16份外部文件保全与74个本地链接全部匹配；没有改写原始manifest或重跑未受影响的ASR。已完成整份台账的结束条件核对，本次状态为implemented。

平级benchmark的README与当前产品状态同步于提交`a2df014`，记载用户已采用本次固定X-ASR；历史报告仍为原快照。隔离工作树已归档，所需忽略证据和验收程序副本已保存在原工作区。客户端源码切换完成；既有安装版未重装，后续完整打包安装并通过原模型准备入口准备新包后生效。既有提示词测试失败按上文保留，本轮不宣称组合默认全套全绿。
