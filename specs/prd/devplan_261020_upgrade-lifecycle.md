---
status: implemented
updated: 2026-10-20
---
# 更新交接与完整退出

用户要求更新无需重启电脑，正常退出同时结束后台。交付 0.1.2 Apple Silicon 安装包。

## 执行台账
### 开发契约
- 必赢场景：已发布 0.1.0（协议17）、0.1.1（协议19）后台仍在运行时，启动新版 GUI 自动优雅交接；明确退出 GUI 后 Core 和采集子进程结束，不被 GUI 请求拉起。
- 保证：保留数据库、模型、两项云配置、来源、录制/暂停意图；已在录制的后台交接后恢复采集许可，未录制的旧版本在交接启动阶段保守保持查询态。随后 GUI 的既有 AppOpened 正常运行信号和定时规则保持原语义，不新增屏蔽定时动作的状态。关闭窗口继续后台，显式退出停止。
- 来源：Core 独占 owner.lock/SQLite/采集监督，客户端只做生命周期协调，不写业务数据库；复用 Quit、owner 锁、采集 join、启动恢复。
- 机制：协议库内 GUI 会话启动与退出屏障；跨 GUI 的有界文件锁；稳定 Lifecycle 查询/交接/退出消息；仅真实已发布协议17–19的兼容 Quit；新 Core 启动携带内存运行许可，不新增持久状态。禁止降级替换更新的后台。
- 不做：修改用户数据、密钥迁移、自动付费请求、无限重试、强杀后台。启动后台故障明确显示实际错误，元数据恢复仅限损坏记录。退出清理失败提示实际错误；程序请求退出可取消，原生最终终止不可取消，提示稍后重新打开应用重试。用户已知本机验证窗口接管了原后台，交付时本机切换到正式 Release 并保持既有录制状态。
### 验收与结束条件
- Rust 测试/构建、前端静态检查；隔离目录运行真实旧版→新版交接，核对退出、唯一 owner、持久状态和数据；模拟并发、超时、更新后台拒绝降级。
- 真机签名包验证生命周期入口，签名/公证/安装包完整性；不得触碰正式正在录制的数据来测试。
- 三路 cross-review 完成后本地 dev 提交、合并 main、打包；不 push。
### 当前批次
- base: ef3a692b90c825d0967ae844904d651be5ea3ba4
- 范围：协议生命周期、Core 交接、GUI 退出/错误、对应测试/架构文档/版本元数据。
- 无关 README、package_macos.sh 与既有未跟踪文件不纳入提交。
### 行动清单
- [x] 实现、回归和实际旧版升级验证
- [x] requirements/adversarial → architecture
- [x] 提交、合并、签名公证包
### 进展
- 本机新版 CLI 请求旧 Core 实际返回 protocol_version expected 17；首次设置丢弃此错误；Quit 同样受版本检查拒绝。
- 实现稳定 Lifecycle 通道、GUI 会话/跨进程交接屏障、17–19 兼容退出；0.1.2 协议20。真实已签名旧 Core 的两版本×两启动方式×升级/退出八场景通过；并发启动、错误混包不停止旧 owner、稳定通道三种运行许可和新构建禁止降级通过。
- 前端 typecheck/lint 通过；Rust workspace 测试进行中。保留正式运行的 /Applications/hear.app，全部测试使用隔离目录。
- Rust 303 passed / 7 ignored。Phase 1 审查完成：adversarial PASS；requirements 提出的 AppOpened 定时激活问题经裁决为契约阶段边界歧义，保留既有正常 GUI 打开行为，补充实际 AppOpened 路径及定时开启/暂停保持验证；不引入新的定时屏蔽状态。native signed smoke 后补。
- 签名 native smoke：真实旧19后台在隔离目录被替换；Cmd-Q 后 GUI 11645/Core 11674 均退出且 socket 删除；关闭窗口保持后台。另一本机旧17录制后台实际交接后，两路采集均恢复 recording，原修订56及录制意图保留。未清理或重建任何用户业务数据。
- requirements 最终 PASS（故障实际错误原生提示已补）；adversarial PASS；最终 architecture PASS。macOS 默认 terminate 直达 Exit，已复用幂等清理补齐；Core 卡死等无法优雅终止时不强杀，原生退出失败可见提示重新打开重试。最终签名 release 的失败提示 smoke / 身份 / 公证检查待构建后完成。
- cb5b060 已提交 dev 并合并 main，clean Release 四组件身份一致；初次公证通过。故障 smoke 发现脚本弹窗脱离 Hear 归属，改为既有 native bridge 的 NSAlert；11 desktop tests 通过；签名包 native 故障 smoke 实见实际错误，可点击“好”且 GUI 正常退出。adversarial / architecture 定向复查均 PASS。最终包需按此修正重新构建公证。
- 最终代码 1aed8d6bacb9dd55462a1522ee785686e7b5386f 已合并 main，clean Release 四组件均为0.1.2/协议20/dirty=false。最终包真实原生失败提示可见且确认后退出；Developer ID 签名、Apple公证、staple、Gatekeeper及镜像校验全部通过。公证4f785706-b86b-4dc2-8501-dea39b58cd81；DMG SHA256 c97d612d5ab13d47b9154965e683627710833a9ca947002855c9c24a3bef1e1d。
- 交付文件 target/release/bundle/dmg/hear_0.1.2_arm64.dmg（17,670,864 bytes），同目录保留 checksum/build-identity/release 元数据。本机 /Applications/hear.app 已切为相同正式包，旧测试 Core 优雅交接；修订56、两路来源、录制/暂停意图不变，两路均恢复 recording。旧 app 留在 /tmp/hear-upgrade-legacy/installed-before-0.1.2.app，仅作文件备份，不支持直接降级处理新数据。
- 最终门禁：真实旧17/19升级与退出8场景、并发初始化/混包保护、30秒忙锁超时、稳定通道许可传递/拒绝降级、实际AppOpened定时语义、native关闭窗口/退出/错误提示均已覆盖。303 Rust tests通过/7既有环境测试忽略；桌面修正后11 tests复跑通过，前端typecheck/lint与最终build通过。无剩余必做项；不push、不清理用户数据；无关README与打包脚本改动原样保留。
