# 离线资源与 Mac 打包（0.1.8）

## 运行架构

Core 从自身可执行文件定位 `HearCore.app/Contents/Resources/models/`，无须 GUI 先设置路径，也不依赖当前目录、App 名称或 `/Applications`。同一个包内只放一份 X-ASR / Silero / CampPlus：总计 204,642,977 字节（含对应许可），ARM 和 Intel 读取相同模型字节。Universal2 仅合并四个可执行组件的机器码，不重复模型。

`config/models.lock.json` 是资源事实源：固定 URL/revision、大小、SHA256、用途、路径及模型 ID。构建脚本校验下载缓存，仅复制清单内文件；Core 内嵌同一清单，先验证随包副本和文件哈希，再实际加载 VAD/ASR。资源不复制到用户目录，不在运行时下载；旧缓存保留但不作为 fallback。模型升级跟随应用版本，尚无独立模型更新器。

Core 保持 ModelState / PrepareModels / Governor 的既有职责。后台首检失败显示原因；重试仅重新校验与加载。资源繁忙提示稍后重试；缺失、损坏、版本不匹配提示重新安装完整 App。声纹模型校验保持独立，不阻断必需语音组件。worker 的推理参数、历史 model_version 和数据库结构保持不变。

开发态独立 Core 可显式指定 `HEAR_MODEL_ROOT` 为含 `models.lock.json` 的构建资源缓存；打包 Core 始终读取自己的资源，发行二进制忽略该变量。

## 构建

入口为 `node scripts/build.mjs build|release [--target <target>]`，默认 `universal-apple-darwin`。根目录 `build_release.sh` 仅转发 release；`scripts/build_macos.sh` 转发内部 build。支持单架构 aarch64 / x86_64 和 Universal2。后者对 Core/worker/CLI 分别编译两个真实 Rust target，再 lipo 合并；GUI 由 Tauri 的 Universal 目标组合。

`config/build-targets.json` 固定最低 macOS 13.0、支持目标及 sherpa-onnx 2.0.0 Universal 静态库的 URL/大小/SHA。工具链实际版本记录在产物报告；本次未承诺字节级可复现构建。Node/npm、Rust 两 target、Xcode 和 `npm ci` 是开发机前提。资源缓存按 SHA 分开，临时 helper 与配置位于每次独立 stage；不会共用固定 `build/macos/HearCore.app`。通过 `SHERPA_ONNX_LIB_DIR` 明确传入已核验原生库，禁止 `target-cpu=native`。

build 产生内部候选，不承诺 Developer ID 或公证。release 要求干净源码及签名公证配置；四组件版本/源码/epoch 一致、两 slice 及最低 OS、模型哈希和宿主架构离线启动检查通过后，先签 worker、helper、CLI，最后签 App。随后 DMG 签名、公证、装订、Gatekeeper、只读挂载与内容比对；全部成功才写 release-info 和最终校验和。私钥和 `.env` 不入包，不替换已安装应用，不远端发布。

权重首次构建需下载约 205 MB，之后使用校验缓存；原生库下载约 41 MB。模型与原生组件许可随包保存，来源在模型 lock 和 `licenses/sources.json`。

## 本次验证边界

用户要求跳过额外流程，本次不新增 CI、多轮独立评审或 682 段完整重跑。保留资源缺失/损坏/路径与并发重检单测、前端检查、双架构编译、真实 Core 模型加载和实际 DMG 验证。ARM 是当前宿主，可实际执行；Intel 仅完成交叉编译和 slice 检查，原生及 macOS 13 实机运行未验证。

历史音频保持本机；若将来补 Intel 原生 CI，按用户决定使用公开音频，未经授权不上传历史录音。原始评测集和参考答案不变。最终体积、签名与本轮实际结果见 release-info 和执行台账。

## 0.1.8 本地产物

2026-10-26 已生成 `hear_0.1.8_macos-universal2.dmg`，199.75 MB（190.50 MiB）。Apple 公证、装订、Gatekeeper 和实际 DMG 检查通过；未替换本机安装。源码与产物校验和、组件身份及验证边界见[机器可读交付记录](../research/2026-10-26-offline-universal-release.json)。
