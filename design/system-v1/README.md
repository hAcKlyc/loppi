# loppi · Design resources v1

本包附带[核心设计规范的离线副本](DESIGN.md)，其权威源为仓库 `specs/DESIGN.md`。规范定义用途；本包提供可复用母版、导出和真实组件样板。探索历史另存于 `design/identity/`。

| 目录/文件 | 用途 |
| --- | --- |
| `DESIGN.md` / `components.md` | 核心规范离线副本与组件状态速查；源规范在仓库 specs/DESIGN.md |
| `brand/wordmark-color.svg` | 彩色字标：炭黑主体、深暖红简单 i 方点；独立矢量轮廓，不依赖字体 |
| `brand/wordmark-mono.svg` | 单色印制/低色彩场景 |
| `brand/wordmark-reversed.svg` | 暖黑背景；奶白主体、深暖红 i 方点 |
| `brand/lockup.svg` | 角色与字标的横向组合；字标矢量、兔子保留嵌入的透明像素图 |
| `icons/` | 原有折耳歪头产品 ICON：完整方形母版、macOS 蒙版、多尺寸 PNG、iconset 与 ICNS |
| `rabbit/` | 与应用相同的透明姿态、动画图集、播放清单及倾听/睡眠 GIF 预览 |
| `tokens.css` / `tokens.json` | 从 `src/tokens.css` 导出；在源文件修改后重新生成，避免手动维护分叉 |
| `usage.svg` / `usage.png` | 基础色、字标留白、角色与禁用方式的速查样张 |
| `preview.html` | 离线真实 App 样板，使用合成资料，不连接录音、用户数据或云端 |
| `release/` | README 头图、分享封面、合成数据产品截图与中英文简述 |
| `manifest.json` | 源文件位置、文件尺寸与 SHA-256 清单 |
| `source/` | 原有产品 ICON 源图与保留规则 |

## 使用

- 字标净空至少一个 i 点宽；保持比例，建议显示宽度不低于 60px。普通正文中的产品名使用界面字体。
- 彩色字标用于纸白/浅底，反白用于暖黑底；单色版适用于只能使用一种油墨或颜色的场景。
- 产品 ICON 与倾听动画独立，禁止用双竖耳动画替换 ICON。
- 兔子使用组件给定尺寸与 pixelated/nearest，保留透明边缘；清醒与睡眠不能混用来伪装故障。
- 应用图标的正方形母版不含平台圆角；`app-macos-1024.png` 和 ICNS 是经过蒙版的导出，勿再次加蒙版。
- 界面浅绿表达完成/留存，暖红表达进行中，深暖红 i 点属于品牌；使用有足够对比的派生色显示小字。

## 样板与重建

打开 `preview.html` 浏览时间线与辅助页面。通过地址参数 `?scene=recording`、`?scene=detail`、`?scene=empty`、`?scene=error`、`?scene=crossday` 查看其他状态；`&reduce` 使用减少动态效果。业务写入在样板中明确拒绝，正式交互以应用为准。

在仓库根运行 `npm run design:build`。字标源为 `src/assets/brand/wordmark.svg`；图标源为 `assets/loppi-icon-source.png`；角色源为 `src/assets/rabbit/`。生成器只复制原有产品 ICON，不修改应用图标；其余资源从源文件导出，并从真实组件/内存样例构建离线预览。重新生成后运行 `node --test scripts/test_design_assets.mjs` 检查对比度、导出一致性与资源清单。

素材来源：字标为项目自制矢量；产品 ICON 原样沿用项目已确认的一折一直歪头形象；双竖耳兔子沿用项目 v2 透明母稿与状态动画，两者独立。系统字体不随包分发。本说明记录来源，不另行指定项目的开源许可证。
