---
status: in-progress
updated: 2026-10-19
---
# 日期导航紧凑布局与连续位移

日期文字按实际内容占宽，收紧当前日期和两个相邻日期的距离。点击邻日及日历选日时，同一日期保留 DOM，以统一位置/尺寸过渡呈现左右移动；快速连点从当前绘制位置继续，减少动态效果时直接切换。

## 执行台账
### 开发契约
- 单批交付；仅修改 TimelineView 日期导航的局部布局/动画与对应回归夹具，不改日期选择规则、加载 owner、录音或后端。
- 复用 datePositions、chooseDate、useLayoutEffect 和既有动效 token；自然宽度布局 + FLIP 位移/文字缩放，不新增依赖、store 或协议。
- 必须保证：间距紧凑；保留日期左右移动连续；快速切换不中断跳回；日历/稀疏日期/窄窗可用；减少动态效果关闭动画。
- 非目标：整页翻页动画、无限日期轮播、数据模型调整；主观节奏微调 advisory。
### 验收与结束条件
- typecheck/lint/build；现有日期导航回归加入真实位移、中间帧、紧凑间距、连续点击检查；浏览器宽/窄布局及截图。
- 三路审查通过后签名构建、安装重启、恢复双路记录并提交。单批不重复全量审查。
### 行动清单
- [x] 实现及自验证
- [x] requirements/adversarial/architecture
- [ ] 构建安装提交
### Review 基线
- Base fce6381；范围 src/TimelineView.tsx、src/App.css、scripts/test_date_navigation.html、本台账；若设计规范变化则 specs/DESIGN.md。
- 其它工作树修改及未跟踪文件全部不在范围。
### 待用户决策
无。
### 进展
- 根因：按字号固定 5.4 倍占宽留白过大；CSS 宽度/字号过渡与 FLIP 位移混合，主要呈现字号变化而非统一横移。

- 静态检查/构建通过，时间线 10 项纯逻辑测试通过。真实 WebKit 日期夹具在 1280px 和 760px 均 PASS：紧凑间距、位置与尺寸中间帧、双向切换、打断续接、DOM 复用、日历选择、慢响应隔离。减少动态效果由现有 CSS/布局 effect 守卫直接关闭。

- Phase 1 requirements/adversarial 均无产品阻塞；共同指出夹具最终位移断言未豁免减少动态效果，属于测试问题。已按动画偏好分别断言位移/直接定位；不改变产品代码或已有正常动画验证结论。

- 最终 architecture PASS，夹具修正后再次 PASS。签名构建及 strict 校验通过，退出旧进程后安装并重启；原生主窗口19→18→19验证完成，revision56双路 recording。证据 /tmp/hear-date-motion-installed.png；仅主观节奏微调可后续反馈。
