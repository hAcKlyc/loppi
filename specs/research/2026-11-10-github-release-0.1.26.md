# 0.1.26 三平台正式发行

2026-11-10 03:37（北京时间），v0.1.26 已公开并设为 Latest。本版提供策略结果实时输出、长结果全文展示、智能分析设置即时应用和事件详情菜单修复。[发行页](https://github.com/hAcKlyc/loppi/releases/tag/v0.1.26)

## 来源与验证

- 发行源码、main 和标签解析提交均为 `2f084c28c1bafa6f4bce5a5168263fdcac912ffa`。三份报告中的 GUI、Core、worker、CLI 均为 0.1.26、协议 29、同一源码、`dirty=false`。
- dev 提交 `1100fae68418086d6beebff4ee97acf31c01f181` 与精确 main 提交的 Verify 均通过。四份报告分别确认每个提交的 Mac 22 项、Windows 10 项通过。GitHub Actions 服务异常期间的启动失败和缺失 Windows 任务由同一 dev 提交的完整重跑关闭。[dev Verify](https://github.com/hAcKlyc/loppi/actions/runs/42675464300) · [main Verify](https://github.com/hAcKlyc/loppi/actions/runs/42676985869) · [GitHub 事故](https://stspg.io/7yw63xl9ztrv)
- 三平台构建首轮通过。发布任务创建内部草稿后首次回读失败，核对草稿和标签源码后仅重跑发布任务，复用首轮的三个产物。第二次发布通过文件、签名和匿名清单校验。[Release 42676985694](https://github.com/hAcKlyc/loppi/actions/runs/42676985694)
- 独立复核确认公开版是 Latest，13 个附件与报告完整，SHA256SUMS 与 GitHub 附件摘要一致。匿名固定地址读回 0.1.26，包含 `darwin-aarch64`、`darwin-x86_64` 和 `windows-x86_64`，各平台的版本化下载地址和签名与公开附件一致。[固定更新清单](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json) · [复核证据](evidence/2026-11-10-release-0.1.26/published-release-2f084c2.json)
- 独立 npm 审计仍报告 2 项高危公告。Rust 审计在上一版报告的 10 项上多报了 `libsqlite3-sys` 的一项公告，锁定的 `libsqlite3-sys` 仍为 0.38.2。该公告的触发条件是打开受较低信任方控制的数据库和配套 WAL。凭据扫描为零项发现。[审计结果](https://github.com/hAcKlyc/loppi/actions/runs/42676985856) · [RustSec 公告](https://github.com/RustSec/advisory-db/blob/main/crates/libsqlite3-sys/RUSTSEC-2026-0459.md)

## 安装包

| 平台 | 字节数 | SHA-256 |
| --- | ---: | --- |
| [Mac Apple Silicon](https://github.com/hAcKlyc/loppi/releases/download/v0.1.26/loppi_0.1.26_macos-arm64.dmg) | 184,745,309 | `425766bd632e75b1e545f39ce7599ddd80219280613da32ce7b684a7384981b1` |
| [Mac Intel](https://github.com/hAcKlyc/loppi/releases/download/v0.1.26/loppi_0.1.26_macos-x86_64.dmg) | 187,657,687 | `d7197d4fb301557af2fe72c56a0dd3328c6165da870aad776d6ec79822034565` |
| [Windows x64](https://github.com/hAcKlyc/loppi/releases/download/v0.1.26/loppi_0.1.26_x64-setup.exe) | 390,258,390 | `bf7359446753c5cbaff43dc3dc5d55c1fbf30dc099b0387bfa13514b50cb614c` |

两款 Mac 的 App 和 DMG 均完成 Developer ID 签名、Apple 公证、装订与票据校验。三个平台的 Tauri 更新签名均通过发布校验，Windows 安装器报告的 Authenticode 状态为 `unsigned`。[Apple Silicon 构建](https://github.com/hAcKlyc/loppi/actions/runs/42676985694/job/129366469953) · [Intel Mac 构建](https://github.com/hAcKlyc/loppi/actions/runs/42676985694/job/129366470017) · [Windows 构建](https://github.com/hAcKlyc/loppi/actions/runs/42676985694/job/129366470057)

## 验收边界

- Mac 本地以正式 v0.1.25 Core 和该 dev 提交的 v0.1.26 Core 验证隔离数据的协议交接、标签和策略配置及修订号读取，以及退出。WebKit 交互验证覆盖两个窗口宽度、策略结果增量和全文展示、菜单与键盘焦点。模型流式输出由可控流夹具验证。
- Windows 由托管验证和正式构建覆盖，安装器原地升级和长时录音未做实机验证。录音权限按钮位置的草稿保留在原工作区。
