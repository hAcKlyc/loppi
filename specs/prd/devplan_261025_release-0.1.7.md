---
status: implemented
created: 2026-10-25
updated: 2026-10-25
---

# 0.1.7：X-ASR客户端release安装包

用户在确认X-ASR源码切换后要求更新版本并打release包。当前共享工作区版本为0.1.6，本次升至0.1.7，按已提交X-ASR源码制作Apple Silicon macOS签名、公证DMG。交付可安装包及来源/校验记录；不安装或重启当前客户端，不推送GitHub release。

## 执行台账

### 开发契约

- 原子交付：npm/Tauri/Rust workspace与锁文件/helper版本对齐、三路review、本地版本提交、干净已提交源码release构建、Developer ID签名、Apple公证/装订/Gatekeeper、DMG及校验记录一起完成。
- Batch base：`d7deb5e61ef6595ba097c7451077df20f1654d1a`。仅拥有Cargo.toml、Cargo.lock、package.json、package-lock.json、src-tauri/Cargo.toml、src-tauri/tauri.conf.json、src-tauri/native/HearCore-Info.plist与本台账/本次release记录。
- 共享版本文件当前仅含0.1.5到0.1.6的机械改动，用户此次版本更新授权将其统一推进到0.1.7，helper build从5升7；其它会话功能改动不纳入源码提交或release。原工作区保持其它未提交源码字节不变，构建在独立受管worktree进行。
- 必须保证：GUI/CLI/Core/worker实际身份均为同一0.1.7、Git SHA、epoch且dirty=false；包内为已提交的X-ASR固定模型实现与精确sherpa-onnx2.0.0，不混入旧worker。模型权重仍经既有PrepareModels下载，不打包用户音频/数据库/凭据/模型缓存。
- 状态来源与生命周期：版本来自既有清单及helper plist；构建身份来自hear-protocol/build.rs与verify_build_identity.py；签名/公证以实际DMG及Apple回执为准。保留旧包，不触及活跃用户数据。
- 复用：build_release.sh、scripts/build_macos.sh、package_macos.sh、verify.py、verify_build_identity.py及既有本机签名/公证配置。新增owner/store/协议/重试/fallback机制为零；不修改构建脚本或运行逻辑。
- 本次负责：机械版本一致性、包的可复现身份和实际分发校验。不是重新开发/审查共享未提交功能、重跑模型排行、跨设备/多小时/续航验证、自动安装或远端发布。
- 架构：遵循现有Core/worker边界及完整包构建；所有版本清单为同一发行号，不增加运行时开关或架构机制。

### 验收与结束条件

- 对齐七个版本/锁文件且依赖解析不变，检查所有workspace包和helper build；保全其它共享文件。
- 版本候选共同static检查、必要构建检查通过。功能源码与上轮已审查候选相同，沿用已有682段/VAD/声纹/安装回归，不重跑未受影响ASR或云服务。
- fresh requirements/adversarial审查固定版本候选，主Agent裁决后fresh architecture PASS；review覆盖版本和打包契约，实际DMG身份/签名/公证在提交后构建时关闭，未完成前不标implemented。
- 从版本提交的干净worktree执行既有完整release流程，检查包内四组件身份与helper plist；签名、公证Accepted、票据、Gatekeeper、只读挂载内容全部通过。
- 在主工作区build/releases/0.1.7保存DMG、SHA256SUMS、release-info.json及原始构建/验证日志。完整核对台账后提交状态收尾。只有文档收尾时不重编实际包；release-info绑定实际构建提交。
- 版本/依赖改变重跑静态与构建身份，打包脚本改变才重审脚本，业务源码改变则重新评估本契约；纯记录不重跑音频或构建。

### 行动清单

- [x] 核实当前版本、release脚本、凭据读取边界及已提交X-ASR来源。
- [x] 统一0.1.7及helper build 7，静态检查并保全共享改动。
- [x] 三路独立审查、裁决与版本提交。
- [x] 干净源码构建签名DMG、公证/装订/Gatekeeper及实际产物核验。
- [x] 保存交付包和校验信息、最终台账及本地提交收尾。

### Review Contract

Mode=repair；target为当前index本次七文件机械版本差异和本台账，base见上。范围外为其余共享改动、既有X-ASR实现与原始评测。必须保证和允许机制同开发契约；只由本次引入或显著放大的范围内缺陷阻塞。requirements/adversarial先审冻结候选，主Agent裁决后最终architecture；三路均required。review不提高为安装/录音/跨设备验收，不把提交后才生成的公证回执视为当前源码缺陷；该回执属于完成交付的后续硬门槛。

### 进展

- 2026-10-25：用户要求release；共享清单为0.1.6、最后已交付包0.1.5。选择0.1.7避免混淆现有开发版本，独立worktree从已提交X-ASR的d7deb5e构建，不包含其它未提交功能。现有release入口已支持逐层签名、公证及凭据安全读取，直接复用。
- 2026-10-25：版本同步自检捕获Cargo.lock中误替换的三项第三方版本，在提交前恢复原锁值；初次static失败回执保留于本机。结构化逐项比对确认最终仅五个本地Cargo包版本变化，npm外部依赖完全相同；其它9份共享功能文件SHA保全。已准备独立worktree和隔离写时复制release缓存，不复用可写的主工作区输出。
- 2026-10-25：最终static三项全部通过。fresh requirements、adversarial与最终architecture均PASS（release017_requirements/release017_adversarial/release017_architecture），独立确认8文件快照、仅本地发行版本变化与9份共享文件保全；主Agent裁决无阻塞/修复项。进入版本提交和干净源码构建，实际产物门槛仍待完成。
- 2026-10-25：版本提交`4d2c5eb`后共同static再次通过；从该干净worktree执行既有完整release成功。四组件均为0.1.7、同一Git SHA/epoch、dirty=false，helper build7；实际签名Core隔离加载X-ASR/VAD到ready，无采集/业务数据/云任务。
- 2026-10-25：DMG公证Accepted（1da6f8d3-8770-43f6-96ad-be22b855e96b）、装订/Gatekeeper通过。独立只读挂载后10份内容与签名App哈希一致、四组件身份及arm64架构正确，无凭据/私人媒体/数据库/权重。交付包及SHA256SUMS/release-info/原始回执均保存在build/releases/0.1.7；DMG SHA256为d278e4ee05c38dbccf0fbbe109630376e4ea2a549ee8d3cc6d8d5a794763cdeb。全部本轮结束条件完成；发行记录见[0.1.7](../research/2026-10-25-release-0.1.7.md)，状态implemented。未安装、重启或远端发布；仅文档收尾，不改实际产物身份。
