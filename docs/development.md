# 开发指南

本指南说明当前工作树的构建与验证。安装使用见[使用指南](usage.md)，贡献边界见 [CONTRIBUTING.md](../CONTRIBUTING.md)。正式发行状态与源码版本分别核对[版本说明](releases.md)。

## 环境与快速开始

| 工具 | 要求 |
| --- | --- |
| Node.js | 24，`.nvmrc` 提供主版本；使用 `npm ci` 安装锁定依赖 |
| Rust | stable，支持 Edition 2024；项目尚未声明独立验证的最低 Rust 版本 |
| macOS 构建 | Xcode 26.0.1+ 与 macOS 26 SDK，选择相应开发者目录；最低部署版本仍为 macOS 13 |
| Windows 构建 | Windows 11 x64、Rust MSVC、VS C++ 工具、Windows SDK、CMake、Python、WebView2；OCR使用Visual Studio生成器，CI通过CMAKE_GENERATOR选择VS2026；发行使用目录14.51.36231中锁定的VC runtime 14.51.36247.0 |

先执行 `npm ci`、`npm run typecheck`、`npm run lint`。本机单架构候选在 Apple Silicon 上使用 `./build_dev.sh --target aarch64-apple-darwin`，Intel 使用 `--target x86_64-apple-darwin`；Windows 使用 `./build_dev.ps1`。开发候选无需发行证书，允许未提交源码，仍会完整打包离线资源。首次 Mac 构建约需下载 205 MB 模型和 41 MB 原生库，Windows 另有 OCR 与运行库。

`npm run dev` 仅启动 Vite，浏览器内无法提供原生录音、权限和 IPC。完整桌面、权限与采集验收使用构建后的 App；构建脚本不会替换已安装应用。没有已有资源时直接运行 `npm run tauri dev` 也不等于完成 Core、helper 和模型准备，尤其不能用未打包的 GUI 验证 macOS 授权。

纯前端样式与组件可使用[离线真实组件预览](../design/system-v1/preview.html)，其数据为合成样本。前端开发服务和预览都不能证明录音或原生交互通过。

## 离线验证

```sh
npm run verify
# 可按范围运行
python3 scripts/verify.py --suite static
python3 scripts/verify.py --suite test
python3 scripts/verify.py --suite smoke
```

Windows 请使用 `python scripts/verify.py`，可附同样的 `--suite` 参数；该入口会转到 `verify_windows.py`。`npm run verify` 使用 `python3` 命令，在未提供该别名的 Windows 环境中改用上述直接入口。Windows Rust 链接需要先解压 `config/windows-assets.json` 固定的 sherpa 原生归档，并将 `SHERPA_ONNX_LIB_DIR` 指向其中 `lib`；CI 中已有相同准备。完整 Windows 开发构建也会准备该归档。独立运行 Core 时，其 worker 应位于同一可执行目录；调试 Core 可通过 `LOPPI_MODEL_ROOT` 显式选择含锁文件的已校验模型缓存，发行 Core 忽略此变量。

默认门禁剔除私有 fixture 配置，不调用付费服务。测试使用临时目录和测试 owner，不能改为日常录音目录。实际采集、设备切换、睡眠、权限、桌面截图、安装和公证属于平台实机验收。详细诊断、资源观测和显式模型检查见[诊断与开发验证](../specs/tech_docs/diagnostics-and-verification.md)。

## Windows 构建与使用

需要 Windows 11 x64、Node.js/npm、Rust MSVC、Visual Studio C++ 工具与 Windows SDK、CMake/Ninja、Python，以及 WebView2。首次在具备 MSVC 环境的 PowerShell 中运行：

```powershell
npm ci
./build_dev.ps1
# 干净、已提交源码的发行构建：
./build_release.ps1
```

两入口共用 `scripts/build_windows.mjs`：生成 x64 NSIS 安装包，随包提供语音模型、离线中英文 OCR、推理 DLL、VC 运行库、WebView2 离线安装程序与第三方许可。发行入口要求干净源码，当前没有 Windows 代码签名证书，报告如实标记 `unsigned`。输出及精确依赖要求见 [Windows 支持说明](../specs/tech_docs/windows-support.md)。

默认程序目录是 `%LOCALAPPDATA%\loppi`，录音和数据库另存于 `%LOCALAPPDATA%\app.loppi.core`；默认卸载保留资料。Windows 麦克风使用桌面应用访问权限，电脑声音由 WASAPI 获取。锁屏时录音可以继续，桌面截图会明确提示不可用。系统托盘和悬浮入口共享同一只状态兔子。CLI/MCP 接入请从应用的「Agent 接入」复制当前安装路径和 PowerShell 命令。

## macOS 发行构建

在已安装 Rust stable、Node.js 24/npm、Xcode 26.0.1 或更新版本（含 macOS 26 SDK），并有 Developer ID Application 证书的 Mac 上，从仓库根目录执行：

```sh
npm ci # 首次构建或依赖更新后执行
./build_release.sh
```

