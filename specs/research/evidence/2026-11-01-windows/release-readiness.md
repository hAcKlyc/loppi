# 0.1.18 跨平台发行准备状态

**尚未发布，台账保持 in-progress。** 三端安装包、源码审查与可在锁屏条件下完成的真实验收已通过；剩余真实桌面与 OS 入口测试需要用户解锁 Windows。现有 Release、标签和 main 保留原状态。

## 已验证的最终包

二进制统一来自干净源码 `30a04cc7879ee69f191b53c744da7483cd48c0e0`，版本均为 0.1.18。后续只记录证据的提交不改变这份二进制来源，不将文档提交号冒充内置身份。

| 安装包 | SHA256 | 状态 |
| --- | --- | --- |
| `loppi_0.1.18_macos-arm64.dmg` | `6940371650eceda66ccde7e7e28500fd762abece08a7b7dfd96649d3b4d94c72` | Developer ID、Apple 公证/装订、Gatekeeper；ARM 本机检查 |
| `loppi_0.1.18_macos-x86_64.dmg` | `4fb0cb990b7d4194dfaa92afcbdfdd6fbf109279a773eb729b90d57faeb25345` | 同上；Intel 通过 Rosetta 检查，未在 Intel 硬件验收 |
| `loppi_0.1.18_x64-setup.exe` | `ef3b8f66ee206a2e7c852a0d9a6ac6a6952336f96c2f658ef077523249d4c533` | 388800577 字节；四组件 epoch1793489277；未做 Authenticode 签名 |

Mac 包、三份待发布 build report 和合并校验清单位于 ignored 的 `build/releases/0.1.18/source-30a04cc7879ee69f191b53c744da7483cd48c0e0/`。Windows EXE 保留在 Windows 工作区的 `target/x86_64-pc-windows-msvc/release/bundle/nsis/`；Mac 尚未下载该 EXE，也未上传 Release。两份报告分别见 [Mac 最终包验收](macos-final-packages.json)与 [Windows 完整证据](windows-final-evidence.json)。

Windows 证据由公开 MyAgents CLI 回传，56654 字节、SHA256 `0941141d626d6a561044361cba75085130e838231227e61136e1b497da966f0d`。18 个 JSON 对象保留真实源码归属；`original_bytes/original_sha256`属于 Windows 未脱敏原文件，不是脱敏后 JSON 的序列化哈希。root 已核对传输摘要、身份、源模型锁、图标 PNG 字节、前后两轮新采集结果及各验收出口。

## 真实验收边界

- 完整 Windows 离线入口在 `0fa6e0e` 运行退出0：10项检查，Rust426通过/22原有opt-in忽略，Node89通过/1原有Apple芯片检查跳过，Python8通过/1原有Mac适配器跳过，兔子资源3项通过。之后仅改 Windows 安装/卸载器图标配置和事实记录；未重贴执行版本或机械重跑不受影响检查。
- `30a04cc` 实际正式构建、默认安装、四组件身份、52资源哈希、19PE普通/延迟依赖闭包通过。GUI、安装器及实际写出的卸载器均含批准ICO的七帧PNG，原PNG/ICNS未改。NSIS壳是标准x86引导程序，四个应用组件为x64。
- 最终包真实双路采集、持久VAD/X-ASR和CLI读取通过；录制中覆盖安装后重新启动，产生不同ID和更晚时间的新粗转；随后双路录制中卸载。SQLite、媒体、合成笔记及非安装文件保留。GUI升级/卸载退出码为0；Core已停止并释放文件，该轮未单独捕获其退出码，保持null。
- Unicode/emoji/空格路径完整流程及缺失/损坏模型负向在`0fa`通过。缺失为`model_bundle_missing`、损坏为`model_checksum`，start均退出2/`model_not_ready`；原权重未改，不伪造可录制状态。
- 完整最终NSIS展开只有两处`RmGetList`，无Shutdown/Kill。小型实际占用负向证明非零拒绝且进程存活；正常退出后放行。旧dirty包一次报错原因未复现/未知；最终两个完整同序列成功，不捏造旧根因。
- Windows最终应用保留在`%LOCALAPPDATA%\loppi`；默认Core合成资料已归档，无测试产品进程、测试凭据或合成首次启动数据，开机启动关闭。此前GUI偏好及WebView资料均隔离。Mac原安装仍运行，当前录音未被重启打断。

## 解锁后必须完成

1. 使用合成全屏内容覆盖私有桌面，验证真实Core GDI截图、该图的OCR及锁屏到解锁后的恢复。
2. 同一GUI PID的HWND排除、隐藏和恢复；不能把合成PNG OCR或WebView内部图当成此项。
3. OS实际托盘点击/键盘菜单、悬浮输入及`--login`呈现；既有Win32消息/几何检查不冒充物理输入。

保持原Windows Agent Session与已安装30a运行时即可续测，无需重建未改变的包、重跑其它已通过检查或再次取得发布授权。用户本轮已经明确授权发布；缺的是实际解锁条件。

[最后只读复核](unlock-last-check.json)为2026-10-31 23:57:38 UTC：进程与活动控制台均Session1，WTS flags0、Default桌面，Windows11仍锁定；未尝试解锁、截图或更改安全设置。

## 随后的发布动作

解锁门槛关闭后，ff同步main，保存并核对旧标签OID `b093afbca1dfa414d658736dafe64f834478aef1`，用精确lease把v0.1.18指向二进制源码30a。原标签/Release与两Mac包备份在`build/backups/v0.1.18-before-windows/`。统一替换现有Release ID430124887的Mac两包/报告、加入Windows包/报告和三包SHA256SUMS；Windows上传须由root明确给现有Agent最后指令，不创建额外Release。上传后独立核对远端大小/摘要，并下载Windows EXE复核实际SHA，再写最终状态收尾提交。Windows签名状态必须仍如实为unsigned。

仓库简介已更新为macOS和Windows；可见性和许可证未变。其它设备、Windows ARM64/Windows10、真实付费云API、最低版本实机与多日稳定性不在本次已验证范围内。
