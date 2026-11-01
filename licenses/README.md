# 许可文件说明

根目录 `LICENSE` 是 loppi 的 Apache-2.0 原文，`NOTICE` 记录 Ethan L 的项目署名、MyAgents 来源与品牌边界。此目录保留用于现有安装包资源复制流程的 `loppi-apache-2.0.txt` 和 `loppi-NOTICE.txt` 副本；更新根文件后同步副本。

| 文件 | 范围 |
| --- | --- |
| `sources.json` 与各原生组件 `.txt` | 离线语音运行库的固定来源及许可，保留原有来源哈希 |
| `component-inventory.json` | 精确版本的全平台 Cargo 锁文件清单与 npm 生产依赖，不是二进制 SBOM |
| `frontend-third-party.txt` | 精确已安装 npm 生产依赖的版权与许可文本 |
| `rust-third-party.txt` | 按 Cargo 校验和核验的 crate 归档所含许可与 vendored 声明 |
| `rust-upstream-notices.txt` | crate 未附独立许可文件时，其固定源码提交中的补充声明 |
| `MPL-2.0.txt` | MPL 2.0 原文，按组件声明适用 |
| `tessdata-fast.txt` | 锁定中英文 OCR 权重来源的 Apache-2.0 原文 |

每条文本对应的来源、版本与内容哈希记录在清单中。Rust 源码下载地址和归档 SHA256 可用于获取完整上游代码；多重许可保留原表达式及明确选择，不因清单收录而将所有平台组件视为已链接到每个安装包。

完整分发说明见[第三方声明](../THIRD_PARTY_NOTICES.md)。来源文档中的第三方版权与作者名单需原样保留，不按项目的个人路径脱敏规则处理。