默认顺序生成 **Apple Silicon（ARM64）和 Intel（x86_64）两份独立安装包**。每个包的 GUI、CLI、Core 和 worker 都只含对应架构；每包各带一份约 **204.64 MB（195.16 MiB）** 的 X-ASR、VAD 和声纹模型。安装后自动检查和加载本地组件，无需额外下载。

先安装两个 Rust target：`rustup target add aarch64-apple-darwin x86_64-apple-darwin`。发行须从干净且已提交的源码构建，分别完成编译、资源与启动检查、签名、DMG、公证和装订。输出为：

- `build/releases/<版本>/macos-arm64/loppi_<版本>_macos-arm64.dmg`
- `build/releases/<版本>/macos-x86_64/loppi_<版本>_macos-x86_64.dmg`

各目录附 `release-info.json` 和 `SHA256SUMS`。同次执行的两包使用同一版本、源码和构建时间；按架构分发即可。签名证书通过 `APPLE_SIGNING_IDENTITY` 指定，未指定时选择本机 Developer ID Application 证书。

开发候选也默认生成两份 DMG，允许未提交源码，不要求 Developer ID 或公证：

```sh
./build_dev.sh
# scripts/build_macos.sh 也是同一个开发构建入口
```

输出在 `build/dev/<版本>/macos-arm64/` 和 `macos-x86_64/`，文件名带 `-dev`，各附 `build-info.json` 和 `SHA256SUMS`。开发候选使用优化编译，供内部安装测试；`npm run dev` 仍只启动前端开发服务。

需要只构建一个架构，或显式生成 Universal2 时：

```sh
./build_dev.sh --target aarch64-apple-darwin
./build_release.sh --target x86_64-apple-darwin
./build_release.sh --target universal-apple-darwin
```

中间 App 位于 `target/<target>/release/bundle/macos/loppi.app`。宿主架构执行原生检查；ARM Mac 已有 Rosetta 时也执行 Intel 包的翻译环境检查。无法执行的异架构仅验证编译、切片、最低系统版本和资源哈希，报告明确标记，不冒充原生验收。脚本不会更新 `/Applications/loppi.app`。完整布局与验证边界见[离线资源与 Mac 打包说明](../specs/tech_docs/offline-macos-packaging.md)。

对外分发前需要完成 Apple 公证。`build_release.sh` 自动读取根目录 `.env` 中的以下配置（已有环境变量优先），不会把 `.env` 或私钥文件放进安装包：

```dotenv
APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
APPLE_API_KEY=YOUR_KEY_ID
APPLE_API_ISSUER=YOUR_ISSUER_ID
APPLE_API_KEY_PATH=/absolute/path/to/AuthKey_YOUR_KEY_ID.p8
```

`APPLE_API_KEY` 是 App Store Connect API Key 的 ID，私钥放在 `APPLE_API_KEY_PATH` 指定的文件中。团队 API Key 需要 Issuer ID，个人 API Key 不应填写 Issuer ID。私钥路径支持绝对路径、相对仓库根目录的路径和 `~/`。配置后直接运行 `./build_release.sh`，即可依次完成构建、DMG 打包、公证、装订票据和 Gatekeeper 验证。

也支持钥匙串凭据：首次在本机终端运行 `xcrun notarytool store-credentials loppi-notary`，按提示输入开发者 Apple ID、App 专用密码与 Team ID，然后执行：

```sh
APPLE_NOTARY_PROFILE=loppi-notary ./build_release.sh
```

`APPLE_NOTARY_PROFILE` 是钥匙串中的配置名，也可写入 `.env`；设置后优先使用钥匙串认证。缺少签名或公证配置时，发行入口在编译前退出。`scripts/build_macos.sh` 同样默认生成两架构开发 DMG；单独重新打包须显式传入 App 和输出 DMG 路径：`zsh scripts/package_macos.sh /absolute/loppi.app /absolute/output.dmg`，并自行传入签名、公证环境变量。内部未公证包可在该命令后加 `--dev`（放在两个路径之前）。

## 开发验证用云端配置

显式云端原型可以从仓库根目录 `.env` 读取 `DEEPSEEK_API_KEY` 和 `VOLCENGINE_API_KEY`，示例见 [`.env.example`](../.env.example)。不得将真实值写入文档、日志、测试结果或安装包；默认验证不使用真实供应商。正式客户端由用户在设置中配置凭据，并保存到系统安全存储。

## 许可与发行检查

项目许可在根目录 `LICENSE`，署名在 `NOTICE`。现有打包器会复制 `licenses/`，因此其中同时保留项目许可副本和第三方许可。维护者更新根目录许可或声明后，同步 `licenses/loppi-apache-2.0.txt`、`licenses/loppi-NOTICE.txt`，再核对最终安装包资源；当前已经发行的旧二进制不会因文档修改获得新内容。

发行前按[发布准备清单](open-source-readiness.md)完成源码、依赖安全、平台实测、包内许可和校验和检查。普通 Verify CI 不接收代码签名凭据；main 的 Release 工作流仅通过已配置的 GitHub Secrets 取得发行凭据，使用临时钥匙串和临时公证文件，结束后清理，详见[应用内更新与自动发行](updates.md)。真实录音不进入 CI。公开版本使用新版本号，保留旧标签和附件。
