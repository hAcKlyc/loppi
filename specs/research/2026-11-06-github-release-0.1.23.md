# 0.1.23 三平台正式发行

2026-11-06（北京时间），[v0.1.23](https://github.com/hAcKlyc/loppi/releases/tag/v0.1.23)已公开并设为 Latest。本版补齐 macOS 完整唤醒通知遗漏后的采集恢复，去掉关闭开关的描边，统一统计金额为两位小数，并将智能识别服务卡片改为左侧开关、右侧齿轮设置。

## 来源与验证

- 发行源码及 tag 解析提交：`5a04a9de12c1e343373af073afaf5a035ba2f7a4`。三平台报告均为版本 0.1.23、同一 source、dirty=false；GUI/Core/worker/CLI 身份一致，协议 26 未改变。
- dev `5579ea8634614a4ff891c317407b48eda322f4f8` 的 [Verify 42071097062](https://github.com/hAcKlyc/loppi/actions/runs/42071097062)和精确 main 的 [Verify 42072696399](https://github.com/hAcKlyc/loppi/actions/runs/42072696399)均通过 Mac/Windows 检查；发行准备的本地完整 `npm run verify` 通过。
- [Release 42072696406](https://github.com/hAcKlyc/loppi/actions/runs/42072696406)最终 attempt 2 成功。首次三平台构建均成功，发布创建空草稿后立即回读报 Release not found；确认草稿存在且绑定正确源码后，仅重跑失败的 publish job。没有修改门禁、重打标签或覆盖已公开附件。
- 发布 job 完整验证三平台二进制哈希、更新签名、版本绑定及报告后公开全部附件。独立只读复核确认 Latest、13 个附件、tag source、三平台组件身份；SHA256SUMS 与 GitHub 附件摘要一致，匿名 latest.json、公开报告和签名文件的字节摘要均匹配，三平台 URL/签名与报告一致。

## 安装包

| 平台 | 字节数 | SHA-256 |
| --- | ---: | --- |
| [Mac Apple Silicon](https://github.com/hAcKlyc/loppi/releases/download/v0.1.23/loppi_0.1.23_macos-arm64.dmg) | 184,032,860 | `7abc66902239a8dd9a80e6776079b96278259139b676e5dda8861197213f495f` |
| [Mac Intel](https://github.com/hAcKlyc/loppi/releases/download/v0.1.23/loppi_0.1.23_macos-x86_64.dmg) | 187,611,277 | `5e394763eecc38dbb3706b22f97748c4f0844d4b1ead4e8bf279c47693984151` |
| [Windows x64](https://github.com/hAcKlyc/loppi/releases/download/v0.1.23/loppi_0.1.23_x64-setup.exe) | 389,866,904 | `48f5a44d45723bbe5532923e3b793d490542fe5740a0ee56a8095894db91d731` |

两 Mac 的 App 与 DMG 均获得 Apple 公证 Accepted，装订和校验成功，Gatekeeper 识别为 Notarized Developer ID。Windows 构建报告仍为 Authenticode unsigned；三平台 Tauri 更新签名均通过发布校验。

[固定更新地址](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json)已匿名读取并确认版本 0.1.23，包含 darwin-aarch64、darwin-x86_64、windows-x86_64。已有支持更新的客户端可通过应用内更新取得本版；本轮未替换维护者本机安装。

## 验收边界

用户试用本次开发包后明确反馈“我测试了，没有问题了”，并要求更新版本直接上线。代码模拟、三路独立审查、本地完整验证及用户验收见[交付台账](../prd/devplan_261105_wake-recovery.md)。未把概括反馈扩写为逐项真实 DarkWake、睡眠后音频落盘、跨平台安装升级实机矩阵证据；这些细分场景继续保持未验证。原生兜底依赖 Apple 实现的动态能力状态键；读取不可用或格式不符时保守维持睡眠态。

完整公开元数据、平台报告和公证日志摘要见[发行证据](evidence/2026-11-06-release-0.1.23/published-release-5a04a9d.json)。文档提交不改变发行二进制来源。
