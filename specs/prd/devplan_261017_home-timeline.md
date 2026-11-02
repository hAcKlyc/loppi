---
status: implemented
updated: 2026-10-17
---
# 首页记录概览与内容摘录

需求源：2026-10-17 对话。修正时间条含义、展示未整理素材、补充卡片内容摘录、收紧布局；保留浅绿色风格。完成后构建签名桌面包并重开给用户看。

## 执行台账

### 开发契约
- 必赢场景：首页概览显示事件及未整理内容范围，麦克风/电脑声音分别显示缺口；不把内容范围或缺口冒充连续录音。卡片直接展示真实且有限长的文字摘录，点击仍进入完整详情。
- 原子交付：单批次，前端+核心列表摘要+协议可选字段+验证+签名桌面运行一起交付。已有旧核心/CLI存在：preview字段可选、默认None，老客户端忽略新增字段；不变更协议版本或数据库。
- 必须保证：时间图裁剪到当天及当前已发生时间；分来源表达缺口；明确“内容范围内可能有空白”；独立现场记录保留；过滤只影响列表；保留所有已有操作和日期/录音控制语义。摘录优先有效精转（含人工修正），否则粗转；尊重事件/手动范围版本、来源和时间边界；每卡最多160字符加省略号，无整份转录下发、无逐卡前端IPC。
- 明确不做：波形/连续音频覆盖统计、AI摘要、搜索、新建事件流程改造、日期导航重构、自动滚动、录音状态/存储/权限变化。
- 本次负责问题：图语义误导、普通未整理范围缺失、空摘录数据链路、低信息密度；以及上述改动引入的回归。
- 状态来源与生命周期：core timeline仍是列表事实源；新增仅可选TimelinePreview摘要投影，复用既有粗精转读取/版本过滤与文字修正，不新增持久状态或缓存。概览位置从当前日期、Date.now和接口区间派生；现有刷新驱动，不新增timer。
- 复用抽象：timeline/unorganized/precise_for/range_precise_for/coarse_for/asr_version、TimelineItem、TimelineView、dayBounds、现有tokens。
- 新机制：允许可选TimelinePreview传输结构(text/source/is_precise)，以及可测试的纯时间裁剪/卡片呈现helper；无新owner/store/持久化/协议路径。
- 架构：核心独占数据；UI只投影和转发现有动作。架构依据specs/ARCHITECTURE.md及DESIGN.md；未发现AGENTS/CLAUDE。

### 验收与结束条件
- 概览覆盖普通未整理/事件/手动精转/现场标记；两路缺口彼此独立，缺口用中性纹理，绿色只表示内容范围。图例明确；现在标记仅当天可见。
- 时间越界/跨日/未来/零长裁剪正确；短区间仍可点击但不按百分比夸大时长；过滤不抹去概览事实。
- 卡片有有界真实摘录及粗精转来源，空文字有诚实回退；纯笔记/截图用已有现场文字作线索。修改/失效精转不泄露旧结果。
- 首页布局在760×540和常规桌面可读，旧详情/录音控制无回归。
- 验证：JS纯函数边界测试；core/protocol针对性及相关existing Rust tests；typecheck/lint/build/cargo check；实际组件合成浏览器烟测；最终签名构建及真实桌面首页验证（mandatory）。
- 影响映射：UI改动重跑静态/浏览器，数据改动重跑Rust对应测试；签名包基于最终源码构建，最终运行确认GUI和core使用新包。
- 结束条件：三路review通过、检查通过、新包已打开且首页效果确认，才implemented。真实录音长期可靠性非本轮新增门槛。

### 开发批次与行动清单
- Batch 1
  - [x] 有界摘要与数据测试
  - [x] 时间图、卡片、布局与前端验证
  - [x] requirements/adversarial + final architecture
  - [x] 签名构建、重开真实桌面、最终验证及提交

### Review 基线
- Batch base：aa898ddd4565d487213c84ca1a1370d0e5740bc1
- in-scope：src/TimelineView.tsx、src/timeline-view.ts、src/api.ts、src/App.css、crates/loppi-core/src/events.rs、crates/loppi-protocol/src/lib.rs、scripts/test_timeline_view.mjs、specs/DESIGN.md、本devplan。
- out-of-scope：既有未跟踪的design/playground/specs/research文件。
- mode：repair；required lenses：requirements/adversarial先并行，最终architecture。

### 待用户决策
无。

### 进展日志
- 2026-10-17：确认单批次契约。timeline当前刻意不返回完整转录，新增独立有界摘要字段保持该性能边界；图以内容范围而非音频覆盖表达，按来源展示缺口。

- 2026-10-17：完成两来源内容范围/缺口图、未整理色块、两行摘录及紧凑布局。JS边界与过滤测试3/3，core events测试45/45，workspace check通过；浏览器实际组件验证760×540和常规窗口、隐藏粗转保留现场笔记和概览、点击未整理色块回调正确。待最终签名桌面验证。

- 2026-10-17：Cross-review requirements、adversarial、final architecture均PASS，无阻塞发现、无需修复；范围与契约未扩张。协议兼容6/6、JS合计11/11（视图3+播放器8）、静态检查及release构建通过。
- 2026-10-17：最终签名包构建及codesign深度验证通过。旧GUI正常退出，旧debug core通过Quit正常退出；启动新release包，恢复重启前的双路记录意图。真实PID GUI2288/core2293均位于target/release/bundle/macos/hear.app；status确认两来源recording、capture_blocker=none。
- 2026-10-17：mandatory真机验收已完成：首页真实卡片显示粗转摘录和来源、两路图形和缺口、未整理色块可打开17:26—17:37详情，关闭后返回首页。最终截图：<USER_HOME>/.codex/visualizations/2026/10/17/01a14936-c656-7503-b05f-6efbc017a5ad/home-native-final.png。新包保持打开；无额外用户决策。长期录音稳定性仅属既有系统观察，未作为本轮新增保证。

- 2026-10-17：实现提交826bf33。整体门禁复核完成，最终共同typecheck/lint、diff检查通过；所有mandatory证据已关闭，单批次review覆盖最终源码。保留原有无关未跟踪素材/研究文件；临时合成验证服务及目录已清理。状态implemented。
