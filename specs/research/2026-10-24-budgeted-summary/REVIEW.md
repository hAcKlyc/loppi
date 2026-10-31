# 审查与验证记录

基线e9463dc04504a33731b4d47c0343c942b514a12c，范围见review-contract.md。9个既有版本/UI修改和其它untracked不属本批。

- fresh requirements/adversarial代码审查PASS：update证据规则、compress历史引用、非法项不消费旧句、旧提案兼容与生命周期复用符合契约。
- requirements提出长度边界：原文本可能更短，但分句补空格后更长。主评审按最终保存形式比较新旧长度，审计同步，新增9字符旧句/8字符原编辑/11字符保存结果拒绝的回归；定向复核PASS。
- fresh architecture审最终代码PASS：请求内派生长度/预算、纯合并，无新持久状态、队列、调用、重试或依赖。
- Core335 passed / 15 ignored，workspacecheck/build通过。无前端改动，不要求新UI测试；不安装替换应用。
- 候选1合成三遍实测发现一次最终1116超标、一个等长compress被拒绝、一处重要不同观点未在summary保留。保留原结果，不选择最好单次。通用修正是900写作预算、4–6主要议题组织、次要细节入要点、明显缩短和多主体立场保留；无样本专名规则。requirements及architecture定向确认范围，原架构PASS适用；修正后Core335/check/build再次通过。

候选2真实36次和合成6次验收、最终统计复核待完成。程序测试和代码PASS不替代模型长度/语义验收。

候选2六例18窗结构通过、最终814–984字符、逐窗最大1038；42/42摘要与42/42要点固定检查、6/6纠正通过。但一例额外复合观点丢失，主评审确认阻塞。候选3的通用替换说明/复合示例已获architecture定向PASS，Core335再次通过、workspace build通过；六例重新实测。

候选3最终合成6/6通过，真实36中34完成、32严格结构通过；一个重复summary键拒绝、一个provider读取失败、两个完成结果漏entity_id。早期锚点条件保留12/15，最长会议出现压缩丢构成/原因与宾语改义，未验收。候选4只作通用主次取舍/完整动作关系与字段顺序统一，requirements/architecture定向PASS，Core335重新通过、workspace build通过；新签名测试二进制与旧版隔离。合成全6重新开始，随后真实全36。

候选4合成最终6/6结构与长度通过、摘要41/42与要点42/42，但s02更新决定时省略优先级主张，未验收且不启动真实36。候选5仅恢复/明确summary update和understanding复合项观点保留，requirements/architecture定向PASS，Core335通过；新独立签名二进制重新六例。

候选5全部6回放完成，严格结构5/6（一次多余type字段）、固定摘要/要点42/42，长度通过；两处旧观点丢失，其中一处全部字段丢失，未验收。候选6从提炼粒度修正：决定与各人观点分别形成可独立修订项，旧复合项部分更新时拆分保留；示例同步更新。requirements/architecture定向PASS，示例未证客户身份改成团队。Schema与状态机制不变，冻结新prompt和同批字节，重跑六例。
