---
status: implemented
created: 2026-10-25
updated: 2026-10-25
---

# 将客户端本地粗转默认模型切换为已评测的 X-ASR

用户已确认选择 X-ASR，并要求客户端仓库记录评测集、评测结果与本次具体模型选型，方便以后升级。目标是让新构建的客户端通过既有模型准备流程加载固定 X-ASR，在相同生产输入上复现评测输出；历史结果保留原模型身份。此次交付是源码、固定下载/安装、真实推理验证、仓库文档与本地提交，不发布/安装包含其它未提交功能的客户端。

## 执行台账

### 开发契约

- 原子交付：固定模型包/身份、worker 默认识别、持久粗转标签、现有安装/加载验证、模型回归、评测与选型文档、三路独立审查和本地提交一起完成。
- 必须保证：采用五配置20事件评测中的 `sherpa-onnx-x-asr-zipformer-transducer-zh-en-int8-2026-06-03`，encoder/joiner INT8、decoder ONNX、原tokens；sherpa-onnx精确2.0.0、CPU/2线程、离线默认解码，不切流式或带标点权重。四个推理文件必须逐字节与冻结评测相同。
- 下载/安装复用 model_pack owner、SHA验证、磁盘预算、受限暂存、同步发布和显式PrepareModels。新包独立路径 `models/speech-xasr-v1`，旧 `speech-v1` 不删除、不当作新模型就绪；复用旧哈希有效 Silero及完整文件缓存；已安装用户按既有设置入口准备新包，不自动开麦或发云请求。
- worker沿用现有recognize命令与有界帧，Core/资源Governor/生命周期/协议/数据库schema保持原边界；只对本次新写的粗转记录使用准确 X-ASR model_version，旧记录不改、不强制历史重转。
- Silero VAD、CampPlus、采集/重采样、资源阈值和云流程保持现值。对新模型实际足迹核对现有Coarse预算，不因RSS与physical footprint口径不同擅自抬高预算。
- 状态来源：model_pack固定URL/哈希和installed_path为安装身份，Core既有ModelState/PrepareModels为准备状态，worker只计算；coarse_segments.model_version为每条已发布结果的模型归属。评测数字来自已冻结私有benchmark，不把摘要另造为识别真值。
- 复用：model_pack::{download,verify,check_space,copy_synced,clean_staging,install}，OfflineRecognizer/OfflineTransducerModelConfig，AnalysisClient，coarse::publish，scripts/verify.py、smoke_model_preparation.py、既有benchmark的framed runner和冻结输入。
- 新增架构机制：无。独立模型包版本是固定资产身份，不增加模型选择器、fallback、配置owner、数据库、队列、协议或自动恢复层。
- 隔离：Batch base `a498c65b01fc83e9663207544e4981e1c77a66a4`，使用已附加 /Users/ethan/.codex/worktrees/xasr-default/hear 验证。当前hear中既有0.1.6/摘要/界面等未提交改动均out-of-scope，初始diff/指纹保存在本机build/xasr-switch；只合入和暂存本任务补丁，两个共享文档只提交自己的差异。
- 文档：客户端README和技术索引提供持续入口；专文记录测试集位置/版本/规模/参考SHA、五配置结果/成本与不确定性、决策日期、完整权重/包SHA与来源、模型准备及安装边界。仓库只存脱敏数值/身份摘要，音频、正文、模型和运行仍在平级benchmark并保持原指纹。

### 验收与结束条件

