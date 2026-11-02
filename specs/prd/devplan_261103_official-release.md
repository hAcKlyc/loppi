---
status: in-progress
updated: 2026-11-03
---

# 启用签名并正式发行0.1.21

用户要求配置自动发行签名并将0.1.21正式发布到GitHub，版本号不变；用户已确认当前版本验收通过，要求三平台正式发行。main已公开且Mac/Windows Verify成功；首次带Secrets发行暴露Windows OCR写死VS2022、而发行runner为VS2026的问题。本次完成发行配置、必要构建修复、准确说明与同源三平台发布，不混用旧本机包。

## 执行台账

### 开发契约

- 必赢场景：同一main源码的Mac ARM/Intel和Windows x64完整构建成功，Mac完成Developer ID签名/公证，三平台使用已有更新私钥签名；CI统一校验并发布v0.1.21与latest.json，匿名获取可用。
- 原子交付：仅本次发行必需的构建修复、正式更新说明、验证与发布事实；版本保持0.1.21，既有公开v0.1.18和附件不动。
- 必须保证：发行凭据仅GitHub Actions Secret/临时文件；复用原更新私钥；三平台同源/同版/签名与字节匹配；不跳过精确main Verify、哈希、许可或签名检查；Windows系统代码签名仍明确为unsigned。
- 明确不做：升级业务依赖、伪造真机验收、替换已公开附件、旋转更新公钥、变更业务架构、不经证据自动选择/放宽微软运行库哈希。
- 本次负责的问题：首次真实Actions暴露的构建工具配置与已锁定资源可用性，以及正式发行说明与实际状态不符。
- 状态来源：源码/版本来自Git；凭据事实来自Secrets名称与实际runner执行；资源来源/哈希来自config/windows-assets.json；公开版本、附件与Latest来自GitHub Release；匿名latest.json和实际附件验签为客户端事实。
- 复用抽象：既有prepare_windows_ocr/build_windows、CMake标准CMAKE_GENERATOR环境变量、Release matrix与publish_update/release_ci。保留平台构建owner，不新增发布服务或产品运行时状态。
- 新增架构机制：无；仅复用CMake生成器配置、前移既有运行库输入校验并输出runner公开依赖元数据，避免等待编译后才发现锁定输入不存在。
- 架构不变量：业务owner与协议不变；正式发行必须同一源码、不可变公开版本与可追溯身份。

### 验收与结束条件

- CMake不再强制VS17；Release Windows明确使用当前VS18生成器；x64/静态CRT/PNG-only配置不变。
- 锁定VC运行库在耗时构建前验证；路径/哈希变化时报错，只有实际微软资源证据支持时才更新锁定值。
- 正式0.1.21说明覆盖上一公开0.1.18之后的用户可见变化，去除“仅本机Mac候选”说明，明确Windows unsigned和首次升级限制。
- 本地脚本语法、actionlint与相关发行回归通过，fresh三路审查完成；真实main Verify及三平台Release负责Windows/macOS构建与签名证据，未完成前不宣称发行成功。
- 只有构建/资源/签名相关变化使相应检查失效；纯事实记录不重复原生打包。全部产物通过后发布并验证匿名更新地址，即结束本次发行。

### 开发批次与行动清单

- Batch 1：真实发行工具链修复与正式说明
  - [x] 修复CMake生成器、前移运行库验证、补充依赖输入证据；更新说明。
  - [x] 自验证、requirements/adversarial和最终architecture审查。
  - [x] 提交并合入main，取得实际Actions证据。
- Batch 2 / Final Integration：据实际runner证据锁定兼容的VC运行库
  - [x] 将10个既有运行库文件锁定为实际VS2026目录与字节/哈希，保留全部验证。
  - [x] 对照来源证据、自验证、完整最终集成三路审查。
  - [ ] 提交并合入main，取得最终原生产物证据。
