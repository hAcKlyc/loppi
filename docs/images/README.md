# 文档图像来源

## README 头图

`readme-header-v3.svg` / `readme-header-v3.png` 沿用现有字标与兔子，移除副标题并收紧高度；正文直接给出产品定位。PNG 使用项目已有 Sharp 从 SVG 导出，不修改品牌母版。

## README 概念说明图

`record-to-agent-v3.png`：声音和随手记录汇入 loppi，通过 CLI 或 MCP 供 Agent 访问。

说明图由内置 image_gen 生成，原图直接复制入库。提示词与来源见 [readme-visuals-v3.json](readme-visuals-v3.json)。

`processing-flow-v4.png`：用于「技术架构」，区分本地录音与粗转、云端按需事件分析与精转，以及 Agent 访问入口。由内置 image_gen 生成，原图直接复制入库；提示词见 [readme-visuals-v4.json](readme-visuals-v4.json)。

`record-to-agent-v3.en.png` / `processing-flow-v4.en.png` 为对应英文版，仅将说明图中的文字本地化，沿用原图结构与视觉风格。由内置 image_gen 编辑生成，原始输出直接复制入库；提示词与来源见 [readme-visuals-en.json](readme-visuals-en.json)。实际界面截图保持原样。

本页头图与说明图采用项目 [Apache-2.0](../../LICENSE) 许可，版权署名 Ethan L；品牌与商标使用见[品牌说明](../brand.md)。

## 界面截图

`timeline.png` 来自仓库内 `design/system-v1/preview.html`，使用 `scene=recording&platform=macos&clock=18:30` 的隔离合成资料，视口 1120 × 760。截图未连接 Core、录音库或云服务，也不代表某个已发行二进制的实机验收。

预览文件 SHA256：`a0a92a0a5b4e3a4d0622155c1bb96dd5ced2525dd9783073765ad12e34d50bab`。界面示例按项目 Apache-2.0 许可提供，版权署名 Ethan L。历史研究截图保留各自原始验收范围。

`island.png` 由用户提供的 `loppi_Island.png` 原样复制，展示实际灵动岛入口，不是生成图。截图本身不确认某个发行版本的验收状态；壁纸、系统及第三方标识归各自权利人，不包含在项目自有图像授权中。
