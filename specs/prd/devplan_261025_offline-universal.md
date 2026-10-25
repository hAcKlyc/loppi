---
status: implemented
created: 2026-10-25
updated: 2026-10-26
---

# 随包离线模型与 Universal Mac

用户已接受[架构方案](../research/2026-10-25-offline-assets-macos-build-design.md)，随后要求“先别搞那么复杂，简单点，跳过这些流程”。本执行记录据此缩减，覆盖此前方案中的流程门槛，不再引入额外评审、CI 或全量评测。

## 本次交付

- 0.1.8：固定 X-ASR、Silero、CampPlus 随 Core 打包，模型只保留一份。
- Core 从自身 bundle 读取只读资源，自动校验与加载，重试只重新检查，不下载、不复制到用户目录。
- macOS ARM / Intel / Universal2 共用简洁构建入口，默认 Universal2；沿用现有签名、公证与 DMG 工具。Windows 不在本次范围。
- 必要验证：受影响代码编译、资源校验与模型加载、双架构检查、签名公证与实际包检查。
- 历史音频留本机。用户允许 Intel 未来用公开音频验收；本次按最新要求跳过 CI 和完整重跑，明确记录 Intel 原生及 macOS 13 实机尚未验证。
- 开发使用独立工作树，保留主工作区其他会话改动；不替换本机安装、不发布远端 release。

## 进展

- [x] 独立工作树、固定模型权重及 Universal 原生库校验。
- [x] Core 随包资源与界面。
- [x] 构建打包脚本与版本更新。
- [x] 必要验证与本地发行包。
- [x] 文档、提交与交付。

必要检查：资源定位/缺失/损坏/清单 5 项单测、并发重检 1 项单测、前端类型与受影响文件格式、onboarding 3 项测试通过。Universal 内部候选四组件双 slice、身份一致、最低 OS 声明、只读且重命名路径的 Core 自动加载和重检通过。公开样例 `test_wavs/0.wav` 在新 ARM slice 与既有 X-ASR worker 的原始转写、人声检测完全一致，192 维声纹最大差异为 0；这是一段烟测，不替代原 20 组准确率或能耗报告。原生运行时实际版本 sherpa-onnx 2.0.0 / ONNX Runtime 1.28.2，CPU。

签名公证包将在源码提交后从同一干净工作树生成；本机安装保持不变。最终包路径、体积及校验和完成后记录。

## 交付（2026-10-26）

源码 `94fa7a5785d4f2d88029c3593b23473e4601368e`；四组件均为干净 0.1.8 / protocol 20，ARM 与 Intel slice 一致，模型只保留一份。App 文件共 279,596,395 字节，DMG 199,754,527 字节（199.75 MB / 190.50 MiB）。

本地包：`build/releases/0.1.8/macos-universal2/hear_0.1.8_macos-universal2.dmg`。最终 SHA256：`6e70d83ab7a64dd468925ce27c376f519541f3c5397558cd92ee38e25b9bbfd3`。Apple 公证 Accepted（`8a644caf-3fe9-48e2-a8de-afeff507ccf7`），装订、Gatekeeper、实际 DMG 只读挂载/签名/内容比对及随包 Core 自动加载与重检全部通过。没有替换本机安装，没有远端发布。

机器可读交付记录：[release receipt](../research/2026-10-26-offline-universal-release.json)。日志与公开样例检查证据保留于 `build/offline-universal/`；资源缓存保留于 `build/cache/`，以后可以直接复用。主工作区其他会话的 9 份 tracked 改动未修改。
