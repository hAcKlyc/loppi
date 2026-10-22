---
status: implemented
updated: 2026-10-23
---
# 0.1.5：PR #14 与设计系统联合发布

用户授权拉取、审查并合并 PR #14，将本对话设计优化一并更新版本、合到 main，完成 release 打包。采用补丁版本 0.1.5；不替换当前正在运行的开发包，不额外执行真实录音、历史恢复或付费模型评测。

## 执行台账

### 开发契约
- 基线：origin/main `0666e02`；PR #14 固定 head `37cec75dfc4c12776dc97f3c6ac180a565c11006`；设计优化 `28d3ae3`、`854b7b9`、`73e07e9`。
- 原子交付：一个组合候选，完整审查和验证后合并 PR #14，再经整合 PR 将设计优化与版本号合入 main；release 必须由最终 main 构建。所有 Git 提交包含 Conventional subject 与非空 body。
- 必须保证：PR #14 的样本连续性、事件完整覆盖、旧窗口/已提交输入稳定、空结果真实呈现，与设计系统的焦点、busy、窄窗及阅读规则同时成立。GUI 不接管 Core 事实或写入；保存/修订/请求ID与真实时间轴保持原 owner。
- 已有 source of truth 与允许机制：PR #14 和设计 devplan 的已授权机制保持原样；整合本身不新增状态、store、协议、重试器、主题或兼容壳。仅合并冲突、必要根因修复、版本对齐与交付记录。
- 范围：两组提交的完整组合、版本清单与锁文件、相关测试/规范/本台账。主工作区的未跟踪研究/素材与正在运行的独立 dev bundle 不属于变更范围。
- 非目标：重新执行已完成的历史恢复、真实付费语义评测或声音采集；安装升级当前客户端；扩大到任意语义正确、全应用 VoiceOver、其它平台保证。若新增修复使相关旧证据失效，才按影响重验。
- 适用：specs/ARCHITECTURE.md、两份既有 devplan、DESIGN；无 AGENTS.md/CLAUDE.md。

### 验收与门禁
- PR #14 精确 head、线上 CI 与 review threads；完整组合的 fresh requirements/adversarial 后 final architecture。
- `scripts/verify.py --suite all`，完整前端 build，版本一致；浏览器组合覆盖设计11组、9尺寸、18视觉及PR空结果/全空结果定位。
- 两组既有原生/私有/真实模型证据在对应代码及前提不变时沿用；只读审查私有数量结果，不复制录音、正文、库或凭据入Git。
- main 合并采用受审 head，等待整合 PR CI；同步本地主分支，确认 main 包含两组改动。
- release：GUI/CLI/Core/worker 构建身份为最终 main、版本0.1.5、干净源码；Developer ID逐层签名、DMG校验、Apple公证/装订/Gatekeeper通过。若公证配置不可用，明确报告阻塞，不宣称完整release。
- 仅文档收尾不重跑高成本业务/原生验证；发布产物的身份和签名须针对最终实际包检查。

### 单批次行动清单
- [x] 获取PR、整合设计改动与0.1.5版本，消除冲突。
- [x] 组合检查与三路review，修复/定向验证。
- [x] 合并PR #14、整合PR及CI；同步main。
- [x] 最终main release打包、公证、产物交付。

### 当前 Review 基线
- Base：`0666e02`，目标为本工作树完整候选；PR/设计范围按上文，细化契约在 review 前保存。

### 待用户决策
无。用户已授权审查、必要修复、合并、版本更新及release打包。

### 进展
- 2026-10-23：PR #14线上offline检查通过、无review或未解决thread；仓库允许merge/squash。GitHub连接器无该私有仓库权限，使用已认证gh。创建独立工作树，保留主工作区与当前dev包。

- 2026-10-23：详情唯一冲突已整合：保留共享 Tabs/TabPanel 与新排版，完整带入PR的空结果状态及按来源回听；新增回听入口使用公共 Button，间距使用现有token。空结果夹具复用fixture-controls进入native dialog测试层。版本清单/锁文件/helper build统一0.1.5（build5），typecheck/lint通过；全量离线门禁与浏览器组合验收进行中。

- 2026-10-23：完整离线14组、浏览器11组交互/18场景/9尺寸与部分空/全空回听通过。fresh requirements、adversarial 与最终 architecture 全部 PASS，无阻塞或证据缺口；生产候选固定。审查/验收记录保存于 `specs/research/evidence/2026-10-23-release/`。

- 2026-10-23：组合提交 `f3c1b45`；PR #14 以 `7c53b72` 合并，整合 PR #15 以 `206a1d9` 合并。PR #15 offline CI 与合并后 main CI 全通过；main 树与受审组合完全一致，本地主分支已快进同步。
- 2026-10-23：main `206a1d9` 的首轮正式构建通过。GUI/CLI/Core/worker 均为0.1.5、相同SHA/epoch且dirty=false；逐层Developer ID深签、DMG校验/只读挂载、Apple公证（Accepted）、装订与Gatekeeper全部通过。公证请求 `b5a7f748-315e-44a3-be45-dab8dca63f4c`。完成全部必做验收，无待决策或mandatory缺口。
- 收尾仅记录状态与证据，不改变生产代码；交付DMG会从包含此台账的干净main重新构建并核验身份/签名/公证，最终SHA、校验和与实际包绑定，保存在随包的release-info.json。原始开发包与用户数据保持原样。

### 交付与复验
- 版本：0.1.5（macOS Apple Silicon；helper build 5）。安装包：`build/releases/0.1.5/hear_0.1.5_arm64.dmg`，随包 `SHA256SUMS` 与 `release-info.json` 记录最终实际产物。
- 审查/功能证据见 `specs/research/evidence/2026-10-23-release/`。文档收尾不使业务验收证据失效，不重复真实模型/历史恢复/采集。
- 可选观感检查：查看时间线与详情在窄窗下的阅读层级、跨日片段以及笔记区域；空结果条目应标明来源和时间并点击后回听。主观微调不构成当前发布缺口。
