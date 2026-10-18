---
status: implemented
updated: 2026-10-18
---
# 从记录分布定位到详情时间

用户要求点击已显示分钟的概览轨道后，播放器和转录列表定位到点击时刻附近。

## 执行台账
### 开发契约
- 必赢场景：hover 17:54 后点击色块，详情播放器停在该绝对时刻，正文呈现包含该时刻或最近的文本块；即使文字在第 100 行之后也可定位。
- 原子交付：单批前端入口参数、已有详情分页与一次性滚动、回归、审查、签名构建重启。
- 用户验收追加：定位文本复用现有浅绿底/加重时间；暂停时即使落在句间空隙也标识最近文本，按播放来源筛选；播放时恢复真实时间区间高亮，不把间隙伪装成正在说话。无额外持久选中状态。
- 必须保证：点击复用 dayMapPoint/实际轨道几何与日边界；详情内夹紧到实际记录边界；不自动播放；正常列表/键盘打开仍从起点；后续刷新不抢用户播放或滚动；关闭淘汰在途详情结果。
- 明确不做：空白轨道创建/选择、改变录音来源、后端时间索引或新协议、持续自动滚动、播放状态持久化。
- 状态与生命周期：App 当前详情选择保存 item 和可选 initialAt；DetailDrawer 消费一次性打开时刻，既有 Core 详情接口是内容权威；现有 ContinuousPlayback 是播放 owner。
- 复用：dayMapPoint、TimelineView onSelect、App selected 更新、DetailDrawer load/bestSegments/分页/ContinuousPlayback。
- 新增机制：仅临时打开时刻、列表 DOM ref 与一次性定位标记；沿用分页补齐目标附近文本，无新依赖/持久状态/协议。
- 不变量：前端展示与导航，Core 继续唯一数据写入者；播放只由明确播放动作触发。

### 验收与结束条件
- 单测：夹紧/缺失目标、最近文本（命中/间隙/空/双来源）、分页目标是否已覆盖。
- 浏览器：真实 TimelineView → DetailDrawer（模拟 IPC）点击传时、100 行以后定位、暂停打开、刷新不重置、卡片/键盘入口、关闭迟到请求。
- 静态：typecheck/lint、相关 Node tests、frontend build。无 Rust 变化不重跑核心测试。
- mandatory：最终安装版实际点击、播放器与文本位置对应、双路录音正常；签名构建。
- 三路 fresh review、范围内问题修复、提交和状态收尾后结束。

### 行动清单
- [x] 入口参数与详情定位
- [x] 回归与浏览器验证
- [x] 三路审查
- [x] 签名部署、实际验收
- [x] 提交

### Review Contract
- Mode repair；base 19abc44。
- Scope: src/App.tsx, src/TimelineView.tsx, src/DetailDrawer.tsx, 新 src/detail-entry.ts, 相关 scripts/test_detail_entry* 与 scripts/test_day_map_cursor.html, specs/DESIGN.md, 本 devplan。
- Exclude: README.md/build_release.sh/scripts/package_macos.sh 与其余无关文件；已提交自动整理与 hover。
- 保证/非目标/允许机制：同上；只阻塞本批新增或明确负责的缺陷。requirements/adversarial/architecture 全部 required；最终签名部署在审查后完成。

### 待用户决策
无。

### 进展日志
- 2026-10-18：确认现有详情每次从零分页、播放器从起点初始化；将复用现有分页读取足够覆盖点击时刻的内容，并仅首次定位正文。

- 2026-10-18：入口透传 initialAt，详情复用既有分页读取至两条转录流均跨过目标或结束，播放器按实际范围夹紧并暂停，正文仅首次定位。分页游标改为 ref 避免自动定位后重复拉取；关闭时淘汰在途请求。
- 验证：typecheck/lint/frontend build 通过，相关 16 项 Node tests 通过；真实浏览器 scripts/test_detail_entry.html PASS：17:54:32 入口定位至第269条（17:52–17:55），后页可见、无音频请求、刷新不抢滚动、键盘/卡片默认起点、关闭后不继续翻页。测试使用整数屏幕坐标，其时间精度受轨道宽度限制，符合“接近点击时刻”要求。

- 2026-10-18：requirements/adversarial/architecture 三路 fresh review 均 PASS，无阻塞/返修。requirements 独立运行定位/时间轴/播放相关 21 项测试通过；实际 Core 两流独立有序分页已由两名 reviewer 核实。最终 architecture 确认现有选择与详情生命周期持有导航，不新增播放或数据 owner。

- 2026-10-18 安装验收进展：签名构建通过，停止旧进程后安装 /Applications/hear.app，revision 51→52；CLI/UI 双路录音正常。安装版键盘/可访问入口打开记录默认起点正确。坐标点击因 CUA windowNotFoundAtPosition 持续失败（普通/全屏、重绑定后均失败），已恢复普通首页并请求用户确认实际点击后的播放器与正文位置；此 mandatory 暂未关闭，不提交或标记完成。
- 最新 DMG：公证上传两次 HTTPClientError.connectTimeout，已分配 751bcb9a-881d-4b07-ba0c-2e48077b2eca / ab4c96e3-db8a-41c8-97e3-674045ec3702，但未取得成功上传/Accepted 证据。不得声称这个新 DMG 已公证；此前 hover 版本公证成功不替代新产物。

- 2026-10-18：用户真机确认播放器定位正确、正文已可见，关闭此前实际点击证据缺口；追加要求正文有明显选中态（附现有浅绿高亮截图）。已定位缺口：现有 is-current 只判断句段严格时间区间，点击落在句间时最近文本虽滚到屏幕内但无高亮。补充暂停时最近句段的派生定位高亮，播放仍按真实区间，不再要求用户重复确认已通过的入口行为。

- 2026-10-18 选中态修复验证：typecheck/lint/build 和 17 项相关 Node tests 通过；真实浏览器测试覆盖 17:54:32 处于第269句结束17:54与下一句17:56之间的空隙，最近句仍有浅绿选中态；播放时撤销间隙高亮，暂停恢复，切换来源选中对应句。原 requirements/adversarial 定向复审均 PASS，architecture 定向确认保留原完整 PASS；无新增状态或 owner。

- 2026-10-18：最终选中态版本已签名安装并重启，revision 52→53，首页确认双路录音；用户后续截图显示播放器16:12:05与高亮16:12:04文本对应，完成实际验收。新增“首次加载骨架”反馈另立小批修复，不重开本批定位行为。

- 提交：fc8af81；用户实际截图已验证最终定位与选中态，单批整体门禁完成。本次签名 App 已安装；DMG 公证仍属于单独分发事项，未声称新 DMG 已公证。
