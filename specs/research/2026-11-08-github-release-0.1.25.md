# 0.1.25 三平台正式发行

2026-11-08 20:17（北京时间），[v0.1.25](https://github.com/hAcKlyc/loppi/releases/tag/v0.1.25) 已公开并设为 Latest。本版包含事件场景标签、自定义策略与生成结果、智能分析页面、按星期定时录制和相应 Agent CLI 功能。

## 来源与验证

- 发行源码、main 和标签解析提交均为 `3224eb7d31968929da4dcb0a3bbb41b25a8eb8bb`。三个报告及 GUI、Core、worker、CLI 均为 0.1.25、协议 28、同一源码、`dirty=false`。
- dev `75a4e31cf959c141ee298ec0bf63adf6965969e5` 的 [Verify 42473641986](https://github.com/hAcKlyc/loppi/actions/runs/42473641986) 与精确 main 的 [Verify 42474474608](https://github.com/hAcKlyc/loppi/actions/runs/42474474608) 均通过。四份报告分别确认 Mac 22 项、Windows 10 项通过。
- 托管 Mac 和 Windows 的上游推理库下载出现 HTTP 504。修复后由 Ubuntu 统一准备原有锁定输入，同次运行内只交接已校验的归档和模型。缓存按锁文件匹配，平台构建再次校验文件。三路独立复核、24 项定向测试、工作流静态检查和实机交接探针通过。 [跨平台交接验证](https://github.com/hAcKlyc/loppi/actions/runs/42473233445)
- [Release 42474474514](https://github.com/hAcKlyc/loppi/actions/runs/42474474514) 最终第 3 次运行成功。Intel App 首次获得公证后，装订票据时 Apple CloudKit 连接中断（-1005、退出 68）。仅重跑 Intel，App 和 DMG 的公证、装订与校验均通过。随后 GitHub 草稿创建后的首次回读失败，核对草稿和标签源码后仅重跑发布任务，复用三平台原产物。
- 发布任务验证三平台文件和更新签名后公开 13 个附件。独立只读复核确认 Latest、标签源码及三平台组件身份。SHA256SUMS 与 GitHub 附件摘要一致，下载的匿名清单、报告和签名文件摘要也一致。
- 独立 npm 和 Rust 依赖审计仍报告与 0.1.24 相同的公告集合。发布的依赖版本未改变，凭据扫描为零项发现。 [依赖和凭据审计](https://github.com/hAcKlyc/loppi/actions/runs/42474474534)

## 安装包

| 平台 | 字节数 | SHA-256 |
| --- | ---: | --- |
| [Mac Apple Silicon](https://github.com/hAcKlyc/loppi/releases/download/v0.1.25/loppi_0.1.25_macos-arm64.dmg) | 184,435,868 | `14f083b763872331e0a44e7d87cf11f44d43b167592854789cae03d46e70f839` |
| [Mac Intel](https://github.com/hAcKlyc/loppi/releases/download/v0.1.25/loppi_0.1.25_macos-x86_64.dmg) | 187,311,123 | `6da53b9b7c2cf23df570731ff9c4e7e7e2a838f2b8e57bdec82adfd00cbfa828` |
| [Windows x64](https://github.com/hAcKlyc/loppi/releases/download/v0.1.25/loppi_0.1.25_x64-setup.exe) | 389,999,342 | `13c3e45e2dca1410f14ed5a2187de32efd87c1e885e8f956bde62f44b812f703` |

两款 Mac 的 App 和 DMG 均完成 Developer ID 签名、Apple 公证及票据校验。三个平台的 Tauri 更新签名已通过发布校验，Windows 安装器报告的 Authenticode 状态仍为 `unsigned`。

[固定更新地址](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json)已匿名读回 0.1.25，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`，版本化下载地址和签名均与各平台报告一致。

## 验收边界

- 功能验收见[事件流程 PRD](../prd/prd_0.1.25_event-workflow.md)和[按星期录制台账](../prd/devplan_261108_recording-weekdays.md)。发布前在两台系统验证了隔离数据库迁移。Mac 基线是正式 0.1.24，Windows 基线是旧版 0.1.24 开发包。原录制时段和开关保留，星期默认全选。Windows 还验证了本地分钟边界、管理入口、草稿和原生开发安装包。
- 本次实机证据覆盖隔离数据库和开发包运行。跨平台安装器原地升级和长时录音仍属于扩展验收。录音权限按钮位置的本地草稿留在原工作区。

完整公开元数据、构建报告和日志摘要见[发行证据](evidence/2026-11-08-release-0.1.25/published-release-3224eb7.json)。文档收尾提交与发行二进制来源分别记录。
