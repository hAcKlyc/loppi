---
type: devplan
status: implemented
created: 2026-10-20
updated: 2026-10-20
---

# 顶部入口焦点、过渡与无刘海布局修正

用户在当前对话确认保留 Tauri NSWindow + React + AppKit 桥接，修好焦点、动画同步和无刘海布局，并补上能做的真机验证。本计划是现有入口的定向修正，不重新承担旧入口 PRD 所有尚未取得的机型/权限/资源验收，不修改录制意图或业务数据。

## 执行台账

### 开发契约
- 必赢场景：评论仅在应用实际激活且入口为 key window 后聚焦；延迟激活可确认，取消/切走后的迟到确认不得抢焦点。macOS 14+ 使用 cooperative activation，13 保留兼容调用。
- 原子交付：本次三个修正一起验证；一个入口窗口、不重挂 WebView、不引入 NSPanel 或另一个业务 owner。
- 动画：原生窗口完成展开后才揭示网页操作区，复用布局 generation 丢弃迟到完成。原生展开约160ms + 内容淡入60ms；取消/动作/评论仍立即布局，减少动态效果时无过渡。键盘焦点随可见操作区进入。
- 无刘海：收起保留32pt兔子点击高度；展开/评论 band=0。有有效左右安全区域的刘海屏仍用真实 safeAreaInsets.top。反馈位置随当前 band。安全区域无效时按无刘海处理。
- 状态来源与生命周期：core 拥有业务事实；EntryRuntime 拥有模式和命令代次；HearEntry 拥有原生窗口/焦点实际状态及本次过渡；React 只持有当前视图/草稿和已确认呈现状态。
- 复用：hear_entry_layout/focus、native_event、layout_generation/canceled_generation、FloatingWidget.layoutQueue/commentFocusIntent、现有 CSS 与测试。
- 新增机制仅限既有桥接内携带 generation 的原生完成/焦点确认通知，以及原生过渡取消序号；不新增持久状态、重试队列或轮询。
- 边界：不替换安装版、不停止用户录音、不提交测试笔记或截图，不改变主屏选择/菜单动作/角色素材；历史所有系统兼容门槛不由本次宣称关闭。

### 验收与结束条件
- 必须：类型/lint/构建、前端迟到完成与焦点回归、原生布局/命中与焦点确认回归、Rust入口测试、签名包检查、三路独立审查。
- 本机 mandatory：新构建的真实入口可展开、评论实际聚焦、取消/外部切走后不迟到抢焦点，完成通知与窗口最终几何一致。使用隔离验证数据且不影响已安装版采集。
- 可用设备尽力验证：无刘海外接屏、全屏/Space、不同缩放；无法取得的硬件/真实鼠标/IME/睡眠证据明确记录，不用模拟冒充。它们仍为整体入口发布门槛，不扩大本次“能做的真机验证”承诺。
- 按影响：原生/桥接改动重跑原生+Rust与签名新包；React/CSS改动重跑静态+交互与实际界面；文档不重跑构建。
- 结束：上述必须项与本机 mandatory 通过、审查 PASS 后提交；若本机 mandatory 无法完成，保持 in-progress 并明确缺口。

### 行动清单
- [x] 实现焦点确认、完成驱动动画、无刘海布局与回归测试
- [x] 静态/构建/相关回归
- [x] 签名新包真机验证与证据
- [x] requirements/adversarial → 裁决修复 → architecture
- [x] 提交与最终验收记录

### Review 基线
- Batch base: `3faad261272eac54c2fb1d306ceae10469e4cb4d`
- 单 Batch；预计范围：src/FloatingWidget.tsx、src/entry.css、src-tauri/native/entry.m、src-tauri/src/entry.rs、入口测试、相关RFC/本devplan。
- 其余用户已有改动全部排除，尤其 README、打包脚本、其他PRD与设计素材。

### 待用户决策
无。

### 进展日志
- 2026-10-20：核查现有实现、Apple cooperative activation 文档与本机 SDK；当前安装版仍在录制，不重启或覆盖它。按会话需求建立独立定向交付契约。

- 2026-10-20：三个修正已实现。原生active+key一次性确认携带原评论intent；窗口完成通知携带layout generation并受原生serial取消；无刘海展开/评论band归零。保持单窗口及现有owner，没有新增轮询、持久状态或激活重试。
- 2026-10-20：新增回归覆盖无刘海/刘海/不完整安全区、旧布局完成、真实焦点尚未成立、延迟焦点确认、关闭后旧intent不得聚焦新会话。原生测试、React交互测试、5项入口状态/请求测试、9项Rust入口测试通过。
- 2026-10-20：requirements、adversarial独立审查PASS，主Agent无阻塞项需修复；最终完整候选的architecture独立审查PASS。审查后生产代码未再变化。
- 2026-10-20：为避开共享工作树中并行开发的定时录制改动，从Batch base建立私有源码副本，叠加本批六个代码/测试文件，单独构建core/CLI与Tauri签名debug包。初次缓存复用导致组件版本不一致，清理私有包缓存并重建后消除；最终包运行无版本错误，六个文件逐字节与审查后工作树一致。隔离副本全量typecheck/lint、原生/前端/Rust回归均通过；签名包codesign --verify --deep --strict通过。构建仅有既有Vite大chunk提示。

