---
status: in-progress
created: 2026-10-20
updated: 2026-10-21
---

# 设置页交互统一

用户已接受本次设计审查并明确要求开始开发。设置页开关立即提交，只有独立编辑流程有保存/取消；分组为通用、录制、识别与整理、存储与数据。修复定时开关草稿与已生效状态矛盾，并统一声音来源开关、权限下一步与云配置呈现。

## 执行台账

### 开发契约
- 必赢场景：点击定时开关直接生效，无保存按钮；时间摘要及下次动作反映 Core 保存值。独立时间弹窗两项一起校验/保存，取消不写；关闭规则保留时间，编辑时间不改变启用状态。
- 原子交付：一次前端交付，既有 Core/Tauri 协议、持久数据、录制/权限语义不变。
- 必须保证：开关不展示未提交草稿；保存失败保留输入；定时结果未知继续用既有同请求 ID 确认，不产生新规则起点；重入防护；关闭/启用规则不改变当前录音；请求冲突必须告知，不静默覆盖。
- 声音来源使用右侧开关并即时提交，权限就绪与配置启用分开。未授权明确尚不能录制，一个主要授权行动，帮助区域保留排障。
- 时间/容量/API Key 独立编辑；容量展示上限与用量并解释触顶暂停；两个云服务并列展示，密钥仅显式验证并保存，取消/失焦不发送；原配置失败保留，未知结果沿用 setup.ts 回执查询。
- 非目标：不修改首次引导密钥失焦提交（compact 路径）、云调用策略、权限原生实现、录制调度算法、导出/清理行为；不发布/安装、不改用户实际设置，不新增跨重启恢复或自动重试。
- 本次负责：设置页的控制语义、草稿隔离、分组、权限反馈、异步提交及修改共享组件的相邻回归。
- 状态来源：Core ServiceState/CloudQueue、宿主 loginStatus/privacy/entrySnapshot 为事实；弹窗草稿仅组件生命周期；setup.ts 仅持有既有未知云请求身份，不存密钥。
- 复用：Switch、SegmentedOptions、Modal、LoadBoundary、SourcePreparation、ModelPreparation、CloudKeyInput、saveCloud/confirmCloud、Core revision + mutation_receipts。
- 新增架构机制：无新业务 owner/store/protocol；允许抽取现有定时提交模式为组件内共享 hook，供设置编辑复用，保持同请求确认/显式重试。
- 不变量：Core 唯一业务写入者；GUI 不执行调度、不绕过权限；视觉沿用 tokens 与原有控件。

### 验收与结束条件
- 静态：typecheck/lint；构建：前端生产构建。
- 交互：真实浏览器运行隔离 IPC mock，覆盖立即开关、取消、时间校验、失败/冲突、同请求确认、关闭保留时间、容量编辑边界、密钥失焦不提交/显式提交/失败/确认、权限状态与操作、分组与键盘弹窗。
- 回归：首次引导既有 browser/Node 检查，共享组件关联 people 设置用例更新；相关 schedule Node 测试。
- 视觉：浏览器截图检查完整设置页面及编辑弹窗、窄窗口；不操作真实用户录制和密钥。按 DESIGN 的视觉门禁增加隔离签名客户端布局及编辑流程检查；没有修改 OS/Core 实现，不要求重跑原生录制/睡眠测试。
- mandatory：上述实现、检查与三路审查；advisory：实际 macOS 字体观感及用户偏好。浏览器验证不声称完成真机授权/录制测试。
- 改动影响：UI/hook 改动重跑静态/构建与相应交互；仅文档更新不重复运行运行时验证。
- 结束：requirements/adversarial 裁决修复、最终 architecture PASS、受影响验证完成后提交；不扩大为全应用改造。

### 开发批次
- Batch 1：完整设置交互统一。
  - [x] 实现与文档
  - [x] 静态、构建、浏览器回归和视觉
  - [x] 需求/对抗审查、修复、最终架构审查
  - [ ] 提交与原子交付闭环

### 当前批次 Review 基线
- Batch base：276ab29eb0908cb45ba22597a796cab8446d17ec
- 分支：codex/settings-interactions
- 范围：src 设置相关组件/样式、必要共享 UI、设置/首次引导回归脚本、DESIGN/ARCHITECTURE/定时 PRD 对应行为说明及本台账。
- 排除：既有 README.md、scripts/package_macos.sh、build_release.sh、playground/design 未跟踪资源、其它 specs 文档。

### 待用户决策
无。

### 进展日志
- 2026-10-20：核对真实代码与已有协议，建立契约。仓库无适用 AGENTS.md/CLAUDE.md；保留工作区既有无关改动。

- 2026-10-20：实现完成；静态检查通过，前端构建通过（既有 chunk 大小提示），5 项 schedule/onboarding Node 检查通过。真实浏览器隔离回归：定时 19 项、设置 32 项通过。签名客户端构建进行中，准备真机视觉检查及三路审查。

- 2026-10-20：Phase 1 requirements/adversarial 均无阻断；人物 21 项及 compact 首次引导 4 项浏览器回归通过。签名构建通过 deep/strict；隔离原生 UI 验证即时定时、时间键盘输入/保存/取消、容量显式保存与密钥取消，无实际录制/云凭据。
- 2026-10-20：键盘验收发现原 Modal passive cleanup 在 DOM 移除后关闭，Escape 丢失发起按钮焦点。本次新编辑流程显著暴露，裁决为 in-scope；修复为 layout cleanup 在移除前关闭并恢复原焦点。新增 src/Controls.tsx 到 review 范围；只需共享弹窗浏览器回归、静态/前端及签名 UI 受影响检查。需求/对抗报告无其他需改生产代码的发现；旧 PRD 操作说明同步为当前文案。

- 2026-10-20：焦点修复定向复查进一步验证成功保存时发起按钮仍待同提交解锁；原生 close 保持 layout cleanup，焦点恢复放到本次提交后的 microtask。浏览器实测确认 Escape 和保存均恢复发起按钮，定时回归增加两项。两位原 reviewer 定向 PASS。

- 2026-10-21：最终三路审查均 PASS：requirements/adversarial 原 reviewer 完成焦点修复定向复核；fresh-context architecture 审完整最终实现，无阻断发现。无新增业务 owner、协议、持久化或自动重试。
- 2026-10-21：最终 typecheck/lint、前端生产构建、diff whitespace 检查通过；浏览器定时 21、设置 32、人物 21、compact 引导 4 项（共 78）及 Node 5 项通过。既有生产 chunk 大小提示为非阻断。
- 2026-10-21：760×540 iframe 实际视口视觉检查通过，时间与密钥弹窗均完整可操作。浏览器 viewport override 未生效，故改用真实嵌套视口，未将 1280px 截图当作窄窗口证据。
- 2026-10-21：最终修复版桌面 release 编译成功；复制到独立 build/settings-preview/hear.app 后替换 GUI 并重新签名，构建身份一致、codesign deep/strict 通过。原生隔离数据实测键盘打开→Escape、保存 09:00→10:00 均恢复编辑时间按钮焦点；时间摘要更新且规则仍关闭。早前原生即时开关、容量保存、密钥取消验收仍有效。未安装替换客户端；未做实际麦克风/系统录制、云端验证或 OS 权限回归，因为本次没有改这些实现。
- 2026-10-21：原生证据 /tmp/hear-settings-evidence/settings-native-final.png；窄窗口证据 time-narrow.png、cloud-narrow.png。验收预览已退出。最终契约逐项核对覆盖，无待用户决策或未完成 mandatory 项；首次引导、导出/清理及后台调度边界保持原实现。