- 固定HF提交的四文件哈希与benchmark一致，逐文件安装/校验可用，许可随包保存；新包不与旧SenseVoice混淆；错误/部分包不ready，旧VAD复用可实测。
- Rust check/test、必要仓库静态门禁和实际Core/worker构建通过；组合态既有失败只在有基线复现时记录，不修改其它session功能。
- 新生产worker完整682段原文字/状态与已选X-ASR基准零差异、零失败；VAD至少10事件对照不变，CampPlus真实加载/向量不变；实际物理足迹符合现有Governor上限。
- 隔离Core在新模型包上进入ready，旧包单独不被当作已准备，显式安装可复用旧VAD/完整文件暂存；不启动采集、调用云或改活跃用户数据。新增测试覆盖模型标签和旧包/缺失组件路径。
- 三路fresh requirements/adversarial、主Agent裁决后final architecture PASS；最终组合态检查与自有补丁提交完成。源码完成不声称安装版已生效；签名安装/长时/跨设备属既有发行边界，不新增用户mandatory验收。
- 修改识别配置/worker才重跑识别；修改安装器重跑安装/加载；纯文档/证据索引不重跑音频。参考/原始报告/模型不更改；达到以上门槛后收尾。

### 行动清单

- [x] 核对现有owner、共享改动边界、选中模型身份与官方来源。
- [x] 实现固定X-ASR包、worker和结果标签，更新相关烟测。
- [x] 完成静态/构建/安装/真实推理与保全回归。
- [x] 客户端内留存测试集、五模型结果、具体权重和选型文档及索引。
- [x] 三路审查、合入自有补丁、最终组合态验证、提交与状态收尾。

### Review基线

单批次，base见上。预计范围：model_pack.rs、hear-worker/main.rs、coarse.rs、两个既有模型烟测脚本、README与技术/架构文档、自有新评测摘要/台账/审查记录；其它共享变更全部排除。审查前按实际文件更新范围。

### 进展

- 2026-10-25：用户明确确认X-ASR；定位到现有SenseVoice安装/worker/写入标签路径，固定benchmark选择为2026-06-03离线非punct INT8，同名官方release包SHA与文件名已核实，随后还须比对实际文件。建立隔离验证工作树；先完成契约，再修改功能。

- 2026-10-25：同名官方tar实测encoder/decoder与冻结HF版本不同且无LICENSE，拒绝作为选中模型。安装源改为已测HF固定revision逐文件下载，沿用原download/SHA/暂存owner；LICENSE从作者固定提交下载并锁哈希，不引入新安装owner或协议。生产worker对682段原文/状态零差异、零失败，20事件VAD一致、CampPlus192维逐值相同，物理足迹峰值615678816字节，低于原640MiB预留。
- 2026-10-25：最终固定文件版本的隔离all14门禁、真实fixture逐文件损坏/缺失、三种Core准备路径通过；联网PrepareModels使用已验encoder缓存，其余四组件实际下载后ready。零缓存大文件残轮主动中断并保留，不冒充成功。自有补丁合入原工作区，16份外部未提交文件在逆向本次文档补丁后与初始SHA相同；只暂存自有差异。
- 2026-10-25：组合态构建/组件身份、Core加载、53前端/3素材/6Python通过；默认Rust仍有既有纯提示词样例失败，未修无关源/测试。其余Rust运行测试通过，构建结束后的串行doctest通过。组合态实际worker再次全跑682段，零差异/失败、20组VAD和192维声纹完全一致，物理足迹峰值622265184字节；当前进入独立审查。
- 2026-10-25：fresh requirements、adversarial和最终architecture三路均PASS，无范围内阻塞或必需证据缺口；主Agent裁决无需修复。功能和原始证据保持冻结，进入自有提交与最终组合态门禁收尾。
- 2026-10-25：实现提交`17549c4`已落入客户端；提交后组合态static（typecheck/lint/Rust check）全通过。再次只读校验63份本机证据、343份冻结benchmark文件、三份生产源码身份、16份外部文件保全及74个本地链接，全部通过；未重跑未受影响的音频。平级benchmark当前状态文档同步于`a2df014`，历史报告未改。隔离验证工作树已归档，所需忽略产物已保存在原工作区build/xasr-switch。三路审查和所有本轮结束条件完成，状态implemented；默认suite既有提示词失败仍按上文保留，安装/发布不在本次源码交付内。
