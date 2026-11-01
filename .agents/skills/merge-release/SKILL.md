---
name: merge-release
description: "Merge and release loppi after acceptance on dev. Use for merge release, /merge-release, 合并发布, 发版, or merging accepted changes into main. Prepare a new consistent version and user-facing CHANGELOG, verify dev, merge with --no-ff, then follow the repository's main-triggered Release Actions through three-platform builds, signing, GitHub publication and updater verification. Do not use for ordinary commits or a request to edit this skill."
---

# loppi Merge and Release

验收在 `dev` 完成，正式发行由 `main` 的 [Release 工作流](../../../.github/workflows/release.yml) 自动执行。该工作流等待同一 main 提交的 Verify 成功，构建 Mac ARM64、Mac Intel、Windows x64，再统一校验、创建版本标签、公开完整 Release 并验证更新地址。不要沿用 MyAgents 的「先公开空 Release、稍后补附件」。

用户请求合并发布已授权对应的合并、推送和正常发行步骤；按已授权范围执行，无须重复确认。修改本 skill 不构成执行发行的授权。

## 1. 固定本次发行范围

读取 [贡献约定](../../../CONTRIBUTING.md)、[更新发行说明](../../../docs/updates.md)、本次需求/验收台账和 diff。核实当前分支、远端、`dev`、`main`、上一公开版本及未提交改动的归属。工作区有其它任务时保留原样，在独立干净工作树完成发行，不把它们纳入发布提交。

本次功能、录音/Core 生命周期、签名安装、Mac/Windows 升级等适用验收应先在 dev 关闭。CI 测试不能替代未执行的 OS/录音验收；普通代码提交也不表示已验收。未决产品/数据迁移冲突按事实交还用户，不通过关闭门禁或改写历史绕过。

首次使用核对 GitHub Secrets 已按更新发行说明配置。不要输出、重建或提交私钥；不要自行改变仓库可见性。匿名更新地址要求仓库公开。

## 2. 在 dev 准备版本与说明

使用用户指定的新版本；未指定时按项目既有版本策略选择下一版本。已公开版本、标签与附件保持不可变。

同步现有版本源：`package.json`、`package-lock.json` 的两处版本、根 `Cargo.toml` 的 workspace 版本、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/native/LoppiCore-Info.plist` 的短版本，以及 `Cargo.lock` 中五个 loppi 包。核对锁文件没有顺带升级无关依赖；业务协议号只在协议实际改变时调整。

按上一公开版本到本次的用户可见变化更新 `CHANGELOG.md`，使用带版本和日期的二级标题。不要把未发行分支内部的修复、审查或重构流水账写成产品变化。按 diff 修正失真的架构/技术说明。

运行确定性版本与说明校验：

```sh
node scripts/release_metadata.mjs vX.Y.Z /tmp/loppi-release-notes.txt
```

它检查所有版本源并提取该版本的完整说明。`未发行` 区、缺日期或空的版本说明不能代替正式发行说明。

提交本次范围（Conventional Commits 标题及非空 body），推送 dev，等待 **这个 dev SHA** 的 `Verify` 成功。使用实际工作流 `Verify`，不要复制来源项目的 `Test` 名称或用旧 SHA 的绿灯代替。发现适用验收未关闭则停在 dev，报告具体缺口。

## 3. 合入 main，交给 Actions 发行

在干净工作树更新 main，使用 `git merge --no-ff dev`（或项目已明确采用的等价 PR merge），核对最终版本/说明/提交范围后推送 main。不要提前在本地创建公开 Release 或抢先打版本标签：标签和公开附件统一由 Actions 在全部产物校验后创建。

用 main 的精确 SHA 查找并等待 `Release`：

```sh
gh run list --workflow Release --branch main --commit <main-sha> --json databaseId,headSha,status,conclusion
gh run watch <run-id> --exit-status
```

Release 自身会等待同 SHA 的 Verify，再并行构建三个平台。不同平台、不同提交、不同版本或校验失败的产物不能混发。只有发布 job 有写 Release 的权限；私钥不会打包进产物。

版本已公开时工作流跳过重复发行。若本次原意是发新版本却被跳过，回到 dev 修正版本及验收流程；不要移动旧标签、覆盖附件或把单纯「工作流成功」称为新版已发布。

## 4. 核对实际发布结果

以 Release 工作流的最终产物校验和日志为依据，核实：

- main 源码、版本、tag 解析的提交和三个平台报告一致；Mac DMG、公证后的更新归档、Windows EXE、更新签名、三个报告、SHA256SUMS 和 latest.json 都在同一个 Release。
- Release 已公开且为 Latest，匿名读取 `https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json` 得到本版本、三平台和已验签 URL/签名。
- Windows Authenticode 状态如实记录；更新签名不等于 Windows 代码签名。

失败时读对应 job 的事实。优先重跑失败 job：已有草稿附件只有字节完全一致才会复用，不覆盖冲突附件。若已经公开但最后的网络检查失败，用相同 source 的干净工作树下载该 run 的三个平台产物，执行 `publish_update.mjs --verify-published` 做只读复核；不要删除或改写公开版本。工作流与本地脚本的参数以 [更新发行说明](../../../docs/updates.md) 为准。

## 5. 收尾

记录 main/发行 SHA、版本、Actions run、Release URL、固定更新地址的验证结果与仍存在的真实限制。更新交付台账状态，保留验收事实的原始源码归属。只有全链路成功时称为已发布；自动化配置就绪不等于执行过一次真实发行。
