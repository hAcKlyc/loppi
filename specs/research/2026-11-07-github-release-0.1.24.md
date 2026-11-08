# 0.1.24 三平台正式发行

2026-11-07（北京时间），[v0.1.24](https://github.com/hAcKlyc/loppi/releases/tag/v0.1.24)已公开并设为 Latest。本版缩短慢操作占用媒体锁的时间，让时间线主内容独立展示，减少播放时的正文重复处理与相同快照下的详情分页读取。

## 来源与验证

- 发行源码、main 与 tag 解析提交：`c6c7fd28744c89a04974c2ccad114ed93aa2bd11`。三平台报告均为 0.1.24、同一 source、dirty=false；GUI/Core/worker/CLI 身份一致，协议 26 未改变。
- dev `e1895a22dee3a0903139409a51c1df97f2f00fcf` 的 [Verify 42365751574](https://github.com/hAcKlyc/loppi/actions/runs/42365751574)和精确 main 的 [Verify 42366405190](https://github.com/hAcKlyc/loppi/actions/runs/42366405190)均通过。已下载核对四份报告：Mac 各 22 项、Windows 各 10 项检查通过；发行准备的本地完整 verify 同样通过。
- [Release 42366405186](https://github.com/hAcKlyc/loppi/actions/runs/42366405186)最终 attempt 3 成功。首次 Intel App/DMG 均获公证 Accepted，但 DMG 装订票据时 Apple CloudKit 连接中断（-1005 / exit 68）；仅重跑失败任务，保留 Apple Silicon 与 Windows 成功产物。第二次三平台构建全部通过，发布创建正确草稿后立即回读报 Release not found；确认草稿与 tag 对应正确源码后，仅重跑 publish。未修改门禁、发行代码、版本标签或公开附件。
- 发布 job 完整验证三平台文件哈希、更新签名与版本绑定后公开 13 个附件。独立只读复核确认 Latest、tag source、三平台组件身份；SHA256SUMS 与 GitHub 附件摘要一致，匿名 latest.json、报告和签名文件的字节摘要均匹配，三平台 URL/签名与报告一致。

## 安装包

| 平台 | 字节数 | SHA-256 |
| --- | ---: | --- |
| [Mac Apple Silicon](https://github.com/hAcKlyc/loppi/releases/download/v0.1.24/loppi_0.1.24_macos-arm64.dmg) | 184,772,184 | `e38a01a5f55e68e3b77a888428b541addbd0bf0218795ab29e9cf99ecc725b87` |
| [Mac Intel](https://github.com/hAcKlyc/loppi/releases/download/v0.1.24/loppi_0.1.24_macos-x86_64.dmg) | 186,941,096 | `aab11470034bb625227222c6a80af6d0760e2ddd1c9afbaba8546c177ff3e358` |
| [Windows x64](https://github.com/hAcKlyc/loppi/releases/download/v0.1.24/loppi_0.1.24_x64-setup.exe) | 389,788,793 | `6fd6bb014186db0cbfcccc1f160d8155329ba7ee8a85da6b96f10975465eea10` |

两 Mac 的 App 与 DMG 均获 Apple 公证 Accepted，票据装订与校验成功，Gatekeeper 识别为 Notarized Developer ID；随包模型及重命名 Unicode 路径检查通过。Windows 构建报告仍为 Authenticode unsigned；三平台 Tauri 更新签名均通过发布校验。

[固定更新地址](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json)已匿名读取并确认版本 0.1.24，包含 darwin-aarch64、darwin-x86_64、windows-x86_64。本轮没有替换维护者本机正在运行的应用。

## 验收边界

本批功能验证、六组真实组件浏览器场景、三路独立审查与隔离开发包证据见[交付台账](../prd/devplan_261107_smooth-core-and-detail.md)。用户在完成自测报告后要求合并发布；这不扩写为用户逐项设备实测。没有新增真实长时录音、能耗、慢盘或跨平台安装升级矩阵证据。

详情减少重复读取的收益依赖相同数据库快照；持续写入或读取连接变化时仍沿用完整刷新。导出最终重命名仍在既有媒体锁内，未宣称任意异常挂载均无等待。未增加服务、队列、重试器或恢复体系。

完整公开元数据、构建报告和日志摘要见[发行证据](evidence/2026-11-07-release-0.1.24/published-release-c6c7fd2.json)。文档提交不改变发行二进制来源。
