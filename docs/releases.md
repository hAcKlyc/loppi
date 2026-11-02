# 版本与已知限制

安装包、源码和正在运行的应用是三个不同的状态。下载文件以 [GitHub Releases](https://github.com/hAcKlyc/loppi/releases) 的实际附件及其构建信息为准；功能分支或未提交工作树不代表已经发行。

## 当前分发

截至2026-11-03，[v0.1.21](https://github.com/hAcKlyc/loppi/releases/tag/v0.1.21)已正式公开并设为Latest，提供Mac Apple Silicon、Mac Intel及Windows x64安装包。13个附件包含安装包、签名更新资源、三份构建报告、SHA256SUMS和latest.json；[固定更新地址](https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json)已通过匿名读取验证。

本次安装包的源码及tag解析提交均为`727e0ffe00514a3fd3ce6fefc2dac1cd68db2f4e`。后续main中的发布脚本修复和事实文档不改变这批产物身份；应根据包内source、dirty、epoch与协议版本判断。完整来源、签名、验收及摘要见[本次发行记录](../specs/research/2026-11-03-github-release-0.1.21.md)。

| 平台 | 当前声明与证据 | 尚未覆盖的范围 |
| --- | --- | --- |
| macOS ARM64 | 最低部署版本13.0；原生CI、签名、公证、票据及包内模型检查通过 | 最低系统、更多设备与多日稳定性 |
| macOS Intel | 独立x86_64正式包；原生Intel CI、签名、公证、票据及包内模型检查通过 | 更多Intel实体机的GUI/录音长期使用及最低系统 |
| Windows x64 | Windows11 Build22000+正式包；用户确认安装、录音和升级验收通过；CI及实际包内运行库检查通过 | 更多设备与多日稳定性 |

两Mac均已完成Developer ID签名和Apple公证，三平台更新签名均已验证。Windows安装包仍无Authenticode系统代码签名。Windows10、Windows ARM64和Linux不在当前支持声明中；CI和单次验收不代表所有设备的录音、睡眠恢复、识别质量与能耗均已验证。

缺少应用内更新功能的旧公开版本需手动安装一次0.1.21；此前本地同版本候选也需手动安装本次正式包，同版本不会触发自动升级。

历史公开v0.1.18保留两份已签名、公证的Mac DMG，其源码为`4969899afd8d2e9d4dc632d568ef33a843cc56bc`，没有改写。历史候选台账中的哈希只属于各自候选，不替代当前发行包身份。

## 从 hear 升级

loppi 使用新的应用身份、数据目录与凭据配置，不读取或自动迁移旧 hear 数据。旧资料保留，首次使用需要重新完成系统授权、供应商配置和 Agent 接入。不要把旧数据库直接复制到新目录当作已支持的迁移流程。

## 发行记录规则

公开发行使用新的版本号和标签，保留已经公开版本的源码与附件身份。后续修复通过新版本发布。每次发行附平台、架构、签名状态、已执行验证、已知限制、源码身份和校验和。

当前源码、候选包、待发布证据与真正发行分别记录。历史台账中的版本只说明当时状态；新增文档不会改变旧二进制的构建身份。
