---
status: in-progress
created: 2026-10-27
updated: 2026-10-27
---

# hear 0.1.9 release

用户要求升级版本号并打 release。将已提交的本机 Agent Context / MCP / CLI / Skill 与侧栏页面从 0.1.8 升为 0.1.9，制作沿用现有流程的 Universal2 签名、公证 DMG。交付本地包与校验记录；不包含共享工作区尚未提交的其他功能，不安装或发布远端 release。

## 执行台账

- Batch base：3b99836（已完成三路审查的 Agent 接入实现）。本批次仅拥有 Cargo.toml、Cargo.lock、package.json、package-lock.json、src-tauri/Cargo.toml、src-tauri/tauri.conf.json、src-tauri/native/HearCore-Info.plist 及本台账/交付记录。
- 复用：现有 build_release.sh → scripts/build.mjs、package_macos.sh、模型 lock/缓存、构建身份核验与已配置的签名公证凭据。不新增 owner/store/协议/运行逻辑/构建机制。
- 保证：版本均为0.1.9，helper build9；外部依赖解析不变；基于干净已提交源码构建，四组件同一 SHA/epoch、IPC21、dirty=false，均含arm64/x86_64；模型只保留既有一份，凭据和用户资料不进入包。
- 结束条件：机械版本与锁文件验证、fresh requirements/adversarial/architecture 审查、版本提交；独立干净worktree完整release构建、Developer ID签名、公证Accepted、装订/Gatekeeper、实际DMG内容/身份/Agent接口验证；保存包/sha256/回执并提交记录。
- 验证边界：沿用既有功能审查和录音测试，测试最终干净源码的完整离线回归与新增Agent子进程功能；不重复云模型评测、不执行真实采集，不宣称Intel真机或macOS13真机验收。版本/清单变更只影响静态及包身份，业务代码未变化，不重新展开功能设计。
- Review Contract：repair，固定本批次七份版本文件及此台账，base如上。三路required，前两路只读并行后architecture；仅本批次引入/暴露的版本和打包契约缺陷阻塞。实际签名公证回执是提交后构建的交付门槛，当前源码审查不把尚未生成的回执当缺陷。所有其他dirty/untracked内容范围外。

### 清单
- [x] 版本/锁一致、外部依赖不变、原有改动保全
- [x] 三路审查及版本提交
- [ ] 干净worktree构建、离线回归及包身份检查
- [ ] 公证/装订/Gatekeeper及DMG内Agent验证
- [ ] 交付产物、记录、清理临时worktree

### 进展
- 2026-10-27：确认上一发行版0.1.8，当前提交包含已审查的Agent接入；已有Universal2脚本、双Rust target、模型缓存及签名/公证配置，可直接复用。当前9份原有dirty tracked文件哈希已保存，不纳入此版源码。

- 2026-10-27：静态检查(typecheck/lint/workspace check)、结构化锁比对和9份既有文件保全通过；fresh requirements/adversarial/architecture 均PASS，无修复项。即将提交版本并在已准备的隔离目录切换到该提交，产物门槛待构建关闭。
