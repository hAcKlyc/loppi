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

## GitHub Actions 正式发行

验收在 `dev`，正式发布在 `main`。`.github/workflows/release.yml` 对 main push 自动执行；手动重跑也只接受 main，不在 PR、dev 或 fork 中使用发行凭据。

1. 在 dev 准备新版本并同步现有 package、Cargo、Tauri、Plist 与锁文件，按上一公开版本更新带日期的 CHANGELOG，完成适用验收及该 dev SHA 的 Verify。
2. 合入并推送 main。Release 检查全部版本源，等待 **同一个 main SHA** 的 Verify 成功。已经公开的版本直接跳过，不重建、不覆盖。
3. Actions 使用 `macos-15`（ARM）、`macos-15-intel`（Intel）和 `windows-2025-vs2026`（x64）调用原有正式构建入口。Mac 完成 Developer ID 签名、公证和装订；三个平台用同一更新私钥签名。
4. 三个构建全部成功后，统一验证源码、版本、更新签名、Mac DMG、报告和文件哈希；生成完整附件及 latest.json。CI 此时才创建新版本的 annotated tag 和内部草稿，将全部附件上传并核对。
5. 自动公开 Release、设为 Latest，再以不携带鉴权的 HTTPS 请求核对固定 latest.json。main 不增加人工草稿确认步骤；内部草稿只是上传过程的中间态。

本地发布协作入口为 [merge-release](../.agents/skills/merge-release/SKILL.md)。用户要求「合并发布」后，Agent 负责 dev 版本/文档/验收、合入 main、跟踪精确 SHA 的 Release 并汇总实际结果；不要再抢先在本地创建公开空 Release。

### GitHub Secrets

在 `hAcKlyc/loppi` 的 Actions Secrets 中配置以下内容。当前工作仅提供仓库配置，没有上传这些凭据，也没有更改仓库可见性。

| 名称 | 内容 |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 已有 loppi 更新私钥的内容；三个平台使用同一把，不能重新生成替换已有公钥 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 该密钥的密码；无密码时可留空 |
| `APPLE_CERTIFICATE_P12` | 含 Developer ID Application 证书及对应私钥的 P12，转为 base64；只导出本应用需要的身份 |
| `APPLE_CERTIFICATE_PASSWORD` | P12 导出密码 |
| `APPLE_API_KEY` | App Store Connect API Key ID |
| `APPLE_API_ISSUER` | 团队 Key 的 Issuer ID；个人 Key 留空 |
| `APPLE_API_PRIVATE_KEY` | 对应 `.p8` 私钥的完整文本 |

P12 导入临时钥匙串，公证私钥只写入 runner 临时目录并在结束时移除；不会上传到 Actions artifacts 或 Release。仅最终发布 job 使用 contents:write，其余 job 只读。Windows 当前仍无 Authenticode 证书，构建报告继续如实标记 unsigned；Tauri 更新签名独立验证，不能称为 Windows 已完成代码签名。

公开更新要求仓库可被匿名访问。私有仓库也能使用 Actions，但当前固定更新地址没有私人访问 token；新版本的正式匿名发行会在私有状态下被阻止，不会由脚本自行公开仓库。GitHub 标准 runner 的可用标签见 [官方说明](https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/choose-the-runner-for-a-job)。

首次启用需要实际验证 runner 的 SDK/锁定资源及签名凭据：Mac 使用 Xcode 26.0.1；Windows 的 VC runtime 按 `config/windows-assets.json` 的版本和精确哈希检查。托管镜像不再提供该锁定版本时必须据实际来源更新并验证资源配置，流程会报错阻止发行，不会自动放宽哈希或混入其它版本。首次真实 Actions 尚未执行，不能把本地离线测试称为远端发行成功。

### 附件与重试

公开附件包括两份 Mac DMG、三平台更新资源及 `.sig`、按平台唯一命名的三个构建报告、`SHA256SUMS` 和 `latest.json`。全局校验和覆盖所有其它公开附件。

```sh
# 本地准备完整资源，原有参数形式继续有效
node scripts/publish_update.mjs /absolute/release-notes.txt \
  build/releases/<version>/macos-arm64/updater.json \
  build/releases/<version>/macos-x86_64/updater.json \
  build/releases/<version>/windows-x64/updater.json
```

输出在 `build/releases/<version>/github/`；清单另保留原来的 `build/releases/<version>/latest.json`。发布日期来自源码提交，重复准备不会随当前时间改变清单。每个平台目录需要完整的 `release-info.json`；Mac 还必须有报告中一致的 DMG。

本地辅助模式为 `--upload-draft`、`--verify-draft`、`--publish`、`--verify-published`，后面仍接说明文件和全部平台清单。默认只准备文件；远端写入模式要求干净源码。它们复用相同的完整资源验证，主要用于同一发行源码工作树的重试/只读复核，常规 main 发布由 Actions 统一负责。

重跑失败 job 时，已存在且大小/摘要完全一致的草稿附件会复用；不同字节或未知附件会阻止发行，不覆盖。如果重新构建导致同版本草稿字节变化，应明确处理未公开草稿或使用新版本，不能覆盖公开版本。若 Release 已公开而最后匿名地址检查因网络/传播失败，用相同 source 和该 run 的原平台资源执行 `--verify-published`；不删除已公开版本来重试。

## 验证

`scripts/test_updater_artifacts.mjs` 使用临时私钥及真实 Tauri CLI 签名验证清单生成；`scripts/test_software_update.mjs` 使用真实 React 组件与模拟 IPC 检查静默下载、侧边栏按钮、直接安装及重试；Rust updater 测试覆盖退出/安装失败的顺序与恢复，既有生命周期烟测覆盖关闭屏障后重新检查 Core。`scripts/test_software_update.html` 是合成数据交互预览。

正式升级需在两个系统上额外验证：录音时点击重启更新后尾块可回放、旧 Core/worker 完全退出、新版组件身份一致、原意图及暂停配置保留；Windows 验证原安装目录、安装器退出门禁及自动重启；Mac 验证签名、公证票据和首次系统校验。失败或取消不得强杀后台或删除用户数据。这些实机检查不能由模拟测试替代。
