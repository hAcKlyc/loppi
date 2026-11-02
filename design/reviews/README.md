# 首页审阅

这里保留首页布局对照的历史审阅材料，不是当前设计规范。`homepage-same-palette.html` 由 `source/build-homepage-review.mjs` 构建：构建时的候选方案导入生产 `src/App.tsx`，无补丁/专属覆盖；外壳消费同一套 tokens。保存的 HTML 不会随源代码自动更新。所有资料为合成资料，不访问 Core 或真实记录。

两个可选对照冻结自 Git：e705b71调整前0.1.17、b7c9d1c开放式条目。对照只统一当前色彩，保留其各自历史样式，不能作为最终源码。

`homepage-color-audit-before-token-refactor.json` 与 `homepage-same-palette.jpg` 留存重构前的16/18色审阅事实，不代表当前颜色数量。`homepage-old-layout-refresh.jpg`、`homepage-old-layout-760.jpg` 为当时生产样板截图（最后提交于 e705b71），不包含后续的中性卡片、录制标签与配色校准。当前规则见 `specs/DESIGN.md`；当前组件样板由 `npm run design:build` 重建至 `design/system-v1/preview.html`。实测证据保留各自日期与范围，不因旧文件中的“最终”字样而成为当前验收。
