# loppi · B4 / I4 视觉素材首版

日期：2026-10-01。用户选定 B4「好奇折耳」角色和 I4「折耳近景」图标为基准。
本包用于形象、尺寸和动效验收，未接入录音服务或生产入口。

## 图标母图

- `source/icon-square-generated.png`：image_gen 生成的原始满幅方图。
- `icons/app-1024.png`：1024×1024 方形母图，四角不透明，无边框、外留白、圆角、阴影或底座。
- `icons/app-{16,24,32,48,64,128,256,512,1024}.png`：方形尺寸导出；小尺寸先供辨识度验收，未逐尺寸手工像素修整。
- `icons/rounded-1024.png` / `circle-1024.png`：从方形母图裁切的透明派生版。
- `icons/loppi.iconset` / `loppi.icns`：圆角派生版的 macOS 容器，用于预览，最终应用到平台前需在实际 Dock 中验收比例。
- `icons/app.ico`：16–256px 的方形、多尺寸 PNG 容器；记录 Windows 图标规则，不表示客户端已适配。

母图不预制平台形状；保留母图，按用途施加遮罩。`source/icon-generated.png` 是已被替代的圆角底座探索图，仅供追溯，不能作为源图。

## 角色与动作

48×48 透明像素单元；九色固定调色板。坐姿脚底最后一行位于 y=44，底部留三行空白。
`poses/*.png` 是逻辑尺寸，`@4x` 是最近邻放大查看版。`sprite-atlas.png` 为 4×3 精灵图，帧坐标见 `manifest.json`。
呼吸帧保持整个脸部 y≤34 与脚部 y≥40 完全不动，只重排胸腹一格。眨眼从同一倾听帧派生，不更换角色脸型。

| 类型 | 素材 | 使用时机 |
| --- | --- | --- |
| 循环 | listening，12.8s | 实际正常倾听；低幅呼吸，偶尔眨眼 |
| 循环 | sleeping，9.6s | 实际主动暂停 / 尚未启用；闭眼呼吸 |
| 单次 | curious，0.62s | hover 进入，歪头回应；不持续摇摆 |
| 单次 | nod，0.52s | 保存确认后点头；不延迟入口收起 |
| 单次 | stretch，0.60s | 实际从暂停恢复后的小伸展 |
| 单次 | wave，0.80s | 有意互动的抬爪彩蛋；首版保留素材，不增加产品动作入口 |
| 单次 | pause，0.56s | 实际停止后合眼入睡 |
| 单次 | sleeping-response，0.50s | 睡眠中 hover / 保存确认，保持闭眼 |
| 静态 | waiting | 启停进行中或状态暂不可得；清醒，不微动 |
| 静态 | partial | 部分已启用来源不可用；清醒，一耳放低 |
| 静态 | interrupted | 异常全部停止；清醒垂耳，加静态琥珀提示 |

动画导出为 WebP 和 GIF。`interrupted` 的提示角标是播放器绘制的 UI 层，动画文件只含兔子本身；播放器是状态组合的参考。其他状态均不显示录制时间、波形或录制灯，不与音量关联。
过渡帧是首版有限帧像素动作，尚非逐帧手工精修。选定基准和节奏后再决定是否细化中间帧，避免一次批量生成不可控的角色漂移。

## 接入参考

通过本地 HTTP 打开 `index.html` 查看实际动画、深浅背景和 28 / 48px 尺寸；浏览器直接打开文件可能阻止清单读取。
`rabbit-player.js` 提供 `RabbitPlayer`：

```js
const rabbit = new RabbitPlayer(canvas, { base: '/rabbit-assets/' });
await rabbit.ready;
rabbit.setState('sleeping', { transition: true });
rabbit.playAction('nod'); // 睡眠中自动改用 sleeping-response
rabbit.dispose();
```

宿主依据实际采集来源设置状态，动画不能提前判定暂停、恢复或保存成功。状态改变会取消旧动作；单次动作完成后回到当前状态。页面隐藏时停计时器，显示后回到当前状态；遵循系统减少动态效果；卸载调用 `dispose()`。播放器按帧时长调度，不使用持续的 60fps 循环，也不轮询服务。
异常提醒去重、暂停动作、评论草稿、截图避让和窗口焦点由产品实现负责，见 `specs/prd/prd_0.1_quiet-rabbit-entry.md`，不在素材包内实现。

## 来源与可重建性

使用内置 image_gen 生成；生成提示词及引用见 `source/prompts.json`。按用户授权，生成后裁切、统一网格和颜色、派生呼吸/眨眼帧、导出尺寸和平台遮罩。
`build-assets.cjs` 仅使用本地 Sharp；产出姿态、精灵图、动画、PNG、ICO 与 iconset。`iconutil -c icns icons/loppi.iconset -o icons/loppi.icns` 生成 macOS 容器。
UI 预览中的截图、重点和评论都是视觉演示，不产生实际数据。
