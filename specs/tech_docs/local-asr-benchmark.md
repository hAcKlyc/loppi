# 本地粗转：评测集、评测结果与当前选型

更新：2026-10-25。**用户确认默认粗转采用 X-ASR 中英离线 Zipformer Transducer INT8，2026-06-03 权重，sherpa-onnx 2.0.0、CPU/2线程。** 本页是客户端仓库的持续入口；原始数据、参考和运行保存在平级 `hear-asr-benchmark`。源码生效与安装版生效分别记录，不由源码推断当前运行的 `.app` 已更新。

## 评测集在哪里

- 工程：`<USER_HOME>/Projects/hear-asr-benchmark`，与客户端仓库平级。相对入口：工程说明（本地材料 `hear-asr-benchmark/README.md`，不随仓库发布）、方法（本地材料 `hear-asr-benchmark/docs/METHOD.md`，不随仓库发布）、复现（本地材料 `hear-asr-benchmark/docs/REPRODUCE.md`，不随仓库发布）。
- 当前冻结输入：`data/v2`，原有10事件＋工作电脑新增10事件，**682段/81.36分钟、1248份原始Opus**。工作集来自4个工作日的独立活动；按实际场景/工作价值兼顾难例，未按新模型成绩选样。
- 答案：`data/v2/reference-v1`，**626段/79.19分钟计分，56段不计准确率但保留输出**。每例保留可播放音频、TXT、原始精转、修订及来源，见音频与答案索引（本地材料 `hear-asr-benchmark/data/v2/INDEX.md`，不随仓库发布）。生产VAD分段和逐块重采样固定，对所有模型相同；未测试端到端VAD召回。
- 参考是既有精转经FireRed AED与Qwen1.7B自动交叉核验的银标准，不是人工金标准。Qwen参与核验，故并列未修订精转敏感性；w02/w10等弱参考与部分时间覆盖另列。原始10例及旧正式报告保持冻结。
- 本仓库的[数值摘要](../research/2026-10-25-local-asr-selection/results-summary.json)记录输入、参考、完整结果和正式证据清单SHA256；不复制私人音频、转录正文、数据库或权重。恢复完整测试集需备份整个benchmark目录，只有客户端Git不够。

## 最终五配置实测

报告版本：`reports/selection20-v1`；benchmark提交 `aff662d`。五配置在新排名前根据旧13配置中文CER预选，全部CPU/2线程；**3410次全量质量尝试、720次串行资源尝试、10次合成控制**。资源为每事件两段、共40段三轮，最后追加同样SenseVoice锚点；并行质量耗时不用作成本排名。

中文为19例按参考字符数加权的CER，越低越好；英文WER仅来自原c10一例，工作集不是新增独立英文验证。不能将100%-CER直接称为准确率。

| 配置 | 整体中文CER | 原有中文CER | 工作中文CER | 未修订精转CER（整体） | 英文WER | 质量失败/682 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| SenseVoice INT8 | 22.18% | 26.00% | 20.93% | 23.42% | 24.35% | 0 |
| **X-ASR INT8** | **21.17%** | **25.20%** | **19.85%** | **23.20%** | **13.00%** | **0** |
| Qwen3-ASR 0.6B Q8 | 21.03% | 24.69% | 19.83% | 22.85% | 14.22% | 0 |
| Qwen3-ASR 1.7B Q8 | 18.40% | 23.14% | 16.85% | 21.34% | 10.12% | 1 |
| Paraformer INT8 | 22.55% | 27.29% | 21.00% | 23.81% | 51.75% | 0 |

本机Apple M5/24GiB，MB为十进制。以下均为同一新资源轮次；不能混入原10例的资源数字。

