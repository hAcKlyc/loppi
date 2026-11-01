# 应用内更新与 GitHub Releases

本次源码接入 macOS Apple Silicon、macOS Intel 与 Windows x64 更新。正式发行及真机验收状态以 [执行台账](../specs/prd/devplan_261102_github-updater.md) 为准；源码接入不代表已发行版本已经具备更新功能。

应用主窗口启动 10 秒后检查一次，设置 → 通用 → 软件更新也可以手动检查。检查失败不会弹窗打断录音，设置中显示反馈并允许重试。发现新版后静默下载并验签，不弹更新窗口。准备好后只在左下角侧边栏显示绿色「重启更新」按钮，点击后直接安装重启，不再联网下载。下载及签名验证完成前不停止录音。安装前通过既有 GUI/Core 会话屏障正常封存录音、退出 Core 并等待 owner 锁释放；失败不安装，也不强杀。安装失败恢复 GUI 连接能力，再由既有 AppOpened 激活流程恢复正常运行。更新成功重启后仍遵守原录音意图、暂停、来源、定时与权限边界。

检查地址固定为 [GitHub latest.json](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json)。已验签更新包只保留在当前宿主会话的匿名临时文件中，避免常驻内存占用，进程退出后由系统释放。没有自建更新服务器、资源代理、定期轮询或跨重启下载缓存。首次提供更新功能之前的旧版本需手动安装一次新版本。

## 签名密钥

Tauri 更新签名与 Apple Developer ID / Windows Authenticode 独立。公钥已写入 `src-tauri/tauri.conf.json`；本次生成的 loppi 专用私钥位于维护者机器 `~/.config/loppi/updater.key`，不在 Git 中。务必在安全位置备份私钥，后续版本继续用同一密钥；不要重新生成公钥导致已有客户端无法升级。

构建机使用如下环境变量，或写入根目录已忽略的 `.env`（环境变量优先）：

```dotenv
TAURI_SIGNING_PRIVATE_KEY=/absolute/path/to/updater.key
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
```

`TAURI_SIGNING_PRIVATE_KEY` 也可直接提供密钥内容，适合 CI Secret。Windows 构建机使用该机器自己的安全文件路径，或注入同一私钥内容；不要提交私钥或在日志中输出。开发候选不要求更新签名，不产生可发布更新清单。发行入口缺少私钥立即失败；签名后还会用配置中的公钥核对最终文件与签名中的版本。

## 构建更新资源

仍使用 [开发指南](development.md) 中的正式构建入口，必须是干净、已提交源码。为新的正式版本同时递增所有现有版本源；不要用新产物覆盖已公开的旧版本。

```sh
./build_release.sh
# Windows 11 x64 上执行 ./build_release.ps1
```

Mac 会完成最终 App 的签名、公证和票据装订，再制作 DMG 和 `.app.tar.gz` 更新归档；DMG 继续供首次安装使用。Windows 复用最终 NSIS `-setup.exe`，Tauri 更新签名不改变其 Authenticode 状态。各目录位于 `build/releases/<version>/<platform>/`，含更新资源、`.sig`、`updater.json`、构建报告及校验和。

跨平台构建必须来自同一提交和版本。将三个平台完整目录归集到一台机器，先准备面向用户的更新说明文件，再生成清单：

```sh
node scripts/publish_update.mjs /absolute/release-notes.txt \
  build/releases/<version>/macos-arm64/updater.json \
  build/releases/<version>/macos-x86_64/updater.json \
  build/releases/<version>/windows-x64/updater.json
```

脚本核对三个平台齐全、同一源码/版本、文件大小/哈希、GitHub 版本化地址、更新签名与签名中的版本，生成 `build/releases/<version>/latest.json`。平台键为 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`；签名字段是 `.sig` 文件内容。客户端也启用了签名版本绑定，不能将旧的已签名包配上新版本号来安装。

## 上传和发布

先按仓库发行流程建立 `v<version>` 标签并推送，创建对应的 GitHub **草稿 Release**。标签必须解析到构建源码提交。完成三平台验收后，使用已登录的 GitHub CLI 上传：

```sh
node scripts/publish_update.mjs --upload-draft /absolute/release-notes.txt \
  build/releases/<version>/macos-arm64/updater.json \
  build/releases/<version>/macos-x86_64/updater.json \
  build/releases/<version>/windows-x64/updater.json
```

该入口仅上传到已有草稿，不创建或修改公开版本，不自动发布，也不覆盖同名附件。首次安装包、构建报告和校验和继续按仓库现有发行流程上传。全部附件完整并关闭实机门槛后再发布草稿、设为 Latest，客户端才会通过固定地址看到更新。后续版本同样保留附件名 `latest.json`，其内部下载 URL 固定到本次 `v<version>`。

## 验证

`scripts/test_updater_artifacts.mjs` 使用临时私钥及真实 Tauri CLI 签名验证清单生成；`scripts/test_software_update.mjs` 使用真实 React 组件与模拟 IPC 检查静默下载、侧边栏按钮、直接安装及重试；Rust updater 测试覆盖退出/安装失败的顺序与恢复，既有生命周期烟测覆盖关闭屏障后重新检查 Core。`scripts/test_software_update.html` 是合成数据交互预览。

正式升级需在两个系统上额外验证：录音时点击重启更新后尾块可回放、旧 Core/worker 完全退出、新版组件身份一致、原意图及暂停配置保留；Windows 验证原安装目录、安装器退出门禁及自动重启；Mac 验证签名、公证票据和首次系统校验。失败或取消不得强杀后台或删除用户数据。这些实机检查不能由模拟测试替代。