### 真机验证证据与边界（2026-10-20）

- 使用独立bundle id `app.hear.entry-check`、独立`HEAR_DATA_DIR`和新签名包；测试数据为空且没有启动录音、保存评论或截图。安装版GUI/core始终保持原PID11609/11614。验证结束仅退出测试GUI及其隔离core，两者已确认退出。
- 当前唯一显示器为内建Retina，1470×956pt、2×、safeTop32pt；左右安全区分别宽646pt/645pt，中间缺口179pt。实际收起入口44×32pt，位置(825,0)。
- 原生窗口逐帧只读采样（约10ms间隔）观察到44×32→142×66→227×96→293×119→339×135→392×153→411×160→412×160，最后位置(529,0)，展开约148ms，符合160ms动画；评论最终412×184pt。实际界面操作区在展开后显示，未观察到仍受窄窗裁切的可用按钮。自动回归另外验证只接受当前generation完成通知。
- 前台评论路径已实测：AX焦点为“留下评论”文本框，粘贴“入口验证，未保存”成功；取消→重开保留草稿，没有保存业务笔记。取消后入口收起；系统应用切换操作后入口回到44×32，后续只读观察前台保持Codex，未见迟到重新激活。原生/React回归还覆盖未激活、迟到确认和取消旧intent。
- 自身应用全屏时入口仍在全屏Space，顶边44×32、layer25；退出全屏后恢复。该证据仅证明自身全屏，不冒充第三方应用全屏覆盖。
- CUA的定点点击不会移动真实系统鼠标；原生穿透使用真实NSEvent.mouseLocation，因此无法用这种点击证明真实鼠标跨应用进入。AX后台请求也可能被macOS拒绝激活；新逻辑正确保持“未确认”，不将请求当成功、不循环抢焦点。真实鼠标跨应用首次评论聚焦、归还目标应用仍待人工，不能据本次自动化声称所有应用均已通过。
- **整体入口发布mandatory、非本次新增交付范围**：真实鼠标跨应用激活/归还、第三方全屏及Space切换、无刘海外接屏与混合缩放、真实中文IME组合输入、macOS13兼容及睡眠唤醒。当前缺少设备/物理输入能力；无刘海几何仅有生产函数测试，macOS13仅构建兼容路径。本次用户要求的“能做的真机验证”已执行，历史入口发布门槛仍未关闭。
- 本次三项修正的本机mandatory已由新签名包的实际展开/评论聚焦/取消及切换后收起/最终几何，以及迟到回调回归闭合；不据此替换安装版或宣告整体入口可发布。

### 复验入口

- `npm run typecheck`、`npm run lint`；`node --experimental-strip-types --test scripts/test_entry.mjs`、`node scripts/test_entry_interaction.mjs`。
- `clang -fobjc-arc -mmacosx-version-min=13.0 -framework AppKit -framework CoreGraphics scripts/test_native_entry.m -o /tmp/hear-native-entry-test && /tmp/hear-native-entry-test`；`cargo test -p hear-desktop entry::tests --lib --locked`。
- 私有验证目录：`/var/folders/n5/pzrmblj11dbc2ltb0ncd36s00000gn/T/hear-entry-polish-5lv855d_`。新签名包在`target/debug/bundle/macos/hear Entry Check.app`；窗口采样保留在该目录`animation-focus.tsv`。验证包基于Batch base及本批改动，未混入并行ASR/定时录制交付，未安装。
- 手工验收应在该独立包上用真实鼠标从其它应用打开评论，确认光标/输入及取消后前台归还；在无刘海外接屏确认展开上方无32pt空带；在第三方全屏及中文IME输入过程中检查不抢焦点、不误提交。未获得证据前保留整体发布门槛。

- 2026-10-20：提交前在当前共享HEAD（已包含独立ASR合并）重新执行全量typecheck/lint、5项入口测试、React交互、原生测试及9项Rust入口测试，全部通过；diff whitespace检查通过。私有签名包再次校验通过。当前候选满足本次定向契约；不承接或关闭历史整体发布门槛。

- 2026-10-20：实现提交`009a885`。单Batch已完成三个修正、受影响回归、独立三路review和本机可执行验收；review后未变更生产代码，无中间兼容层或新增owner。最终状态`implemented`仅指本devplan限定的定向修正与“能做的真机验证”，不代表旧入口PRD整体发布验收完成。其余共享工作树改动未纳入提交。
