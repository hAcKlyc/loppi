---
status: in-progress
updated: 2026-10-18
---

# 详情首次加载骨架

详情首次打开时，转录和现场记录在各自数据返回前显示匹配布局的柔和骨架动画；成功返回空数据后才显示真实空态和零条计数。失败显示可重试的错误提示，后台刷新保留已有内容。保留点击时间定位、选中态和阅读位置。不改后端、全局请求框架或发布 DMG。

## 执行台账

### 开发契约
- 必赢场景：延迟响应期间不出现“还没有转录文字 / 还没有现场记录 / 0 条”；成功后显示内容或真实空态。
- 原子交付：单批次完成实现、验证、审查、签名 App 重建与重启，无数据迁移。
- 必须保证：区域独立完成加载；错误可见且可手动重试；背景刷新不闪骨架、不抢阅读位置；减少动态效果设置下不播放动画。
- 明确不做：新缓存、自动重试协议、后端与打包改动。
- 本次负责的问题：首次加载被误判为空，及新状态引入的失败/生命周期问题。
- 状态来源与生命周期：DetailDrawer 的 load/loadMoments 请求结果是真实数据；沿用请求序号淘汰迟到响应。detailReady 及新增局部 momentsReady / 两个读取错误区分未加载、成功、失败，随抽屉卸载销毁。
- 复用：DetailDrawer、MomentList、detailReady、detailRequest/momentsRequest、现有颜色和间距 token。
- 新增机制：仅组件局部读取状态和手动重试入口、骨架展示，无新 owner/store/protocol。
- 架构约束：前端仅展示 core 数据；加载状态由已有请求 owner 管理；设计 token 复用。

### 验收与结束条件
- 延迟转录/现场记录、非空成功、空成功、失败重试、后台刷新、关闭迟到响应、时间定位回归均由真实 React 浏览器夹具验证。
- typecheck / lint / build 通过；三路审查通过；签名 App 安装重启并确认记录意图保持。
- 仅前端变更，不重跑 Rust 全套；修改只重跑受影响检查。动画主观观感为 advisory。

### 开发批次与行动清单
- Batch 1
  - [x] 实现首次骨架和读取失败状态
  - [x] 浏览器及静态构建验证
  - [x] requirements / adversarial / architecture 审查
  - [ ] 签名 App 重建重启、提交与收尾

### 当前批次 Review 基线
- Batch base：be1964a
- 范围：src/DetailDrawer.tsx、src/App.css、src/tokens.css、scripts/test_detail_loading.html、本 devplan。
- 其它工作树改动不属于本次。

### 待用户决策
无。

### 进展日志
- 2026-10-18：确认空数组初始值被直接渲染为空态；现场记录依赖 detailReady，需要覆盖上游读取失败以避免永久骨架。

- 2026-10-18：typecheck、lint、build 和 17 项纯逻辑测试通过；真实 WebKit 加载夹具 PASS（延迟、独立完成、空/非空、两区域失败重试、背景保留、关闭请求），已有定位夹具 PASS。截图 /tmp/hear-detail-loading.png；减少动态效果通过 CSS media 覆盖确认。

- 2026-10-18：三路 fresh-context 审查均 PASS，无阻塞发现，无修复循环。签名 App 构建、严格签名验证通过，安装至 /Applications/hear.app 并重启；revision 53→54，麦克风和电脑声音均 recording。原生截图确认详情文字、时间定位与选中态正常；首次延迟窗口由受控 WebKit 夹具覆盖。
