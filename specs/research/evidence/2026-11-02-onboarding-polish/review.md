# 2026-11-02 交付检查

- 首次准备页：requirements / adversarial / architecture 均 PASS，无阻塞发现；追加侧栏 logo 居中定向 requirements / architecture 均 PASS。
- 用户明确追加授权当前仓库全部未提交内容进入 dev，并构建本机 ARM64 release 包。对既有 Core AppOpened、GUI lifecycle 驱动、隔离验证、元数据/文档/CI/许可资料另行执行 fresh-context requirements / adversarial / architecture；均 PASS，无阻塞修复。
- Core 仍是业务事实与记录许可的 owner；沿用既有 run/capture 许可、日程结算及退出标记清理，无新增状态、owner、协议或恢复机制。
- npm run verify 全量离线门禁：21 检查全部通过，含 433 项 Rust（另 21 项既有显式资源测试按原策略 ignored）、123 项 Node、12 项 Python、原生策略/采集错误/回放/电源，以及隔离 Core/诊断/test-owner 烟测。
- 另外以同一新构建 epoch 构建 0.1.20 Core 和 gui_lifecycle，运行 smoke_gui_activation.py：query 不提权、首次不开录、GUI 恢复许可、normal handoff 提权、退出再打开、暂停与来源保留均通过；来源均关闭，没有采集真实音频。
- 1120×760、760×540、1920×1080 合成数据截图及几何记录证明首次准备页无水平/行内溢出，标题 16px、底部动作均 13px、三个成功对号均为绿色且位于文字右侧；侧栏字标/紧凑图标居中。
- 浏览器 macOS/Windows 呈现各 6 项首次准备回归通过，两个服务商动作分别传 llm/asr，打开密钥入口不提交凭据，失焦仅提交一次、保存后恢复掩码、完成设置不开始/恢复记录。键盘密钥入口/稍后、浏览器打开失败及凭据失败提示通过。
- 既有非阻塞资料问题：component-inventory 的哈希及 571 个 Cargo 包计数落后于当前 590 包锁文件；增加的 19 个依赖通知已包含在 updater-third-party.txt，未缺失安装包通知文本。按审查契约记录，不扩张为本次修复。
- 本地 release 构建待本次全量提交后执行；不合并 main，不推送、打 tag 或发布 GitHub。其它 PRD 原有待验收状态保留。