| 配置 | 模型资产MB | RSS峰值MB | 物理足迹峰值MB | 热RTF | CPU秒/输入分钟 | 进程估算J/输入分钟 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| SenseVoice INT8 | 239.5 | 675.9 | 651.7 | 0.083 | 9.90 | 20.38 |
| **X-ASR INT8** | **175.7** | **959.2** | **628.3** | **0.034** | **4.33** | **7.36** |
| Qwen3-ASR 0.6B Q8 | 850.4 | 1476.6 | 632.7 | 0.326 | 36.79 | 34.36 |
| Qwen3-ASR 1.7B Q8 | 2185.0 | 2812.6 | 631.1 | 0.521 | 60.48 | 96.74 |
| Paraformer INT8 | 227.4 | 614.9 | 600.0 | 0.059 | 6.94 | 15.32 |

RTF为处理秒/音频秒；RSS和macOS物理足迹口径不同，干净映射权重页可回收，峰值不是未来上限。线程参数2不是核硬绑定；X-ASR实际平均约2.11核。模型资产不是最终安装包；本轮原生程序/必要库X-ASR约26.3MB，产品UI和共同组件另计。进程能量是macOS同机估算，不能换算电池续航或承诺省电比例。

SenseVoice首末RTF变化+7.46%、CPU+2.88%、进程能量+3.24%；Qwen1.7B三轮RTF为0.598/0.510/0.454。后台负载和温度不可完全隔离，不用小差距作稳定领先结论。完整冷启动、P95、控制、逐例结果和证据见最终评估（本地材料 `hear-asr-benchmark/reports/selection20-v1/DECISION.md`，不随仓库发布）、完整成本表（本地材料 `hear-asr-benchmark/reports/selection20-v1/REPORT.md`，不随仓库发布）、分层及参考QA（本地材料 `hear-asr-benchmark/reports/selection20-v1/COHORTS.md`，不随仓库发布）。

## 为什么选择X-ASR

用于持续后台粗转，X-ASR相比SenseVoice：整体中文CER下降约1.01个百分点（相对4.6%）、英文WER从24.35%降至13.00%；CPU开销约为0.44倍、热处理约快2.44倍、模型资产少26.6%。代价是RSS峰值多约283MB，新进程到首结果1.91秒，原SenseVoice为0.49秒。

Qwen1.7B准确率更好，但CPU约为SenseVoice的6.1倍、X-ASR的14倍，模型资产约2.19GB。其整体CER相对改善在修订参考下约17%，未修订精转下约8.9%；不能忽略同源参考偏向。Qwen0.6B与X-ASR工作CER仅差0.02个百分点，CPU却约为X-ASR的8.5倍；逐一移除中文事件时两者第2/3名互换。Paraformer没有整体准确率升级收益。

X-ASR也有回退：c08为48.27%（SenseVoice37.13%）、w10为53.29%（51.56%）；工作否定词错误36/270，基线34/270。预置术语匹配较强，但自动关键项不代表完整语义准确率。两个静音/低幅噪声控制中X-ASR为空，不足以估计真实幻觉概率。当前只验证中英场景，不延续SenseVoice其它语言的覆盖假设。

这是用户本轮明确采用的工程取舍，不是“模型已无提升空间”或模型家族上限；分段、量化和运行时都影响表现。以后更新模型或策略，须使用同集同参考重测，再决定是否采用。

## 具体采用的模型