- Batch 3 / Final Integration：已验证票据的Apple网络读取失败
  - [x] 仅在现有Mac打包owner内，对`stapler validate`实际观察到的exit68且NSURLErrorDomain -1005连接丢失做最多3次尝试；其它错误立即失败，耗尽仍失败；成功校验、签名、公证门禁完整保留。
  - [x] 通过真实zsh打包入口及隔离外部命令验证App/DMG暂态恢复、永久错误拒绝与有限次数；完成最终集成三路审查。
  - [ ] 提交并合入main，重建最终同源三平台。
- [ ] 跟踪三平台构建；按实际失败修复本次负责的阻塞。
- [ ] 公开完整v0.1.21，核对三平台附件、更新签名、main/tag/source及匿名latest.json。
- [ ] 保存最终事实并更新文档/台账。

### Review基线

- Batch base：`f424813a18ee0419790c83a2099cbc31fdb9972f`。
- 预计范围：scripts/prepare_windows_ocr.mjs、scripts/build_windows.mjs、.github/workflows/release.yml、CHANGELOG.md、docs/updates.md、docs/development.md及本台账；资源锁定仅在实际证据支持时另行修复。
- mode repair；三路required。只审本范围与本次发行阻塞；现有依赖公告和无关业务问题不扩大。
- Batch 2 base：`851601c711cef3013f6d554761eceab7eaad9134`；新增范围限`config/windows-assets.json`、实际运行库元数据证据及开发文档的版本值。Final Review从最初base `f424813a18ee0419790c83a2099cbc31fdb9972f`覆盖两批组合态，不回卷既有业务代码。
- Batch 3 base：`300b9f4a149ef9d3d85aef32475f1c2a1570e641`；新增范围仅`scripts/package_macos.sh`、对应外部命令边界回归、发行文档及本次证据。允许在既有构建owner对幂等外部只读校验做有界网络重试，不引入产品运行时retry/owner/store；这是已授权自动发行正常失败恢复的局部实现，结束标准不变。最终review从最初base覆盖全部构建配置组合态。

### 待用户决策

无。用户明确授权签名配置与三平台正式发行，确认验收通过；Mac/Windows行为验收按用户反馈记录，不冒称Agent自行完成所有实机升级场景。

### 进展日志

