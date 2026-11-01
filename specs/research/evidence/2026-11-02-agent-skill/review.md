# Cross-review 与裁决

## Phase 1

两个 fresh-context、只读 reviewer 完成 requirements / adversarial 审查。主 Agent 按现有契约核实并裁决：

| 发现 | 裁决与修复 | 受影响验证 |
| --- | --- | --- |
| relative LOPPI_DATA_DIR 导出相对来源（两路重复） | 阻塞；只在 agent_assets 用 std::path::absolute 解析派生来源，分段 join 产生本机分隔符，不改 Core data_dir | real CLI 新增相对 cwd/中文/空格用例、protocol/CLI Rust 测试 |
| GUI 丢弃新来源发布失败的具体原因 | 阻塞；保留请求代次控制，显示原字符串/Error message，未知错误保留通用提示 | typecheck、lint、真实 React 错误回归、浏览器故障状态 |
| context read dedupe 默认写成 true | 阻塞；按公开 schema 和 Core unwrap_or(false) 写为 false，明确 true 可能省略与精转重叠的独有粗转文字 | 新增 CLI help 对照公开 schema 默认值的回归、help 遍历与 CLI 构建 |
| if-revision 和 request-id 说明过度概括 | 阻塞；区分状态/对象/事件修订，说明仅重跑同一 CLI 参数不足以保证幂等，需原适用修订和最终负载，未知结果先核实；readme 同步 | CLI help 语义回归、help 遍历、CLI 构建 |
| vocabulary remove 必填 ID 被描述为可省略/改名 | 阻塞；改为从 list 取得必填删除 UUID | 实际帮助输出和 CLI 测试 |
| 采集验证秒数范围未说明 | 接受范围遗漏，但 reviewer 将两者都概括为 1–20 不准确；probe 依据 main.rs::probe 为 1–20，record-test 依据 record.rs::capture 为 1–60；分别修正，不改采集行为 | CLI help 语义回归；无需实际采集或重跑权限测试 |

## 修复后验证

- `cargo test -p loppi-protocol -p loppi-cli --locked` PASS：CLI 4，protocol 19；含新的 help/schema/修订/秒数语义回归。
- `cargo build -p loppi-cli --locked` PASS；`python3 scripts/test_agent_cli.py` PASS（3 项，含真实相对数据目录、绝对来源、中文空格、幂等、旧安装保护、无 Core 和失败分支）。
- `npm run typecheck` / `npm run lint` PASS；`node --test scripts/test_agent_access.mjs` PASS，具体失败原因和复制内容均验证。
- Windows GNU 目标 `cargo check -p loppi-cli --target x86_64-pc-windows-gnu --locked` PASS；原生 Windows 执行仍为 mandatory 待验收，不宣称已覆盖。
- 全量验证的 21 项 PASS 记录见 verification.json。修复只影响文档发现/帮助及 UI 错误，未重跑无关采集、签名或付费处理。
- 初次 macOS ARM64 开发包构建与随包 CLI readme/Skill/no-Core 烟测 PASS，构建 dirty=true；最终修复版完整开发包 PASS，日志 `/tmp/loppi-agent-dev-build-final.log`；随包 CLI 再次验证绝对中文空格来源、Skill 名称/版本/主动更新、dedupe help、MCP name/title 和无 Core：PASS。

## 定向复核与最终架构

原 requirements reviewer 定向复核 PASS（独立重跑真实 CLI 3 项）；原 adversarial reviewer 定向复核 PASS。全新 architecture reviewer 审查完整最终候选，结论 PASS：沿用 agent_assets / Core / platform 既有职责，单一可重建源文件，无未获授权机制或多余状态。所有本次阻塞项已关闭。

原子门禁：整份 PRD 已逐条核对，代码范围全部实现且适用验证通过；没有批次兼容壳。只保留 Windows 原生执行的 mandatory 发布验收缺口，PRD 维持 in-progress，按用户授权提交代码供验收，不发布安装包或替换日常应用。
