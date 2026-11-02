# 0.1.21 三平台正式发行

2026-11-03（北京时间），[v0.1.21](https://github.com/hAcKlyc/loppi/releases/tag/v0.1.21)已公开并设为Latest。版本未变，既有v0.1.18及其附件未修改。仓库已公开，项目自有内容采用Apache-2.0。

## 来源与实际验证

- 发行源码及tag解析提交：`727e0ffe00514a3fd3ce6fefc2dac1cd68db2f4e`，来自发行时的干净main。三个平台的GUI、Core、worker、CLI均为0.1.21、协议26、同一source、dirty=false，各平台内部构建时间一致。
- 精确main [Verify 41524386296](https://github.com/hAcKlyc/loppi/actions/runs/41524386296)的Mac/Windows成功；[Release 41524386506](https://github.com/hAcKlyc/loppi/actions/runs/41524386506)最终attempt3成功，保留先前已成功的原生产物，仅重跑发布复核。
- 首次发布已创建正确tag及空草稿，但错误地用只查询已发布版本的接口读取草稿。经三路审查的适配器修复`98a93d7`改为分页列表及捕获前精确筛选；从原干净727e0ff工作树调用既有finalize和修复客户端完成全部校验、上传与公开。未移动tag、重编译或重标产物来源；后续main的发布脚本及事实记录不改变这批二进制身份。
- 12项相关发行/真实CLI临时签名回归、脚本语法和排版通过。真实草稿及第二页历史Release读取验证通过；长发行历史不会把无关正文送入输出缓冲，权限/服务器错误仍会中止发布。

## 安装包

| 平台 | 字节数 | SHA-256 |
| --- | ---: | --- |
| [Mac Apple Silicon](https://github.com/hAcKlyc/loppi/releases/download/v0.1.21/loppi_0.1.21_macos-arm64.dmg) | 184,116,527 | `7714cc83816d79c0c901513520dac4d0c5cbe7f7efb6462478332331a577e4ae` |
| [Mac Intel](https://github.com/hAcKlyc/loppi/releases/download/v0.1.21/loppi_0.1.21_macos-x86_64.dmg) | 187,982,771 | `f796872007647dc8dfe2f3f6c65b0c8f6fc2bbd9f9bca960c611c8817d05aa86` |
| [Windows x64](https://github.com/hAcKlyc/loppi/releases/download/v0.1.21/loppi_0.1.21_x64-setup.exe) | 389,868,815 | `56a31749f45e21ddfcf54e28659a35614ab75df0d88d9b2513e03cc381652bfd` |

两Mac的App和DMG均完成Developer ID签名、Apple公证Accepted、票据装订/独立验证及Gatekeeper检查；两个原生runner上的包内只读模型烟测通过。签名凭据已配置在GitHub Actions Secrets，复用现有更新私钥；临时加密P12上传后删除，辅助文件已清理。

三平台更新签名均以已配置公钥及版本绑定重新验证，两个DMG的实际字节/摘要与报告一致。Windows实际安装包内10个CRT文件与源码锁定值及包内清单一致；三个实际包内24份源码许可/第三方声明逐字节一致。

13个公开附件包含两DMG、两Mac更新归档及签名、Windows EXE及签名、三份平台报告、SHA256SUMS和latest.json。公开状态、Latest、tag、所有附件字节/摘要及[匿名更新地址](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json)已通过完整发布校验和独立只读复核，三个更新平台齐全。

## 验收边界

用户确认当前版本的Windows最终安装、录音与升级验收通过，并要求三平台正式发行；不冒称Agent自行完成这些实机操作。Intel已有原生CI执行证据，但最低系统、更多实体设备、GUI长期使用和多日稳定性不能由CI推定。Windows安装包仍无Authenticode系统代码签名，更新签名已经验证。Windows10、Windows ARM64与Linux不在当前支持声明中。

缺少更新功能的旧公开版本需手动安装一次；使用此前本地0.1.21包的用户，也需手动安装本次正式包，同版本不会触发升级。现有依赖审计命中保留于[开源准备记录](../../docs/open-source-readiness.md)，本次未升级业务依赖或宣称所有安全检查通过。

完整元数据见[公开发行证据](evidence/2026-11-03-official-release/published-release-727e0ff.json)、[Mac公证证据](evidence/2026-11-03-official-release/macos-notarization-727e0ff.json)、[Windows实际运行库审计](evidence/2026-11-03-official-release/windows-installer-audit-727e0ff.json)和[Windows源码许可审计](evidence/2026-11-03-official-release/windows-source-license-audit-727e0ff.json)。早先300b9f4候选证据仅保留历史归属，不作为本次附件。
