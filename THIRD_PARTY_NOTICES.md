# 第三方组件与许可

loppi 自有代码、文档和图像采用根目录 [Apache License 2.0](LICENSE)，版权署名 Ethan L。第三方库、模型和系统组件保留各自许可，不能用项目许可证替代。项目来源及品牌边界见 [NOTICE](NOTICE)和[品牌说明](docs/brand.md)。

## 应用依赖

[组件清单](licenses/component-inventory.json)按当前 `Cargo.lock` 与 `package-lock.json` 记录精确版本、声明许可、来源和许可文本哈希。清单包含 **571 个第三方 Cargo 包**（含构建、测试与其他平台依赖）及 **108 个前端生产依赖**，用于来源与许可追踪，不是某个平台二进制实际链接内容的 SBOM。

- [前端许可原文](licenses/frontend-third-party.txt)：React、Tauri JS API、Markdown 及其生产依赖的版权与许可声明。
- [Rust 包内许可原文](licenses/rust-third-party.txt)及[上游补充声明](licenses/rust-upstream-notices.txt)：保留 crate 自带文本，缺少独立许可文件的包补充其固定源码提交中的声明或明确的标准许可文本。
- [MPL 2.0](licenses/MPL-2.0.txt)：适用于清单中相应组件。每个 Cargo 组件的完整源码可从清单的固定版本 `.crate` 链接获取，并以 Cargo 锁文件校验和核对；保留上游源文件声明，修改相关文件时继续履行其许可义务。

清单保留原 SPDX 表达式；多重许可按上游允许的选项使用，不将 OR 误认为所有许可必须同时适用。r-efi 使用其 MIT 选项，其完整 AUTHORS 声明也保留在补充文本中。系统字体不随项目分发。

## 离线语音模型与运行库

| 组件 | 许可与来源 |
| --- | --- |
| X-ASR Zipformer Transducer | Apache-2.0；固定权重与作者 LICENSE 见 `config/models.lock.json` |
| Silero VAD | MIT；固定模型与 LICENSE 见模型锁文件 |
| CampPlus / 3D-Speaker | Apache-2.0；固定模型与 LICENSE 见模型锁文件 |
| sherpa-onnx 2.0.0 | Apache-2.0；原文与来源见 `licenses/sherpa-onnx.txt`、`licenses/sources.json` |
| ONNX Runtime 1.28.2 | MIT；完整第三方声明见 `licenses/onnxruntime-third-party.txt` |

原生语音运行库还包含 kaldi-decoder、kaldi-native-fbank、kaldifst/OpenFst、KissFFT、PortAudio、SentencePiece 和 utf8proc，其原始文本及固定来源哈希保存在 `licenses/`。Opus、SQLite 及 Rust 绑定按各自条款提供；相关包及随包原生源码中的许可已列入 Rust 文本，SQLite 原生实现另见其[公有领域声明](https://www.sqlite.org/copyright.html)。

模型文件与可执行架构分开管理；每个安装包的具体文件和许可以锁定资源及最终包清单为准，运行库的许可不自动覆盖所有模型权重。

## Windows 资源

Windows 的 PNG-only OCR 使用源码构建的 Tesseract 5.4.0、Leptonica 1.84.1、libpng 1.6.43 和 zlib 1.3.1。源码归档及哈希见 `config/windows-ocr-sources.json`；构建器随 OCR 资源复制四者原文与来源。中英文 tessdata_fast 权重来自 `config/windows-assets.json` 固定提交，其 Apache-2.0 原文另见 `licenses/tessdata-fast.txt`。

VC 运行库与 WebView2 由 Microsoft 的相应许可约束，不适用 loppi 的 Apache-2.0。VC runtime 的官方再分发许可和来源由 Windows 构建器随资源复制。仅使用许可允许再分发的组件，不能将开发工具或 Windows SDK 整体当作运行资源打包。

## 分发与维护

现有打包器复制 `licenses/`，其中包含项目 `LICENSE` / `NOTICE` 副本、第三方原文与组件清单。模型许可随模型资源目录保存，Windows OCR 与 VC 原文由其构建步骤补入。发布前核对最终安装包确实包含这些文件；本次源码声明更新不会修改已经发行的旧安装包。

依赖或资源变化后更新组件清单、文本和哈希，保持第三方署名。版权与许可咨询请联系 [myagents.io@gmail.com](mailto:myagents.io@gmail.com)。
