# 薄荷兔 v2：双竖耳与趴睡

2026-10-07，用户确认两张imagegen生成图，并要求首页/notch/悬浮球共用状态和动画。

- `listening-generated.png`、`sleeping-generated.png`：生图原图，保留透明背景；提示词与参考素材见`prompts.json`。
- `src/assets/rabbit/poses/*.png`、`sprite-atlas.png`、`manifest.json`：实际生产素材。统一96px单元、脚底基线90，现有Rabbit按清单格子绘制。等待/部分/中断保留原形象和语义，所有入口共用。
- 正常循环6秒：两次约3秒呼吸，包含偶尔眨眼。三档0/2/4px头部上提和胸腹起伏，脚底固定；48px悬浮对应0/1/2px离散位移。
- 趴睡循环4.8秒：三档背部起伏；闭眼脸部和前爪固定。回应/启停也由新形象派生，不切回旧折耳图。
- `animations/listening.gif`与`sleeping.gif`：从最终清单导出的192px检查图；分别10帧6000ms、5帧4800ms。生产使用共用atlas播放器。

源图经裁切透明留白、整数尺寸归一及既有像素帧派生，不改变角色设计。atlas直接复制RGBA，避免透明边缘合成舍入与静态姿态产生差异。构建工具sharp仅用于素材准备，不引入应用运行依赖。

重建：`node scripts/build_rabbit_assets.cjs <sharp模块路径>`。若项目可解析sharp可省略路径；本机使用workspace dependencies的node_modules/sharp。验证：`node scripts/test_rabbit_assets.cjs <sharp模块路径>`，检查切片逐像素一致、透明度、正常耳形/脚底、睡觉脸部与旧形象闪回。

状态由core及EntryRuntime快照驱动；三处复用同一个Rabbit与清单，局部hover/点头只影响短回应，不改录音状态。不另建状态或跨窗口动画时钟。
