# 更新功能审查与裁决

## Phase 1

- requirements：fresh-context `requirements_update` 完成全部需求映射，无实现阻塞。保留真实签名两版本升级、Windows 安装与 GitHub 三平台清单联动的必做证据缺口；开发计划不得标记 implemented。
- adversarial：fresh-context `adversarial_update` 确认一个 P1：Mac 系统 tar 会为 App 根目录扩展属性生成 `._loppi.app`，Tauri 更新器去掉一层路径后将该普通文件映射到解包目录，导致安装失败。
- 主 Agent 裁决：该问题由本次更新归档引入，违反 Mac 自动更新要求，属于阻塞。其余 Windows 安装器成功启动后的结果属于已记录真机验收缺口，不通过额外生产状态或重试机制扩大保证。

## 修复与受影响验证

- 修复归回现有更新产物脚本：`createMacUpdateArchive` 使用系统 tar 禁止 macOS metadata/xattrs sidecar，保留 App 的真实文件。没有新增运行时 owner、协议或持久状态。
- 回归测试给合成 App 根目录及内部文件设置真实 xattr，验证没有 AppleDouble 条目，去掉外层 App 后仍保留 plist 和票据样例字节。
- 15 项归档/签名/清单/构建计划测试全部通过，日志 `/tmp/loppi-updater-archive-tests.log`。其中签名使用真实 Tauri CLI 与临时密钥，不接触生产密钥。
- 用最终 ARM 开发 App 实际生成归档，以项目编译出的 Rust tar/flate2 依赖执行和 Tauri Mac updater 相同的逐条路径剥离及 unpack 操作：60 条成功；解包后的整个 App 与原目录 `diff -qr` 一致，日志 `/tmp/loppi-updater-native-archive.log`。
- 此修复只影响更新归档生成，未改变前端/原生宿主/Core。此前最终组合态 21 项离线门禁仍适用：426 Rust 通过/21 既有忽略，112 前端测试+3资源测试通过，Python 9 通过。统一身份生命周期专项 7 个场景全部通过，包括更新中止恢复屏障、并发初始化、退出、不启动缺失 owner、繁忙锁及三种交接/较新版本保护。
- ARM 开发包已完成完整构建、组件身份与离线模型烟测。Intel 完整编译与 App 检查通过；首次镜像模型检查因 model_load_timeout 失败，随后定向模型复测及重新打包挂载检查均通过。只声明 Rosetta，不称为 Intel 实体机结果。

## 最终门禁

- 原 adversarial 定向复查：PASS，独立复跑真实 xattr 回归并核对最终代码/实际解包证据，原 P1 关闭。
- fresh-context architecture：PASS，完整最终候选符合 owner、生命周期、复用和复杂度边界，无未批准机制。
- 真实签名 Mac/Windows 两版本升级与三平台 GitHub latest.json 联动仍未执行；没有 Windows host，也没有发布/改写当前公开版本。保持 in-progress，不提交实现、不发布。

- 2026-11-02 用户明确要求提交代码，覆盖默认的暂缓提交条件；真实签名升级门槛继续约束发布与implemented状态。提交仅包含本任务差异；白字样式的类型/格式、9项受影响测试及实际浏览器对比度5.27:1验证均已通过。
