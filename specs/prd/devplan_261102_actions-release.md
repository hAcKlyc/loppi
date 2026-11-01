---
status: implemented
updated: 2026-11-02
---

# 仓库内 Actions 发行流程

用户要求补齐 merge-release skill 及发布脚本，并询问开源后通过 GitHub Actions 全部自动化。当前仓库为 private；本任务不更改可见性、不上传密钥、不触发实际发布。以仓库内脚本和 Actions 完成三个原生平台的构建、签名、公证、资源归集、latest.json 与远端校验；merge-release 负责版本/合并/CI/验收收尾。用户已明确：dev完成验收，main直接正式自动发布；草稿仅是CI内部上传资源的短暂中间态，不增加人工确认。

## 执行台账

### 开发契约
- 必赢场景：新main提交触发 Mac ARM64/Intel 和 Windows x64 构建，全部成功后统一组装首次安装包、更新包、签名、报告、校验和与 latest.json；完整校验后在CI内部草稿归集附件并自动公开、设为Latest，验证匿名客户端实际读取的固定更新地址。main版本号不变时跳过已发行版本。
- 原子范围：loppi 自有 merge-release skill、仓库元信息/发布脚本、Actions 工作流、离线测试与运行说明一次交付；不改 MyAgents 仓库。
- 必须保证：同一版本/提交、三个平台齐全、签名与实际文件匹配；公开 Release 不被覆盖；部分构建不暴露更新；CI 签名信息仅通过 Secrets 与临时目录；资源重试只允许复用字节一致的草稿附件；无用户授权不执行远端变更。
- 不做：改变项目可见性、移动已公开标签、上传私钥、立即发布0.1.18、替代录音/升级的真实验收、引入更新服务器或自动调整锁定依赖哈希。
- 状态来源：版本来自现有 package/Cargo/Plist；发布对象来自固定 tag 的提交；签名与平台信息复用 updater.json 和 release-info.json；GitHub Release API 是远端附件/草稿状态事实源，latest.json 的匿名 HTTPS读取是客户端可用性事实源。
- 复用：build_release.sh/ps1、build.mjs/build_windows.mjs、mergeManifests、verifyArtifact、publish_update.mjs、现有 Verify workflow、CHANGELOG.md 与更新文档。只新增薄编排，不重复原生构建、签名和业务逻辑。
- 新增机制：共享发行元信息校验/说明提取；公开附件目录与校验和属于可重建构建产物；Actions job/手动发布入口，不新增产品运行时状态。

### 验收与结束条件
- 脚本：合成三平台且使用真实CLI临时签名的端到端离线测试；缺平台/错源码/错版本/错签名/附件冲突/公开版本/远端损坏拒绝；发布后匿名固定地址检查。测试不访问实际仓库或生产密钥。
- 工作流：YAML/表达式与actionlint检查；正确平台、锁定action、最小权限、main source/tag一致性、全部构建汇合、Secrets临时清理、无PR密钥任务、无提前公开。
- 技能：三个真实场景（正常新版本/平台失败/重跑与旧版本保护）的只读演练，核对指令使用真实仓库入口。工作流更新不代表执行实际发行。
- 审查：一轮 fresh requirements/adversarial，主Agent修复后 fresh architecture。只运行受影响脚本/静态检查，不重建无变化原生包。
- 外部前置：第一次带真实签名的 Actions 执行需要配置Secrets、确认runner锁定资源可用性；未执行就如实记录。已有自动更新的真实两版本升级验收仍约束正式发布。

### 行动清单
- [x] 发行元信息与完整附件准备、上传/验证/发布入口
- [x] Actions 三平台构建及发布策略
- [x] loppi merge-release 技能与发布说明
- [x] 离线验证、技能演练与独立审查

### Review 基线
- 开始HEAD：c9660f44bb28182f78ea2c923e66124322c32d36
- 基线保存：/tmp/loppi-release-workflow-baseline；共享工作树其它改动排除。

### 待决策
- 无。用户明确选择dev验收、main直接自动发布，不另设发布确认。

### 进展
- 2026-11-02：MyAgents 有 merge-release，loppi 暂无；原技能允许空附件公开，不适合当前自动更新契约。现有脚本仅生成更新清单/上传已有草稿，需补齐完整附件及远端检查。

- 2026-11-02：用户纠正发布分支契约：dev验收，main直接自动公开；工作流将以main push触发，在exact-SHA Verify通过和三平台完整资源校验后自动建tag/Release。

- 2026-11-02：main正式发行工作流、完整附件/远端/匿名地址验证与loppi专用skill已实现。actionlint1.7.12通过，21项版本/签名/资源/流程回归通过，类型/格式与技能frontmatter/链接检查通过。未修改仓库可见性、未上传Secrets、未触发远端workflow/发行；首次真实CI运行属于启用前置，不能冒充已执行。
- 2026-11-02：fresh requirements通过并完成三个技能演练；adversarial发现长时gh watch输出可能超过默认缓冲，修复为CLI边界流式输出，2MiB回归与原reviewer定向复核通过；fresh architecture最终PASS。修正开发指南中“所有CI均无签名凭据”的旧陈述，区分普通Verify与main发行。记录见specs/research/evidence/2026-11-02-actions-release/review.md。本devplan仅完成流程编写，真实CI启用条件和自动更新真机验收仍需在正式发行前关闭。
