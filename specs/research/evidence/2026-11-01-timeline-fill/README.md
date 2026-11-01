# 轨道满高修正验证

实际 App 组件与隔离合成数据，使用项目 Vite 样板进行浏览器检查；没有读写正式录音或笔记。

- before.png/json：原记录段18px，轨道24px，上对齐后底部空6px。
- after.png/json、after-with-gaps.png、gaps.json：7个真实按钮均24px，top/bottom差0；两处实际缺口仍以局部5px纹理可辨。未来与空来源不填色。
- macos-visual.txt / windows-visual.txt：各18组场景/尺寸与减少动态模拟通过；包括760×540、1120×760、1440×900的时间线、详情、跨日、记录中、空和失败。
- macos-clock.json / windows-clock.json：各8个最小/常规窗口时刻几何通过，00:00、01:17、12:00、23:59标签在时刻线上方且不横向溢出。
- bottom-click.json：新增底部点击区域命中真实记录按钮，点击后详情定位21:05:07。
- keyboard.json：同一记录按钮Enter进入详情，定位起点18:10:00。

相关静态检查、34项Node回归与前端构建通过；native主题解析2项测试与Mac GUI编译通过。Windows标题栏与原生缺陷的实机证明另存Windows交付证据，不由浏览器模拟替代。
