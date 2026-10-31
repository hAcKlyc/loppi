# Windows 组合态审查

修复模式。基线`1e36cf56c5c49b9cc4ba9d9305131e7e83782ca9`，原生候选`34bfec2f93e42ca80c8671bcb2c2a8a313b5ae71`加root集成；最终冻结135文件，需求、故障及fresh最终架构审查均核对哈希一致、只读完成，源码结论全部PASS。原生候选自身的审查见[windows-review.md](../../../tech_docs/windows-review.md)。本结论允许内部源码checkpoint，不代表发布验收完成。

## 确认问题、裁决与修复

| 问题 | 裁决及根治 | 直接证据 |
| --- | --- | --- |
| 新ICO误用旧Hear waveform/h | blocking：违反用户保留折耳歪头兔ICON。仅从批准square母版派生7尺寸，固定整数像素中心采样，原PNG/ICNS不改。移除浮点48px跨CPU差异。 | 两OS七尺寸RGBA及ICO SHA478b1c5d…53603完全一致；三位审查者逐像素0差异，实际GUI/rc编译通过。正式包使用Git字节，不在构建时重新生成。 |
| 托盘v4遗漏键盘上下文菜单 | blocking：WM_CONTEXTMENU未路由Open/Quit。复用原菜单，v4锚点来自WPARAM，保留legacy鼠标路径。 | 实际Windows18项几何/消息测试及GUI编译通过；菜单选择被stub，不能冒充物理任务栏输入。 |
| PowerShell多Node/启动失败误返回0 | blocking：选第一应用、重置并读取原生exit、显式catch非零；stderr警告不替代exit。 | 两入口实际参数拒绝/缺Node/坏exe非零，多Node正确，受控warning+exit0仍0。 |
| 早启动GUI存在但尚无入口，被安装准备漏掉 | blocking：复用Toolhelp、进程身份与完整安装路径，先发现进程，再在一个15秒截止内等入口、发既有正常exit并确认真实句柄结束；末尾5秒静止检查包括GUI/Core/worker。句柄RAII，无强杀或新owner。 | 协议19项；真实GUI两次early、一次ready，均exit0且Core/worker结束；合成无入口15.36秒/静止5.10秒非零保活，跨目录不影响。 |
| 上游Tauri NSIS在Hook后仍可能RmForceShutdown | blocking：按匹配CLI2.12的包含顺序覆盖同签名宏，只读RmGetList，busy/error非零Abort，silent默认退出。签名约定改变时编译失败。 | 真实NSIS3.11小安装器：空/未运行路径放行；安装和卸载占用时非零且持有者存活；正常关闭后放行。展开两处RmGetList，无Shutdown/Kill。完整正式包仍待重建。 |
| Windows CRLF及首次设置测试fixture不完整 | required evidence gaps：仓库.gitattributes统一源码LF；测试显式表达两端音频事实并正确模拟宿主，不放宽lint、不改全局Git配置、不加生产flag。 | root普通Mac与Win32呈现模拟90Node均通过；真实Windows干净checkout完整门禁仍待执行。 |

消息依据：[Microsoft Shell_NotifyIconW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw)。安装器依据：[匹配的Tauri CLI2.12模板](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.12.0/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)及[utils](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.12.0/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh)。

## 验证与审查结论

Mac完整verify通过21项（[原始报告](macos-regression.json)），包括共享Rust/前端/原生策略/四组件身份/隔离Core。随后修复未改Mac业务；受影响的fmt、diff检查、typecheck/lint及Node检查通过，原生产Mac应用未重启、未读写其资料。

需求定向复核：源码PASS，独立19项Node在Mac及Win32模拟均19/19通过。故障定向复核：源码PASS，独立13项相关检查通过。两者均未发现修复残留，也未将尚待回传的Windows原始JSON冒称为自己独立读取。

最终fresh architecture：PASS。完整最终diff保持Core唯一业务/SQLite/录制owner；平台facade、受限IPC、来源监督、凭据、moments、播放、EntryRuntime、安装退出及构建依赖归既有owner。未新增业务双状态、代理协议、重试owner或未经授权架构机制；87项设计manifest及原ICON指纹匹配。审查完成后只补本报告和真实证据，不改变生产源码。

Windows[紧凑原始运行报告](native-repair-evidence.json)已通过公开CLI文本回传，11853字节、SHA256 `bea14082374b42ae06d67d5dd03004f387b5ee7e0643bb2dbf21af96d346b814`，root解压验证并逐项核对。内含早启动、合成超时、NSIS、PS、ICO及十项旧Hook对照；路径脱敏，明确dirty诊断构建。`install_lease.nsis_force_shutdown_still_blocker`是覆盖修复前的历史快照，后续`nsis_fresh/nsis_install_uninstall`才是覆盖修复证据；仍不替代最终完整包。

## 旧Hook报错与剩余发布门槛

旧dirty安装器在“录制中覆盖→再启动→录制中卸载”一次报错，GUI/Core后来正常退出。原失败进程在后续读取已不在，未发送终止；原因未复现/未知。旧/当前CLI×直接/nsExec×空闲/真实双录制八项，以及真实临时卸载副本两项全部返回0、GUI正常退出，原安装文件哈希不变。不能据此发明根因或把早启动竞态混作该失败的已证实原因；最终完整新包仍须重复原顺序。

尚待mandatory：最终干净源码的真实Windows offline/static/lint、完整NSIS与默认/Unicode安装/录制中升级/默认卸载及资料保留；最终模型负向、组件身份、资源闭包和实际图标；解锁桌面截图/OCR/HWND排除及物理托盘/悬浮/启动操作；同源码三份包、Mac签名公证、准确标签/Release及上传SHA/大小核对。锁屏失败、WebView内部图、mock、dirty候选、小型NSIS与最终事实分别记录。全部关闭前台账保持in-progress，不发布。


## 实际Windows完整门禁的Python工具收敛

73f53d4完整Windows检查通过typecheck/lint/Rust check/全部Rust/Node/rabbit后，Python discovery发现两处顶层fcntl导入和一处Mac合成路径随宿主Path改写。分类required evidence gap；仅将OS依赖移到Mac操作函数，并用PurePosixPath固定Mac样例；不改产品源、不跳过既有断言。Mac9项通过；实际Windows运行9项、1个原有Mac native adapter跳过，其余8项通过。原始stdout及边界见[报告](python-portability-evidence.json)。

两位需求/故障定向复核PASS，分别独立跑9项及拒绝fcntl导入模拟，均9通过0跳过；原架构审查者定向PASS，AST确认操作函数逻辑和13项样例断言保持。原最终架构结论继续适用，不机械重开全量。该修复提交后统一新SHA重建三端正式包；73f的两份Mac公证包仅是中间验证，不混为最终产物。
