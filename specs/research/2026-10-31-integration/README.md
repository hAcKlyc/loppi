# 0.1.14未合入工作与本地资料审计

基线：main 9c9cbd8（0.1.13）。本轮检查21个本地分支、16个实际远端分支、全部开放PR，以及原项目108个未跟踪文件；原文件全部保留。

真正未合入的5提交形成同一祖先链：5a529c0/69863c1（PR18转录分页）、154c53e/1374a97（原生拖动）、4bf6322（Agent页面）。本轮从最新main合入4bf6322，保留原提交祖先关系；冲突解决保留30条读取上限、增量追加/去重、独立说话人错误、NativeAudio输出和0.1.13生命周期修复。详细引用见[分支审计](branch-audit.json)。[PR33](https://github.com/hAcKlyc/hear/pull/33)已以2ef2b47合入main，[PR18](https://github.com/hAcKlyc/hear/pull/18)亦已自动标记MERGED。

`fix/agent-context-reliability`、`fix/local-failure-isolation`、`fix/partial-fit-publication`分别与PR19/17/16压缩合并的整体补丁完全一致；`fix/read-capacity`与`codex/review-pr-11`通过cherry等价核实。它们虽然不全是main祖先，但没有缺失代码。其余旧引用已是main祖先；不重复合并，不删除分支。

## 108个本地文件的去向

[逐文件去向表](untracked-disposition.json)记录原内容SHA256与分类。83个可移植原型源码、研究和计划纳入版本库，25个保留本地：16个指向外部技能的绝对链接、1个81条内容均与已跟踪资源相同的zip、7个生成页面/历史截图、1份含真实对话细节的样例说明。没有待合入产品代码被归入保留本地。

本次纳入的是历史资料：`specs/prd/_drafts/`仍是draft，其余旧devplan/研究中的版本、测试结论和状态仅描述当时情况，不构成本轮新增需求或0.1.14验收。私有录音、正文、案例输入/模型响应和凭据不纳入；匿名数字和公开模型元数据保留。已有`.gitignore`继续隔离研究private目录。

`playground/style-comparison`保留可生成的独立示例原型；构建已复核，不连接真实Core/云服务。生成HTML被忽略。2026-10-16性能实验固定使用2c81ebd历史源码，SQL脚本已修正为显式读取该版本，避免用当前重构代码冒充当时实验；重复运行成功，只代表局部合成暖缓存查询。

## 交付与验证

完整离线verify的21项检查全部通过（build/integration-0.1.14/verify.json）。实际浏览器滚动465句精转及4条未覆盖粗转：469行且无重复，失败可手动重试，刷新全前缀后scrollTop保持48477；Agent配置/复制/五种连接状态回归PASS。当前候选实际CUA四边吸附/向内菜单、拖动无额外重点、普通物理点击一次、退出重开配置与8条合成重点保持均通过。requirements/adversarial代码PASS，详见[审查记录](REVIEW.md)；最终架构完整diff审查PASS；历史编码harness按固定源码复跑32组PASS。clean发行与main交付均已完成，详见下方记录。

## 最终交付记录

- [v0.1.14正式Release](https://github.com/hAcKlyc/hear/releases/tag/v0.1.14)含DMG、release-info.json和SHA256SUMS三资源。源码tag为62b320aa8a9abc549d168d2b5cadd1abc1cba023；main代码合并为2ef2b478f2ce4821f99936b26f315e3bc866e77f，后续只更新本交付文档。
- 发布构建为Universal2（arm64/x86_64）；四组件clean、协议23、epoch1793423677一致。签名Team PMSG2Y8C6Z；Apple公证975b5faf-b359-4d47-80ac-07386abb82c0 Accepted，装订/签名/Gatekeeper及只读Unicode模型预检通过。
- DMG：202087685字节，SHA256 `dfe0cb230b73f97a9df1743a8ed856ac5ebe034b71d1db49852653b7653b6132`。PR33完整CI 2m48s PASS；本地21项离线门禁与三路独立审查PASS。
- 本地main复核工作树干净、旧HEAD未变后仅fast-forward同步。最终审计覆盖22本地/17实际远端分支：真正缺失代码0组，开放PR0；五个原待合提交均为main祖先。保留旧分支及全部108份原件；压缩/cherry等价引用不重复合并。
- 安装前备份0.1.13应用、一致DB、音频及GUI配置至`~/Library/Application Support/hear-update-backups/20261031-to-0.1.14`。原子应用交换后，关于窗口实际显示0.1.14；主窗口重开和Agent通用JSON、连接状态、实际复制成功均经CUA确认。
- 实时Core lifecycle与四个已安装组件均为clean62b320a；模型及说话人ready，双路录制正常，revision79、原8:00–23:00计划及意图/容量保持，新启动日志无Warn/Error。
- 六张保护表逐行保持：635事件、42986精转、2文字修正、2944说话人绑定、4人物、21重点；2266个已接受用量远端ID、2954个已提交/完成任务远端ID保持，SQLite quick_check=ok。未重跑付费历史任务。

本机验收与签名安装证据位于`build/integration-0.1.14`，含runtime/data-preservation、branch-audit和CUA截图。私有DB/媒体/日志不进入GitHub。当前macOS/Apple Silicon及内置屏已实测；macOS13/15、Intel和外接屏完整运行验收边界保留，不能用交叉编译或几何测试代替。
