# 0.1.22 三平台正式发行

2026-11-04（北京时间），[v0.1.22](https://github.com/hAcKlyc/loppi/releases/tag/v0.1.22)已公开并设为 Latest。本版修复登录后台接管竞争、设置刷新跳动，以及精转完成被历史录音缺口覆盖的问题。

## 来源与验证

- 发行源码及 tag 解析提交：`7cc63e9e98f3785802f96fa15ef2c6eba05c1185`。三平台报告均为版本 0.1.22、同一 source、dirty=false；GUI/Core/worker/CLI 身份一致，协议 26 未改变。
- dev `b377c357e2085995c3ac774a4ccd255170dcfae2` 的 [Verify 41686285922](https://github.com/hAcKlyc/loppi/actions/runs/41686285922)及精确 main 的 [Verify 41688013520](https://github.com/hAcKlyc/loppi/actions/runs/41688013520)均通过 Mac/Windows 检查。
- [Release 41688013693](https://github.com/hAcKlyc/loppi/actions/runs/41688013693)最终 attempt 3 成功。首次 Intel 在 Apple 公证 Accepted 后遇到 CloudKit -1005 / Error 68 票据装订网络错误；仅重跑失败任务。第二轮三平台完成，发布创建标签和空草稿后立即回读报 Release not found；确认草稿存在后仅重跑发布，第三轮成功。没有修改签名校验、重打标签、混入其它源码或覆盖旧公开版本。
- 发布 job 完整验证三平台二进制哈希、更新签名、版本绑定及报告后公开全部附件。独立只读复核确认 Latest、13 个附件、tag source、三平台组件身份；SHA256SUMS 与 GitHub 附件摘要一致，匿名 latest.json 字节摘要及平台内容与报告一致。

## 安装包

| 平台 | 字节数 | SHA-256 |
| --- | ---: | --- |
| [Mac Apple Silicon](https://github.com/hAcKlyc/loppi/releases/download/v0.1.22/loppi_0.1.22_macos-arm64.dmg) | 184,829,556 | `3b663fa5f71d492b2eb28269d9b8d1e7baa2baeab30d3e6c190bd1b5206eb073` |
| [Mac Intel](https://github.com/hAcKlyc/loppi/releases/download/v0.1.22/loppi_0.1.22_macos-x86_64.dmg) | 186,952,531 | `100ec7cc44273eaef95b48aed56eb76284583828366252faf48d4dcd48dd5718` |
| [Windows x64](https://github.com/hAcKlyc/loppi/releases/download/v0.1.22/loppi_0.1.22_x64-setup.exe) | 389,891,270 | `0f42030e8f37fae393a4df90eccd036a8abbe4a4ca8c7b4e2c3161431554e880` |

两 Mac 的 App 与 DMG 均获得 Apple 公证 Accepted，装订和独立票据校验成功，Gatekeeper 识别为 Notarized Developer ID。Windows 构建报告仍为 Authenticode unsigned；三平台 Tauri 更新签名均通过发布校验。

[固定更新地址](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json)已匿名读取并确认版本 0.1.22，包含 darwin-aarch64、darwin-x86_64、windows-x86_64。已有 0.1.21 客户端可通过应用内更新取得本版；本轮未替换维护者本机安装。

## 验收边界

用户在了解既有验收限制后明确要求提交、关闭 #49 并发布。已完成的独立审查、离线验证及真实隔离采集/暂停/声源关闭/进程崩溃重启/定时边界检查见[问题台账](../prd/devplan_261104_issue-49.md)和[登录接管台账](../prd/devplan_261104_login-handoff.md)。真实睡眠/DarkWake、设备暂不可用后的恢复、真实云端自动收尾、更多设备与本版跨版本安装实机矩阵未完成；本次发布不将其标成已通过。0.1.21 的 Windows 用户安装验收仅保留历史归属，不等同于 0.1.22 实机验收。

完整公开元数据、平台报告和公证日志摘要见[发行证据](evidence/2026-11-04-release-0.1.22/published-release-7cc63e9.json)。文档提交不改变发行二进制来源。
