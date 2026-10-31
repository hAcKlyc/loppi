# 本地 CPU ASR 候选调研

核实日期：2026-10-24。目标：为 hear 的真实事件粗转录 benchmark 筛选模型和部署组合。当前实验机为 Apple M5 / 24 GiB / macOS 26.6.2，现有链路为 sherpa-onnx 1.13.8 + SenseVoice INT8、2 线程。

**本报告是联网调查与候选筛选，尚未安装或运行这些候选。下文外部跑分均标明来源，不能当作本机、私人测试集或能耗的实测结论。**

## 结论与取舍

此前初筛需要扩充。优先实测应包含：SenseVoice 基线、FireRedASR2 CTC、Qwen3-ASR 0.6B/1.7B、Fun-ASR-Nano、X-ASR、MOSS-Transcribe-Diarize 0.9B、VibeVoice-ASR-BitNet、Cohere Transcribe，以及 Whisper 对照。它们提供不同的准确率、模型体积和计算开销取舍；尚不能宣布哪个适合直接替换现有模型。

近期变化不只来自新权重，也来自 CPU 运行时：Qwen3-ASR 已有 C、Rust 和 GGUF 实现；MOSS 有专门的 C++ 实现；微软为 VibeVoice 发布了明确面向 CPU 的 BitNet 变体。不能把早期 Python/CUDA 部署要求套到今天的全部实现，也不能因存在 CPU 参数就推断后台持续运行成本合格。

