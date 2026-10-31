# 0.1.14未合入工作与本地资料审计

基线：main 9c9cbd8（0.1.13）。本轮检查21个本地分支、16个实际远端分支、全部开放PR，以及原项目108个未跟踪文件；原文件全部保留。

真正未合入的5提交形成同一祖先链：5a529c0/69863c1（PR18转录分页）、154c53e/1374a97（原生拖动）、4bf6322（Agent页面）。本轮从最新main合入4bf6322，保留原提交祖先关系；冲突解决保留30条读取上限、增量追加/去重、独立说话人错误、NativeAudio输出和0.1.13生命周期修复。详细引用见[分支审计](branch-audit.json)。最终合并状态在下方交付记录补全。

`fix/agent-context-reliability`、`fix/local-failure-isolation`、`fix/partial-fit-publication`分别与PR19/17/16压缩合并的整体补丁完全一致；`fix/read-capacity`与`codex/review-pr-11`通过cherry等价核实。它们虽然不全是main祖先，但没有缺失代码。其余旧引用已是main祖先；不重复合并，不删除分支。

## 108个本地文件的去向

[逐文件去向表](untracked-disposition.json)记录原内容SHA256与分类。83个可移植原型源码、研究和计划纳入版本库，25个保留本地：16个指向外部技能的绝对链接、1个81条内容均与已跟踪资源相同的zip、7个生成页面/历史截图、1份含真实对话细节的样例说明。没有待合入产品代码被归入保留本地。

本次纳入的是历史资料：`specs/prd/_drafts/`仍是draft，其余旧devplan/研究中的版本、测试结论和状态仅描述当时情况，不构成本轮新增需求或0.1.14验收。私有录音、正文、案例输入/模型响应和凭据不纳入；匿名数字和公开模型元数据保留。已有`.gitignore`继续隔离研究private目录。

`playground/style-comparison`保留可生成的独立示例原型；构建已复核，不连接真实Core/云服务。生成HTML被忽略。2026-10-16性能实验固定使用2c81ebd历史源码，SQL脚本已修正为显式读取该版本，避免用当前重构代码冒充当时实验；重复运行成功，只代表局部合成暖缓存查询。

## 交付与验证

完整离线verify的21项检查全部通过（build/integration-0.1.14/verify.json）。实际浏览器滚动465句精转及4条未覆盖粗转：469行且无重复，失败可手动重试，刷新全前缀后scrollTop保持48477；Agent配置/复制/五种连接状态回归PASS。当前候选实际CUA四边吸附/向内菜单、拖动无额外重点、普通物理点击一次、退出重开配置与8条合成重点保持均通过。requirements/adversarial代码PASS，详见[审查记录](REVIEW.md)；最终架构完整diff审查PASS；历史编码harness按固定源码复跑32组PASS。clean发行与main交付待补。
