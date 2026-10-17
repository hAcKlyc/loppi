---
status: implemented
updated: 2026-10-17
---
# 首页卡片收紧、录制反馈与有记录日期导航

需求源：本轮对话四项调整。保留浅绿色风格，完成后重建签名桌面包并打开。

## 执行台账
### 开发契约
- 单批次原子交付：卡片来源/粗精转/状态/现场记录信息统一顶行；移除底部元信息行。正在录制且匹配当前素材的最新卡片呈现柔和流光，暂停/历史/来源停止时不动；reduced-motion使用静态浅绿。
- 删除分布模块底部解释和未记录区间展开两行；保留图例与悬停详情，不改时间范围含义。
- 日历按钮直接展开周一开头的标准月视图，可切月，仅有实际可见素材/事件/现场记录的过去或当天日期可选；空日置灰。顶部仅呈现选中月内有记录的日期（当前选中优先，至多两项相邻有记录日）；无记录时显示“暂无记录”，不伪造可选日期。
- core独占数据：月可用日期是现有事实的只读投影，按UI传入的本地自然日边界查询，跨日/时区正确。无新数据库、缓存、定时器或录音owner。空缺口不算记录，清理失效手动范围不算记录。
- 复用 timeline、visible_manual_ranges、recording_sessions、service_state、既有轮询/EntrySnapshot、dayBounds、现有tokens。允许新增 CalendarDays 只读请求/响应及TimelineItem可选recording_sources投影；旧Timeline响应缺字段默认空，不改变协议版本。当前签名包与CLI一起发布；旧core不支持日历时给查询失败提示，不伪造可用日。
- 录制投影只结合当前活跃来源、open session的最近确认尾部与卡片真实范围，避免仅因全局录音就把历史卡片点亮；UI用最新EntrySnapshot二次抑制暂停/失联。
- 不做：录音切段/状态机改造、AI摘要、跨月最近记录搜索、时间图重设计、业务数据迁移。
- 本次责任：以上四项及引入回归；不扩张原有录音长期可靠性保证。架构依据specs/ARCHITECTURE.md，设计同步specs/DESIGN.md。
### 验收与结束条件
- 纯函数测试：月份长度/跨年/闰年、recorded日导航、活跃卡片/暂停/历史/来源门控。
- Rust测试：月可用日空白/缺口/跨日/纯笔记/清理手动范围，当前录制尾部归属与旧会话排除；协议往返/可选字段。
- typecheck/lint/build/cargo check及相关既有测试；实际组件浏览器验证月切换/禁用空日/选择/紧凑顶行/动效与reduced-motion。
- mandatory：最终签名构建、真实桌面新GUI+core、日历与卡片真机验证，恢复原先录制状态，留在首页。
- 变更后只重跑受影响检查；最终三路review通过、mandatory关闭后提交并implemented。
### 批次
- [x] 数据投影与测试
- [x] 卡片/月历/动效与前端验证
- [x] requirements + adversarial，最终architecture
- [x] 签名构建、真机验证、提交与关闭台账
### Review契约
- mode repair，base 4b1776e（本轮开始HEAD）。三路required；Phase1固定快照，最终architecture。
- in-scope: src/App.tsx, src/TimelineView.tsx, src/RecordCalendar.tsx, src/timeline-view.ts, src/api.ts, src/App.css, src/tokens.css（如需）, crates/hear-core/src/events.rs, crates/hear-core/src/main.rs, crates/hear-protocol/src/lib.rs, crates/hear-cli/src/main.rs（响应穷举适配）, scripts/test_timeline_view.mjs, specs/DESIGN.md, 本台账。
- out-of-scope：原有未跟踪design/playground/specs/research文件。只阻塞本轮引入/显著放大的问题，禁止提高保证或扩张机制。
### 待用户决策
无。
### 进展
- 2026-10-17：核实当前数据与刷新路径，建立单批次契约；不沿用上一轮要求保留底部解释，因为本轮用户明确要求移除。

- 2026-10-17：实现数据投影、月历、合并卡片顶行和柔和流光；无新增录音状态owner。core events47/47、协议7/7、JS视图5/5，typecheck/lint/build/workspace check通过。浏览器实际组件验证：空日禁用、切月/选择收起、顶部不含空日、暂停消除live class；窄760×540可读，顶行高度16.5px，底部元信息已不存在。reduced-motion静态规则源码核实；当前浏览器不支持媒体偏好模拟，不修改用户系统设置，主观动效强度为advisory。

- 2026-10-17：requirements PASS。adversarial发现P2：快速暂停/恢复可复用旧recording_sources短暂亮起。裁决为本轮引入、违反历史卡片不假亮保证，阻塞。修复复用App既有timelineRequest序号：录制状态/来源阶段/修订/错误变化立即清除旧marker并刷新timeline，过渡前在途响应失效；不新增epoch/store/owner。需重跑静态、前端状态过渡烟测及最终构建，原Rust数据与日期证据不受影响。

- 2026-10-17：修复后typecheck/lint通过；完整App浏览器验证挂起旧查询→暂停→恢复→释放旧响应、来源重启两条路径，旧卡片均不亮。adversarial定向复核PASS，最终architecture PASS，无剩余阻塞项。
- 2026-10-17：最终签名构建成功，签名校验通过。旧GUI/core退出，新包GUI 6208、core 6213及两路采集6223/6224运行，status确认双路recording。真机月历验证10月1/6/7/17可选、空日/未来日禁用；选择7日显示历史记录且无流光，返回17日最新卡片正在记录且浅绿流光可见。顶部仅17/7/6日；元信息同首行；分布底部两行已删除。已留在今天首页。截图：home-calendar-final.png、home-live-final.png，位于本会话visualizations目录。mandatory全部关闭；仅动效强度主观偏好保留advisory。
- 2026-10-17：实现提交e43a555。逐条复核四项需求、反向边界与mandatory证据全部满足；最终HEAD共同静态门禁typecheck/lint/cargo check --workspace通过，单批次最终review后生产代码未变化。无中间态机制或待决策；清理自建浏览器fixture和服务，保留原有未跟踪design/playground/research资料。原子交付完成。
