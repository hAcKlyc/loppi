---
status: implemented
updated: 2026-11-15
---

# 0.1.27 依赖更新与候选验证

从远端 dev 的 6f2a1b7 建立 codex/release-0.1.27，逐条处理 #34–#48 的依赖升级。版本标识在候选分支统一为 0.1.27。只在候选分支整合、验证和记录，正式发行和主线合并另行决定。

## 执行台账

- 保留现有 Core/GUI/CLI/worker 边界、IPC 29、已保存数据和云请求合同。npm、Cargo 和已校验原生资源继续各自使用现有锁定入口。
- React 两个包一起升级，#39 已覆盖 #43。语音库同步 macOS/Windows 依赖及原生包归档。保持现有 ureq 2.x。#46 只替换依赖，未迁移 ASR HTTP 调用，Mac/Windows 均无法编译，扫描结果与基线一致。构建 Action 分组核查运行平台、依赖产物交接和发行侧兼容性。
- 验收以最终组合态为准。用本机实机运行隔离的候选 App，确认版本身份、真实 WebView 与本地模型；使用托管 macOS 和 Windows 检查验证跨平台实现。只记录实际取得的硬件和安装证据。
- 运行锁定依赖和发行产物相关检查，核对现有安全扫描基线，审查后更新候选分支并处理原 PR。
- 基线 6f2a1b700d5023447f06f6d860cfb15af47a87fb。生产范围限于依赖、兼容适配、构建验证和版本。现有工作区的草稿不在本次范围。

- 本机 macOS 26.6.2 / Apple Silicon 跑完 Verify 22 项。检查两份 sherpa 2.0.1 原生归档的发布来源和校验值，修复八个组件的随包版本和许可来源，核对现有更新器许可补充文件。

### 候选验证

- 候选源码 `0862177ee2dbc6d0b9dd0b011f31649bc5781736`。用实际版本锁核对了五个 Actions 的 SHA。三个独立审查完成，已修复随包的依赖许可版本与来源。
- [GitHub Verify](https://github.com/hAcKlyc/loppi/actions/runs/43619403300) 验证同一提交。Mac 22 项、Windows 10 项均通过，更新后的二进制构建输入交接通过。
- [依赖安全扫描](https://github.com/hAcKlyc/loppi/actions/runs/43619403560) 与 [dev 基线](https://github.com/hAcKlyc/loppi/actions/runs/42685509545) 公告一致。两项 npm 高危、11 项 Rust 公告继续存在，凭据扫描通过。
- 两份 Mac 开发 DMG 均构建、挂载验证并通过校验和。Apple Silicon 原生运行及 Intel 包 Rosetta 运行均载入随包离线模型，四组件身份一致。Apple Silicon 上从 DMG 拷出的 App 实际打开时间线，进入设置并显示 0.1.27，合成语音离线识别通过。实际导出页可打开并取消 macOS 原生保存对话框。单独隔离目录试录电脑声 5 秒（240000 帧），落盘并产生对应粗转，没有麦克风会话或真实用户数据。
- [Windows 安装包与安装验证](https://github.com/hAcKlyc/loppi/actions/runs/43619786928) 用托管 x64 Windows 构建相同源码。首次安装和模型加载通过，卸载程序已返回但即时检查仍看到 GUI 文件。使用同一 SHA256 安装包 [复跑](https://github.com/hAcKlyc/loppi/actions/runs/43621714271)，走正常退出并等待文件移除后，安装、运行库 2.0.1、三类随包模型就绪、程序卸载与测试数据库保留全部通过。独立验证分支的工作流不进入候选源码。

- 原 PR 的处置为 #34–#42、#44–#45、#47–#48 合批整合到候选源码，#43 重复，#46 本轮废弃。按候选提交结案 GitHub 原提案。
- 当前交付为分支候选。Intel Mac 通过 Rosetta 测试，Windows 通过托管虚拟机测试；未在 Intel Mac 或 Windows 物理设备上跑交互和录音，也未执行正式发行的签名、公证和发布。Apple Silicon 上运行 Intel 候选的 Rosetta 提醒不涉及已安装的 ARM 版。