另一条值得优先验证的路线是复用现有 sherpa-onnx：FireRedASR2 的 CTC 分支已有 INT8 转换，X-ASR 使用 Zipformer Transducer。它们避免长文本自回归解码或控制解码开销，适合检验持续粗转场景的准确率/计算成本边界。这里是架构推断，不是性能结论。[FireRed 部署文档](https://k2-fsa.github.io/sherpa/onnx/FireRedAsr/pretrained.html)、[X-ASR 作者仓库](https://github.com/Gilgamesh-J/X-ASR)。

## 调研方法与证据强度

- 从 Hugging Face 模型卡、文件列表、官方组织的发布记录、GitHub、论文、Reddit 与可检索社媒查找线索。X 原站检索可见性有限，不假称全面读过 X；检索到的镜像或二手聚合只作线索。
- 技术事实回到模型作者或运行时维护者的一手资料；社媒热度不作为准确率证据。
- 用 Hugging Face 公共 API 记录原始/量化权重的文件大小、版本与文件指纹；GitHub 公共 API 记录主要运行时提交。没有下载模型权重，也没有外发私人音频。
- CPU 能运行、CPU 上快于实时、在当前 2 线程后台预算下可用，是三个不同门槛。Apple GPU/Metal、MLX GPU、ANE 的数字不混入 CPU 结果。
- 时间以论文/发布公告/仓库历史为线索，Hub 创建时间和最后更新日不等同正式发布日期。元数据中的 `401` 只证明该次请求未成功，不据此判断模型不存在、闭源或必须付费。

## 优先实测的模型与运行时

以下 MB/GB 为十进制文件大小，GiB 为二进制容量。数值仅指列明的权重文件，不是进程内存、压缩下载包或最终安装包体积。原模型与量化转换要分别固定版本。

| 路线 | 当前可核实的 CPU 部署证据 | 文件与资源线索 | 在 hear 中要回答的问题 |
|---|---|---|---|
| SenseVoice 当前 INT8 | 正在使用的 sherpa-onnx CPU 链路 | 本机模型文件 239,233,841 字节，约 239 MB | 生产基线；增加非 INT8 对照可区分量化损失与模型能力 |
| FireRedASR2 CTC INT8 | sherpa 明确提供 CTC 分支及单线程 CPU 命令；当前本机 Rust 1.13.8 包含对应配置类型 | ONNX 775,861,420 字节，约 776 MB | 中文/口音/中英混说是否较基线改善；CTC 效果与完整 AED 不能混称 |
| Qwen3-ASR 0.6B | `antirez/qwen-asr`、Rust `QwenASR`、`transcribe.cpp` 均有 CPU 路径 | 官方 BF16 权重约 1.876 GB；Handy Q8 850 MB、Q4_K_M 590 MB | 准确率与后台 CPU/能耗是否达到可替换门槛；原生运行时会改变结论 |
| Qwen3-ASR 1.7B | 上述原生实现支持两个型号 | 官方 BF16 分片合计约 4.699 GB；Handy Q8 2.185 GB、Q4_K_M 1.320 GB | 额外准确率能否抵偿更高加载、解码和常驻成本 |
| Fun-ASR-Nano 2512 | 当前 sherpa Rust 包已有配置；作者生态有 llama.cpp/GGUF CPU 路线 | FunAudioLLM GGUF Q8 1.045 GB、F16 1.676 GB | 中文难例、术语与方言是否实质改善；不同转换的量化与授权标注须固定 |
| X-ASR-zh-en | 作者提供 sherpa-onnx 模型与真流式/离线解码；约 0.16B 的作者标称 | Hub 提供分 chunk 的 ONNX 文件；480 ms encoder 约 593 MB，不能只计 encoder 忽略 decoder/joiner | 小型中英模型能否提供更好的持续运行成本；离线与流式分别评价 |
| MOSS-Transcribe-Diarize 0.9B | LocalAI 的 `moss-transcribe.cpp` 与 Handy 均有 C++/ggml CPU 实现和参考核验记录 | 原 BF16 1.817 GB；Handy Q8 987 MB、Q4_K_M 617 MB | 长录音、多人交谈的识别是否改善；额外说话人功能不能替代文字准确率，也不默认替换现有人物链路 |
| VibeVoice-ASR-BitNet | 微软官方 `VibeASR.cpp`，ARM NEON / x86 AVX2，明确面向 CPU | 两个必需 GGUF 合计 1,695,957,664 字节，即 1.696 GB / 1.58 GiB | 强量化带来的效果与耗能取舍；作者的 3 线程实时结论是否适用于我们的设备和中文 |
| Cohere Transcribe 03-2026 | 作者确认支持普通话；Handy 提供 CPU 量化运行与参考核验记录 | 原权重 4.132 GB；Handy Q8 2.411 GB、Q4_K_M 1.558 GB | 大编码器、小解码器能否取得较好 CPU 效率；英文榜首不能外推中文领先 |
| Whisper small / large-v3-turbo | 成熟的 whisper.cpp CPU 和量化路径 | 按选择的实际量化文件计量；不套用 Metal/Core ML 的速度 | 保留跨架构、多语种对照，检验噪声、幻觉、长音频切分与英文片段 |

FireRed 的 CTC 转换**只包含完整 AED 的 CTC 分支**，移除了注意力解码器。完整 FireRedASR2-AED 的 INT8 encoder + decoder 约 1.235 GB，可作为额外效果对照；不能把 AED 或 LLM 的论文分数直接挂到 CTC 版本上。[sherpa 一手说明](https://k2-fsa.github.io/sherpa/onnx/FireRedAsr/pretrained.html)。

Qwen 需要同时比较模型与运行时。C 实现支持直接读取原 safetensors、macOS Accelerate 与 Linux OpenBLAS；Rust 版本针对 Apple Silicon 优化；Handy 是另一套量化与 ggml 路径。三者不是同一个实验配置。共享权重、转换布局和张量去重会影响文件体积，不能仅凭型号名或 Hub 张量计数估算总内存。[C 实现](https://github.com/antirez/qwen-asr)、[Rust 实现](https://github.com/huanglizhuo/QwenASR)、[Handy Qwen 模型说明](https://github.com/handy-computer/transcribe.cpp/blob/main/docs/models/qwen3-asr.md)。

MOSS 作者于 2026-07-09 发布 0.9B 版本，面向长录音、多人转录、时间戳和自定义热词。作者的中文会议、播客结果值得把它列入候选，但长上下文会改变内存开销；按生产短段测和按完整活动测必须分开报告。[作者模型卡](https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize)、[LocalAI CPU 实现](https://github.com/localai-org/moss-transcribe.cpp)。

## 扩展候选与暂不优先的理由

| 路线 | 查证结论与下一步处理 |
|---|---|
| Paraformer INT8 / small INT8 | 当前运行时可直接支持，约 227 MB / 82 MB 模型文件。值得作为低体积、低计算成本对照；不能预设比当前 SenseVoice 更准。[部署文档](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-paraformer/index.html) |
| Dolphin-CN-Dialect base / small | 2026 年 5 月的新中文/方言系列；作者标称约 0.1B / 0.4B，提供离线、流式、热词版本；已核实 base 权重约 447 MB。当前 sherpa 有 Dolphin 配置，但不能未经导出适配就认定所有 CN 新版可直接加载。优先补核中文版本的 ONNX 与 CPU 吞吐。[作者仓库](https://github.com/DataoceanAI/Dolphin)、[清华发布页](https://web.ee.tsinghua.edu.cn/satlab/zh_CN/gxsj/6995/content/1051.htm) |
| Nemotron 3.5 ASR 0.6B | 2026-06-04 发布，支持普通话，已有 Q8 GGUF 742 MB。官方把中文列于 broad-coverage，而非最高准确率的 transcription-ready 档；原生流式值得比较，中文领先尚无证据。[官方模型卡](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b) |
| ARK-ASR 0.6B | 作者使用蒸馏方案，支持中英等语言，有 CPU 示例；原始 BF16 文件约 2.599 GB，型号名是解码器规模，不能当全模型规模。可作精度候补，需继续核实实际 CPU 优化和分发运行时。[作者模型卡](https://huggingface.co/Edge0/ARK-ASR-0.6B) |
| Audio8-ASR 0.1B | 实际端到端约 324M 参数，原权重约 648 MB；有 ONNX 端侧版本，CPU/体积方向有价值。卡片标注 CC-BY-NC-4.0，暂作为研究参考，不能按可直接分发的产品候选处理。其 GPU 高并发吞吐不能当 CPU 速度。[作者模型卡](https://huggingface.co/Audio8/Audio8-ASR-0.1B) |
| Moonshine 中文 tiny | 中文权重约 108 MB，27M 参数；有 ONNX 路径，是极低资源对照。不能把新版英文 streaming 的榜单表现外推到中文 tiny；语言专用模型还要考察中英混说损失。模型文件的具体许可优先于库的许可。[中文模型](https://huggingface.co/moonshine-ai/moonshine-tiny-zh)、[作者系列论文](https://arxiv.org/abs/2509.02523) |
| Meta Omnilingual ASR CTC 300M | 官方覆盖普通话等大量语言，提供 CTC 小模型；当前 sherpa Rust 包有 Omnilingual CTC 配置。适合低资源/多语种探索，但目前未核实在本场景优于中文专用模型。`-v2` 的某次 Hub 请求未成功，不作不存在判断。[官方 CTC 模型](https://huggingface.co/facebook/omniASR-CTC-300M) |
| GLM-ASR-Nano 2512 | 中英支持，原权重约 4.516 GB；已有 CPU 示例与社区原生实现线索。需核实同型号、同版本的中文质量和内存，不能用一段 JFK 英文音频的零错误当完整验证。[作者仓库](https://github.com/zai-org/GLM-ASR)、[模型卡](https://huggingface.co/zai-org/GLM-ASR-Nano-2512) |
| Voxtral Mini 4B Realtime | 支持中文，有 Pure C 等社区 CPU 路线，流式机制值得关注；模型与上下文预算明显更重，作为资源达标后再扩展的候选。官方 GPU 部署门槛不等于 CPU 绝不可能，社区 CPU 实现也不代表 2 线程后台能跑得好。[官方模型卡](https://huggingface.co/mistralai/Voxtral-Mini-4B-Realtime-2602) |
| Ming-3.0-asr-tiny | 2026-10-19 左右公开发布的近期进展，支持中文/方言/上下文；Hub 原权重合计约 17.733 GB、约 8.87B 张量计数，官方示例依赖 Linux/CUDA。没有查到足以证明当前后台 CPU 预算合格的证据，保留跟踪，暂不列首选。Tiny 命名不能当轻量保证。[作者模型卡](https://huggingface.co/inclusionAI/Ming-3.0-asr-tiny) |
| MiMo-V2.5-ASR | 中英、方言、混说、复杂声学场景有作者评测；当前原 safetensors 合计约 32.074 GB，官方起步依赖 Linux/CUDA，超出当前 24 GiB 机器直接装载原格式的合理范围。未核实适用本预算的量化 CPU 实现；社区其他名为 MiMo-ASR 的较小旧模型不能与 V2.5 混为一谈。[官方模型卡](https://huggingface.co/XiaomiMiMo/MiMo-V2.5-ASR) |
| Granite Speech 5.0 2B / TurboCTC | 2026-10-21 的确有新发布/更新，但 2B 的六种 ASR 输入语言不含中文，英文到中文翻译另计；TurboCTC 新版增加多语种仍未含中文。不适合作为中文主力替代，纯英文可另测。[IBM 官方更新](https://huggingface.co/blog/ibm-granite/granite-speech-5) |
| Parakeet TDT 0.6B v3 | 速度与英语/欧洲多语种方向值得关注，但该版官方语言列表不含中文，排除中文主力替代；不可因“多语种”就假定中英混说可用。[官方模型卡](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) |
| Apple SpeechTranscriber / SpeechAnalyzer | 下载语言资产后本地识别，适合 Mac 产品的补充比较；系统会调配计算资源，不能把它声称为可强制 CPU 的同预算结果。资源统计还要覆盖系统代管进程。[Apple 官方介绍](https://developer.apple.com/videos/play/wwdc2025/277/) |

## 外部 CPU 数据：有用，但边界必须保留

| 来源与条件 | 发布者报告的结果 | 能支持什么；不能支持什么 |
|---|---|---|
| Rust QwenASR，Apple M5 Pro / 48 GB，0.6B，同一 28.2 秒音频，10 次默认配置 | 推理中位数 613 ms，约 46 倍实时；报告另列含进程开销的耗时 | 支持专用 CPU 运行时值得测试；不证明中文普遍准确、2 线程合格或更省电。机器也不是本机普通 M5。[原始说明](https://github.com/huanglizhuo/QwenASR) |
| 微软 VibeVoice-ASR-BitNet，AMD EPYC 7V13，20 秒音频 | 2 线程 RTF 1.08，3 线程 0.77，4 线程 0.63 | 支持 CPU 实时可行；不证明任意 CPU、2 线程或我们的场景可用。中文 FLEURS/AISHELL4 的作者比较并非全面胜过 SenseVoice。[官方数据](https://huggingface.co/microsoft/VibeVoice-ASR-BitNet) |
| Handy Cohere，M4 Max CPU，Q8，预热后 3 次计算均值 | 11 秒样本约 926 ms；35.3 秒样本约 3.25 秒 | 有明确 CPU 与 Metal 分列，以及运行时提交；只是少量英文音频，加载排除，不能代替中文与能耗测试。[维护者记录](https://github.com/handy-computer/transcribe.cpp/blob/main/docs/models/cohere-transcribe-03-2026.md) |
| Handy MOSS，M4 Max CPU，Q8，同一预热协议 | 11 秒样本约 2.08 秒；35.3 秒样本约 5.43 秒 | 支持 CPU 路线可行；不能据此推断 90 分钟整场的内存和准确率。[维护者记录](https://github.com/handy-computer/transcribe.cpp/blob/main/docs/models/moss-transcribe-diarize.md) |
| FunASR 生态 CPU benchmark，184 段真实中文，8 线程，排除模型加载 | SenseVoice Q8 CER 8.17，Fun-ASR-Nano Q8 8.42；Whisper small 22.12、turbo 23.15 | 说明新模型未必在每套中文数据上胜过 SenseVoice；是特定作者数据和归一化口径的结果，不复制为 hear 排名。[原始说明](https://github.com/QwenAudio/SenseVoice/blob/main/runtime/llama.cpp/BENCHMARKS.md) |

不能把不同发布者的 CER/WER、音频长度、量化、线程数、VAD、热词或归一化混成一个总榜。部分轻量模型在标准朗读集表现不错，但真实多人录音、低音量、中英混说可能改变排名。

## 社媒调查实际带来的线索

- [2026 年 10 月的 MOSS 长会议实跑帖](https://www.reddit.com/r/LocalLLaMA/comments/1xcl0rt/ran_a_09b_transcribe_diarize_model_on_a_175/)提示小型转录/说话人联合模型已进入实际 Mac 长录音测试；回到 MOSS 官方卡和 CPU 实现核实，帖子单场结果不作胜出证据。
- [VibeVoice-ASR-BitNet 讨论](https://www.reddit.com/r/LocalLLaMA/comments/1v8ncmr/microsoftvibevoiceasrbitnet/)提供专用 CPU 量化线索；最终采用微软仓库、模型文件和论文作为技术依据。
- [Qwen3 与其他模型的使用讨论](https://www.reddit.com/r/LocalLLaMA/comments/1ups1c8/are_there_any_local_asr_models_that_surpass/)涉及准确率、时间戳和不同场景结论；用户经历相互有差异，说明要在本用户录音上评估，不能把热度当排名。
- [audio.cpp 作者更新讨论](https://www.reddit.com/r/LocalLLaMA/comments/1urd4ln/audiocpp_what_does_the_fox_say_4_asr_models/)把多个新模型带入原生部署视野，但作者也明确当时重点是 CUDA 优化；没有把宣传吞吐作为 CPU 数据。

## 对 benchmark 设计的直接影响

1. **主评测固定 CPU。** 同一设备、固定生产分段、无 GPU/Metal/ANE，先跑现有 2 线程预算；额外报告候选在 4 线程等实际可用配置下的结果，单列计算开销。显式设置 BLAS/运行时线程，核对实际进程 CPU，避免名义两线程但底层开满核心。
2. **配置粒度为模型 × 权重版本 × 量化 × 运行时 × 解码。** Qwen 的原生 BF16、GGUF Q8 和 Q4 各自计分。先验证转换与原始实现的一致性，再谈产品适用性；不要求所有浮点后端逐字一致，但差异必须留存。
3. **默认持续预热与冷启动分开。** hear 粗转 worker 会复用，热运行更贴近日常；下载、加载、首次推理、闲置常驻和活动推理均记录。RTF 是推理时间 / 音频时长，越低越快；快不自动意味着低能耗。
4. **把模型增益与切分增益分开。** 共同短段衡量可替换性；对支持整场/长上下文者再测自身推荐分段，报告额外等待、内存与真实增益。VAD、降噪或热词不偷偷加给单个候选；若测试，作为显式独立配置。
5. **体积与内存使用实数。** 指定必需文件及压缩下载量、落盘量、原生运行时增量，排除重复格式和开发环境；内存记录进程树物理足迹、峰值、mmap/共享页与 swap。统一内存不能简单拿 RSS 或权重大小代替。没有实测的能耗/内存不填估算排名。
6. **先冻结自动核验参考答案。** 用户已明确不参与校对，由 Agent 负责。利用既有精转与独立声学证据修订；听不清区域单列，同时报告其时长和占比，避免靠排除难例抬分。若某个受评模型参与生成参考答案，需记录来源，并对相关区间做不使用该模型输出的复核或敏感性检查，防止自我一致得分优势。
7. **用分层结果选型。** 中文 CER、中英混说 MER、关键人名/术语/数字/否定错误、插入/漏字、无声幻觉、失败率，与 RTF/延迟/能耗一起看；按事件等权和按字数加权都报告。约 10 组用于本场景回归，不能宣称普遍 SOTA。

## 版本、文件与尚待验证事项

- [原始模型元数据](hf-model-metadata.json)、[补充及量化模型元数据](hf-model-metadata-extra.json)：31 次精确模型查询，含失败与重定向记录。
- [运行时元数据](github-runtime-metadata.json)：10 个运行时/作者仓库的提交与许可标识。
- [Hub 查找记录](hf-discovery.json)：FireRed、Dolphin、Qwen 量化仓库发现路径。

需要在实测阶段解决：低量化对中文术语的损失；Qwen 各运行时在同样 2 线程下的性能；CTC 与完整 AED 的准确率差距；MOSS 在短段与整场的差别；所有候选的物理内存和能耗；后台与录音并行时的积压和资源竞争。

Fun-ASR-Nano 原始卡片与其 GGUF 仓库的许可标注存在差异，后者明确提及 FunASR Model License 1.1；应按具体权重的附带条款核实，不能沿用最初看到的 Apache 标签就宣称可直接打包。[原始卡片](https://huggingface.co/FunAudioLLM/Fun-ASR-Nano-2512)、[GGUF 许可说明](https://huggingface.co/FunAudioLLM/Fun-ASR-Nano-2512-GGUF)。这不阻碍记录其研究结果，但影响后续分发决策。

本轮只新增公开来源的研究资产及讨论记录；现有测试集尚未制作，未产生私人测试集分数，也没有更换产品模型。