- 2026-11-03：用户要求自动签名和v0.1.21正式发行；六项非空Actions Secret已配置，更新密钥未旋转，无密码更新密钥的密码参数仍为空；只导出对应Developer ID身份的加密临时P12，上传后已删除。没有将凭据写入Git或记录。
- 2026-11-03：dev Verify 41502481665、main Verify 41503568942的Mac/Windows均success，先前Skill并发发布修复已取得Windows完整证据。
- 2026-11-03：Release 41505740487的preflight成功、两个Mac的证书导入/公证临时密钥准备成功。Windows在OCR配置时失败：VS17生成器找不到实例；实际runner为VS18，MSVC/Redist路径14.51.36231。取消该未完成run以整合必要源码修复，未创建v0.1.21标签/公开附件。
- 依据：CMake[标准生成器配置](https://cmake.org/cmake/help/latest/envvar/CMAKE_GENERATOR.html)、[VS18生成器](https://cmake.org/cmake/help/latest/generator/Visual%20Studio%2018%202026.html)；实际工具链以Actions日志为准，运行库继续精确哈希验证，不把runner README当作二进制哈希证据。
- 2026-11-03：Batch 1脚本语法、actionlint、差异排版、0.1.21版本/说明一致性通过，发行流水线与真实CLI更新签名回归9项通过。requirements发现“所有文档双语”的新说明不准确，已限定为中英文README；adversarial无阻塞。Windows锁定输入可用性、三平台产物与远端检查继续等待实际Actions，不以本地结果替代。
- 2026-11-03：requirements原文案问题定向复核通过；最终fresh architecture PASS，无新增业务owner/状态/重试机制。此次只改构建配置与说明，版本和业务代码保持原样；精确main Verify仍由正式Release等待，不关闭任何门禁。
- 2026-11-03：Batch 1提交`b948faf`并合入main `851601c`。main Verify 41507988472的Mac/Windows全部success；同树重复dev验证41507952370按用户立即合入main的要求取消，不冒称dev新SHA绿灯。精确main门禁完整保留。
- 2026-11-03：Release 41507988478实际Windows日志证明旧锁定目录14.50.35710不存在，构建已在编译前安全拒绝；没有跳过检查。该runner为官方image `win25-vs2026/20261030.294`，实际编译器/重分发目录为14.51.36231，10个CRT文件版本均为14.51.36247.0。只选择这组与实际编译器匹配的微软文件，依据已获取的每文件字节数和SHA-256人工更新锁定值；旧14.29/14.44数据仅为枚举结果，不作为fallback。
- 2026-11-03：原始日志保存在忽略构建目录，公开可审计证据位于`specs/research/evidence/2026-11-03-official-release/windows-runtime-inputs.json`。最终实际打包仍须重新按锁定值验证并运行PE依赖闭包和四组件身份检查；当前元数据不替代包验收。
- 2026-11-03：最终组合态的脚本语法、actionlint、版本/更新说明、排版及9项发行/实际CLI签名回归通过；逐一核对10项锁定值与原始runner结果一致。全新requirements/adversarial均PASS，fresh最终architecture PASS。最终Windows安装包还须只读解包，对照包内`resources/windows-assets.json`及实际10个CRT文件字节；PE闭包检查本身不证明运行库哈希，未虚称已经完成。
- 2026-11-03：Batch 2提交`9215adc`并合入main `300b9f4a149ef9d3d85aef32475f1c2a1570e641`，版本仍0.1.21。精确main Verify 41512164321的Mac/Windows均success；同树重复dev验证41512121294取消，未减少正式main门禁。
- 2026-11-03：Release 41512164218的ARM与Windows构建成功；实际Windows EXE为389,852,475字节，SHA-256 `6fc902819920e5a5290b7866eb2ef8e19718a2eaba02ea0c209c2c58e044997a`。只读解包后的10个实际CRT文件、包内清单与源锁定值全部一致，四组件0.1.21/source/dirty=false一致；实际EXE的更新签名、公钥及版本绑定复核通过。证据见同目录`windows-installer-audit-300b9f4.json`，属于300b9f4候选，不冒称后续新源码产物。Windows Authenticode仍unsigned。
- 2026-11-03：Intel首轮已完成编译、模型检查、签名，Apple公证Accepted（submission `38d6b231-3fbb-4a90-b8f2-c96d6b748b0f`），staple成功；随后独立validate遇Apple CloudKit连接丢失，exit68，属于网络失败而非公证拒绝。按既有失败job重跑流程启动attempt2，保留已成功ARM/Windows产物；源码、版本与私钥不变，未关闭任何签名校验。
- 2026-11-03：attempt2的Intel App公证`5def9cbb-f472-4372-b596-a62f869631a8`与DMG公证`2f0ef664-fba5-4f84-8a48-6b5f7bc4fb84`均Accepted，staple均成功；App独立validate通过，DMG独立validate再次因同一CloudKit -1005连接丢失返回68。两次不是证书/公证拒绝；为避免再次仅重编译整包，按真实失败在现有打包步骤补有限网络重试，校验本身仍须返回0。源码变化后将重建同一新提交的三平台，不混入300b9f4产物，不创建同版本混源发行。
- 2026-11-03：Batch 3真实zsh入口的6种隔离外部命令回归全部通过；与原发行/真实CLI临时签名回归组合15项通过，zsh语法、actionlint、版本/说明及排版检查通过。全新requirements/adversarial均PASS（后者独立复跑6种情况），fresh最终architecture PASS。未新增产品owner/store/队列/协议，有限只读网络重试留在既有Mac构建owner；原生产物与匿名发布证据仍待新提交的Actions，不将模拟命令当作真实Apple验收。
