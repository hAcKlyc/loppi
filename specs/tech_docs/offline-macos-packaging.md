# 离线资源与 Mac 打包

## 运行架构

Core 从自身可执行文件定位 `LoppiCore.app/Contents/Resources/models/`，无须 GUI 先设置路径，也不依赖当前目录、App 名称或 `/Applications`。同一个包内只放一份 X-ASR / Silero / CampPlus：总计 204,642,977 字节（含对应许可），ARM 和 Intel 读取相同模型字节。Universal2 仅合并四个可执行组件的机器码，不重复模型。

`config/models.lock.json` 是资源事实源：固定 URL/revision、大小、SHA256、用途、路径及模型 ID。构建脚本校验下载缓存，仅复制清单内文件；Core 内嵌同一清单，先验证随包副本和文件哈希，再实际加载 VAD/ASR。资源不复制到用户目录，不在运行时下载；旧缓存保留但不作为 fallback。模型升级跟随应用版本，尚无独立模型更新器。

Core 保持 ModelState / PrepareModels / Governor 的既有职责。后台首检失败显示原因；重试仅重新校验与加载。资源繁忙提示稍后重试；缺失、损坏、版本不匹配提示重新安装完整 App。声纹模型校验保持独立，不阻断必需语音组件。worker 的推理参数、历史 model_version 和数据库结构保持不变。

开发态独立 Core 可显式指定 `LOPPI_MODEL_ROOT` 为含 `models.lock.json` 的构建资源缓存；打包 Core 始终读取自己的资源，发行二进制忽略该变量。

## 构建

入口为 `node scripts/build.mjs build|release [--target <target>]`。`config/build-targets.json` 的 `default_targets` 固定顺序为 aarch64、x86_64，默认分别生成两个单架构包。根目录 `build_dev.sh` 和 `scripts/build_macos.sh` 转发内部 build；`build_release.sh` 转发 release。显式 `--target` 只构建对应目标，保留 Universal2：对 Core/worker/CLI 分别编译两 target 再 lipo，GUI 由 Tauri Universal 目标组合。

默认两包顺序构建，避免共享 Cargo/前端输出竞争。一次入口执行共用版本、源码 SHA 与 epoch；每目标单独 stage、App 与输出目录。四个执行组件必须与目标架构完全相同，不能误带另一 slice；单架构不使用 lipo 合并。每包独立包含一份同样的模型。

`config/build-targets.json` 同时固定最低 macOS 13.0、目标及 sherpa-onnx 2.0.0 Universal 静态库的 URL/大小/SHA。Node/npm、Rust 两 target、Xcode 和 `npm ci` 是开发机前提；实际版本写入报告，不承诺字节级可复现。资源缓存按 SHA 分开，临时 helper/配置属于各 stage。`SHERPA_ONNX_LIB_DIR` 指向核验后的原生库；禁止 `target-cpu=native`。

- build：允许脏源码，优化编译，生成 `build/dev/<版本>/<平台>/loppi_<版本>_<平台>-dev.dmg`，附 `build-info.json` 与 `SHA256SUMS`。内部候选不要求 Developer ID、公证或 Gatekeeper 分发验收；仍验证实际 DMG 挂载、Applications 链接、完整内容和可执行时的离线模型加载。
- release：要求干净源码及签名/公证配置，生成 `build/releases/<版本>/<平台>/loppi_<版本>_<平台>.dmg`，附 `release-info.json` 与 `SHA256SUMS`。逐包先签 worker、helper、CLI，最后签 App，再签 DMG、公证、装订、Gatekeeper、只读挂载与内容比对。全部成功才写该包报告和校验和；两包均成功后才打印批次完成。

平台名为 `macos-arm64` / `macos-x86_64`，显式 Universal2 为 `macos-universal2`。四组件编译身份先验证一致，再与入口版本/源码/dirty/epoch 核对。通过系统查询实际硬件架构，避免把 Rosetta 下的 Intel Node 当成 Intel 硬件；运行报告的架构取自实际 worker。检查执行能力：宿主架构原生；ARM 宿主已有 Rosetta 时允许 Intel 翻译执行，报告 `runtime_execution: rosetta`；不可执行时报告 `not_available`、运行数据为 null，不生成伪造的已核验身份。各情况下仍必须通过架构、最低 OS、资源/lock 哈希检查。最终挂载 DMG 的运行检查同样按实际能力执行。

私钥和 `.env` 不入包；脚本不替换已安装应用、不远端发布。`package_macos.sh [--dev] <App> <DMG>` 的缺省模式仍要求签名和公证，`--dev` 仅用于内部未公证候选。

权重首次构建需下载约 205 MB，之后使用校验缓存；原生库下载约 41 MB。模型与原生组件许可随包保存，来源在模型 lock 和 `licenses/sources.json`。

## 验证边界

保留资源缺失/损坏/路径与并发重检单测、前端检查、双架构编译、真实 Core 模型加载和实际 DMG 验证。当前 ARM 宿主可执行原生和已有 Rosetta 下的 Intel 检查；这不等价于 Intel 硬件或 macOS 13 实机验收。每次产物的实际覆盖以报告为准，不新增 CI 或重跑历史音频评测集。

历史音频保持本机；若将来补 Intel 原生 CI，按用户决定使用公开音频，未经授权不上传历史录音。原始评测集和参考答案不变。最终体积、签名与本轮实际结果见 release-info 和执行台账。

## 0.1.8 本地产物

2026-10-26 已生成 `hear_0.1.8_macos-universal2.dmg`，199.75 MB（190.50 MiB）。Apple 公证、装订、Gatekeeper 和实际 DMG 检查通过；未替换本机安装。源码与产物校验和、组件身份及验证边界见[机器可读交付记录](../research/2026-10-26-offline-universal-release.json)。