- 作者项目：[Gilgamesh-J/X-ASR](https://github.com/Gilgamesh-J/X-ASR)。转换模型卡指向该项目；[上游许可](https://github.com/Gilgamesh-J/X-ASR/blob/main/LICENSE)为Apache-2.0，安装保留包内LICENSE。
- 转换仓库：`csukuangfj2/sherpa-onnx-x-asr-zipformer-transducer-zh-en-int8-2026-06-03`，固定revision `bbc3ebbf8f134848f804967f3a3b69f981308a85`；[转换模型卡](https://huggingface.co/csukuangfj2/sherpa-onnx-x-asr-zipformer-transducer-zh-en-int8-2026-06-03)。
- 安装源：上述Hugging Face固定revision下的四个文件，逐文件下载和SHA校验；许可证另取作者固定提交 `838297cd47fed858e6cf72eaf5a52f948a3edd73` 的LICENSE，11357字节、SHA256 `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4`。
- **同名包不可混用**：已实际下载并校验GitHub发行tar（136902463字节、SHA256 `7592d1a1e9bc1e034c39fbaab9b0db8af46429d95d64c6e2a69593934af10e9c`），但其encoder/decoder SHA分别为 `fea6a8d0066b327401b603a7deff24e685379f523771bafd5127aa572018951c` / `fd2ac10c5936a1411f7a68e669e8cfec882b17791dd2ca44ab9baf69bb8ab855`，与已评测文件不同，且不带LICENSE。故本次不采用该tar，也不把其结果冒充本轮分数。
- **离线、非punct版本**；不替换为2026-06-05流式或带标点模型。encoder/joiner为INT8，decoder为原ONNX；tokens不改，解码使用已测默认配置，无热词或测试集定制。

| 文件 | 字节 | SHA256 |
| --- | ---: | --- |
| encoder-epoch-99-avg-1.int8.onnx | 161744450 | `2964d4afd257762931184b15fac04bcae79158b6b84fd9441fc33c8eb5a360ab` |
| decoder-epoch-99-avg-1.onnx | 11309084 | `c6a045cd52d4d2693c19218f404be6e37b6130e1244814ce31fa2188e4355cb5` |
| joiner-epoch-99-avg-1.int8.onnx | 2581422 | `82e88dad29301e29144565cd3b7347fa163f07ddb30ea60346b7c4c536adf4ba` |
| tokens.txt | 59240 | `ad0df596d9cc60c90795409549ccf7d1c4f901bdc060dd6b2b067d9ef2fddfaf` |

机器可读身份：[model-identity.json](../research/2026-10-25-local-asr-selection/model-identity.json)。构建及 Core 核验各推理文件和 LICENSE，新发布`coarse_segments.model_version`写入 `x-asr-zipformer-transducer-zh-en-int8-2026-06-03`；历史行原标签不改、不强制重转。

## 客户端准备与升级边界

从 **0.1.8** 起，模型随 `HearCore.app/Contents/Resources/models/` 交付。X-ASR 的四份推理资产及原 Silero/CampPlus 字节不变，具体来源和哈希以仓库 [`config/models.lock.json`](../../config/models.lock.json) 为准。Core 启动自动校验资源并实际加载 VAD/ASR；设置中的重试只重新检查，不下载。旧数据目录模型不再参与就绪判定，也不会自动删除；历史识别结果标签不改、不强制重转、不退回 SenseVoice。

Core 继续拥有模型状态和 Governor 准入，worker 只计算。IPC、数据库、推理参数和选型结论不变。当前默认按 ARM64 与 x86_64 分别打包，每包权重各保留一份；显式 Universal2 也仅含一份权重，详见[打包说明](offline-macos-packaging.md)。本次按用户要求简化交付，只进行必要编译、启动和实际包检查，不新增完整质量或能耗评测结论；原 20 组评测仍是选型依据。Intel 原生与最低系统实机验收未执行，不把 ARM 结果当作 Intel 指标。安装包与当前正在运行的安装版仍需区分。

本次验证、审查与采用状态见[执行台账](../prd/devplan_261025_xasr-model-selection.md)。以后升级先核验冻结输入/参考，再建立新模型ID、独立run/report，完整保留失败；新答案/分段/归一化须升版本。当前结论、具体权重与新评测证据一起更新，保留旧报告原文和身份。


## 公开复现的边界

上文的 `hear-asr-benchmark` 是本地私有评测工程，原声、转录和参考答案不随此仓库发布；材料名用于描述当时方法，不能作为可下载数据集入口。公开摘要保留模型文件、哈希、数量、参考构建方法及结果局限，不能据此独立重算私人样本的 CER/WER。

可独立运行的模型加载、极短输入和 worker 生命周期检查使用本仓库 `scripts/smoke_recognizer.py`，命令见[诊断与开发验证](diagnostics-and-verification.md)。真实语音端到端检查由 `scripts/verify_ingestion_quality.py` 接收调用者提供的公开授权 WAV；需要模型、ffmpeg 及隔离输出目录。它们不复现私人 20 组评测成绩，也不上传录音。
