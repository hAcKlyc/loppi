---
status: in-progress
updated: 2026-11-03
---

# Windows Skill 并发发布修复

将已验收的0.1.21源码合入main并公开仓库时，精确dev提交`d370f4a55747012cff26d887a7498a2a427f2119`的Verify在Windows失败：`agent_assets::tests::concurrent_publication_exposes_only_complete_files`返回`Access is denied (os error 5)`。修复同一派生文件的并发检查/替换冲突，保留版本、技能内容与安装语义。

## 执行台账

### 开发契约

- 必赢场景：多个线程/进程并发发布同一官方Skill，发布成功且最终文件属于某一完整版本，临时文件清理干净；Windows原失败测试通过。
- 原子交付：一个修复批次，包含生产修复、回归与审查，再推送dev验证；不把现有本地0.1.21安装包冒称为本次新源码构建。
- 必须保证：目标检查、幂等判断、临时文件替换共享同一个目录发布锁；保留原子替换、私有权限、无覆盖用户自定义安装目录的既有语义。
- 明确不做：不改版本、不改LLM或事件流程、不升级依赖、不引入自动重试、不扩展安装为跨文件事务、不处理外部非协作进程阻塞文件。
- 本次负责的问题：当前Windows Verify暴露的官方派生Skill并发发布失败及本次修改引入的缺陷。
- 状态来源与生命周期：官方Skill由`agent_assets::publish_at`从内置模板派生；锁仅覆盖本次检查/发布调用，Windows既有`.publication.lock`留存以维持跨进程锁身份，不承载业务状态。
- 复用的既有抽象：`platform::open_directory`、`fs2::FileExt::lock_exclusive`、`platform::rename`、`platform::set_mode`。
- 新增架构机制：复用已有目录发布锁到正确的派生文件发布owner；无新store、业务owner、协议、重试或fallback。
- 架构不变量：Core业务状态唯一写入者不变；平台文件语义由`protocol::platform`提供，跨平台共用发布过程不分叉。

### 验收与结束条件

- 保留幂等、替换旧版、无临时残留、无效/符号链接目标保护测试，并使并发回归覆盖不同完整内容的竞争替换。
- 本地`cargo check --workspace --locked`、protocol回归、workspace构建通过；完整dev Verify提供真实Windows文件系统执行证据。
- 此改动不影响前端语义；本地静态门禁及CI完整门禁保留。Windows真实CI文件系统测试足以覆盖本次故障，不涉及录音/GUI/安装权限，其他既有真机验收缺口继续保留。
- 生产/测试代码变化使对应Rust证据失效；仅台账结果更新不使代码检查失效。精确dev SHA仍需完整Verify通过才能合入main。
- 三路审查完成、阻塞缺陷修复、受影响检查与Windows CI通过后结束；依赖审计和既有发行凭据缺口只如实记录，不扩展本批次。

### 开发批次与行动清单

- Batch 1（同一根因与owner）
  - [x] 复用目录发布锁，增强并发回归。
  - [x] 自验证与三路审查。
  - [ ] 提交、推送dev并通过精确SHA的Verify。

### 当前批次Review基线

- Batch base：`d370f4a55747012cff26d887a7498a2a427f2119`。
- 范围：`crates/loppi-protocol/src/agent_assets.rs`及本台账。
- review mode：repair；requirements/adversarial/architecture均required。只审本范围与上述保证，不回卷已验收功能和既有依赖风险。

### 待用户决策

无；属于已授权合并操作中的常规失败修复，复用现有抽象。

### 进展日志

- 2026-11-03：Verify [41498974464](https://github.com/hAcKlyc/loppi/actions/runs/41498974464)的Mac通过；Windows类型、lint、Rust check及Core 403项回归通过，protocol并发发布失败。原始报告保存在本机忽略构建目录及对应Actions artifact。
- 2026-11-03：目录锁覆盖目标校验、幂等比较、完整临时写入、原子替换及失败清理；Windows沿用平台层永久锁文件。并发回归变为4轮、每轮8个同步竞争的不同完整版本。
- 2026-11-03：修改文件rustfmt、前端typecheck/lint、workspace check/build、protocol全19项测试通过。8个实际CLI进程使用隔离Unicode/空格路径同时执行readme，全部成功、结果一致、无临时文件；未连接Core或读取真实录音。全workspace rustfmt命中基线onboarding.rs的既有排版差异，未扩大范围。
- 2026-11-03：fresh-context requirements、adversarial均无阻塞发现；最终architecture为PASS。未新增owner/store/重试/协议。Windows精确dev提交的完整Verify仍是合入main前的必需证据，待本次提交后取得。
