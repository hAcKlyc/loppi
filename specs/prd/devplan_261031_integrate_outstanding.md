---
status: implemented
created: 2026-10-31
updated: 2026-10-31
---

# 未合入工作整合与0.1.14交付

用户要求仔细检查全部未合并代码、PR及相关文件，合入main并更新版本。以origin/main 9c9cbd83bbd55be29d58a698bfd20e0f8e44297b（已发行0.1.13）为基线，检查21个本地分支、16个实际远端分支及原项目108个未跟踪文件。原始审核已确认真正剩下5个独有提交、3组已实现的界面工作；不把压缩合并过的旧分支重复带回，也不把历史draft材料变成新的产品需求。本轮目标0.1.14，一个原子交付。

## 执行台账

### 开发契约

- 必赢场景：PR18转录分页/连续滚动、原生兔子四边拖动/吸附、Agent连接设置优先界面在最新0.1.13可靠性基线上一起工作，完整验证后合入main并发布/更新本机0.1.14。
- 来源：PR18及其两提交5a529c0/69863c1；本地154c53e/1374a97原生拖动两提交；4bf6322 Agent页面提交。codex/agent-access-compact与本地codex/event-understanding指向相同4bf6322，其祖先已包含另两组。使用保留原提交祖先关系的单次integration merge，最终main包含5个提交，避免重复合并或伪造完成。
- 必须保证：Core/Store继续是业务事实owner；现有转录事实、人工修正、音频、付费远端ID和用量保持；详情分页仅投影已知时间前缀，精/粗与来源的覆盖、刷新/关闭/定位/失败重试正确；原生窗口拥有完整拖动与点击互斥、四边几何/跨屏/位置保存；React只投影；拖动无重点、普通点击一条重点；保留顶部模式/键盘/焦点、0.1.13原生回放/清理/epoch/系统声排除及隐藏广播过滤。
- 状态来源与抽象：DetailDrawer的load/detailRequest/initialScroll与detail-entry投影，ContinuousPlayback/NativeAudio既有输出；HearEntry.updatePointer/install/layout/hitRegion及EntryRuntime.preferences/save/EntryRequests；AgentAccess的既有连接信息/复制入口；版本与构建身份由既有build入口负责。
- 允许架构修正：仅合入已实现的现有原生鼠标owner及几何字段/私有事件调整、详情组件内IntersectionObserver/进行中与失败投影、纯函数detail-transcript；删除WebView异步startDragging。无新业务owner/store/Core协议/锁/后台队列/重试服务/兼容壳。协议若未改变保持23，版本统一0.1.14。
- 未跟踪文件逐项审计：有效、可移植源码/必要文档可纳入；本机绝对技能链接、重复生成归档、私有证据保留本地并在去向表说明，禁止盲目git add；保留全部原件。历史draft不激活开发、不把过时计划冒充当前完成状态。
- 明确不做：重写业务、换模型/阈值、付费历史重跑、传录音/正文/凭据、修改Keychain ACL、实现draft新功能、强制重置/删除用户分支或丢弃原工作树文件。未取得macOS13/15/Intel或外接屏实机不冒充已测；已有公开API与生产几何回归证据仍有边界。

### 验收与结束条件

- 分支/PR：所有实际未合入提交均在main祖先内，PR18与本轮整合PR合并；压缩合并历史通过aggregate patch/cherry等价核实；没有遗漏新远端head。同步本地main前复核其工作树干净且HEAD未变，仅fast-forward。
- 分页：密集精转/稀疏粗转100条边界、真正缺口、双来源、最后页；自动滚动在途合并，刷新已加载前缀、保持正文/位置，失败手动重试、关闭和跳转淘汰迟到请求；保留当前NativeAudio与部分失败标签。
- 拖动：7pt阈值、快速松手越阈值、点击/拖动互斥、取消/模式/屏幕变化、四边/负坐标/菜单/提示可见、位置保存。mandatory用隔离数据在Codex Computer Use实际拖到四边、检查菜单、点击重点与拖动计数及重开恢复；外接屏仅几何回归不能称实机。
- Agent：默认通用JSON、连接设置优先，MCP/CLI与Codex配置复制仍可用；不改Core接入/授权。
- 资料：108文件均有分类/去向；移植的源码可读可运行且必要时去除本机路径，资料无凭据/私人原文/私有截图；原件保留。未合入的产品代码不能因资料分类而遗漏。
- 验证：静态/有意义的Node、React、原生/Rust回归，最终完整离线verify；组合态真实GUI验收，requirements/adversarial fresh-context并行、裁决修复后fresh architecture PASS，未通过不提交。
- 发行：统一版本，clean源码四组件同epoch，Universal2签名/公证/装订/Gatekeeper；升级前备份0.1.13应用/一致DB/媒体并记录当时录制意图，更新后核实实际运行身份/模型/UI/原计划/数据保全。无相关代码变化不重复不受影响的昂贵私有验证。
- 结束：必做范围、验证及review完成后统一发行；最终台账implemented并提交留档，报告剩余真实外部条件。达到门禁即结束，不再扩展到不相关旧缺陷。

### 批次与行动清单

一个整合批次，以9c9cbd8为review base，资料作为同一次交付的审计部分。

- [x] 全分支/PR状态与压缩合并等价审计。
- [x] 完整读取未合入实现，冲突整合并审查组合行为。
- [x] 108文件去向与必要可移植资料整合。
- [x] 回归/静态/完整离线门禁与真实GUI验收。
- [x] 三路独立审查与定向修复/复查、架构PASS。
- [x] 0.1.14版本/提交、PR18及整合PR合入main、本地main同步。
- [x] 签名公证发行、本机更新与运行/数据核验、最终留档。

### 当前review基线

Base9c9cbd83bbd55be29d58a698bfd20e0f8e44297b，工作分支codex/integrate-outstanding-0.1.14，工作目录<USER_HOME>/.codex/worktrees/hear-update-0-1-12/hear。实际scope在review前列清；原项目和其它工作树的用户文件只读/保留，最终本地main仅干净fast-forward。

### 待用户决策

无已知岔路。用户已授权仔细检查、整合所有未合入工作与升级；本轮复用现有owner和已实现需求。

### 进展日志

- 2026-10-31：真实未合入为5提交3组；PR18仍open/conflicting，其余远端旧分支通过祖先或压缩补丁等价确认已在main；本地main落后7提交。原项目无tracked改动，108未跟踪原件保留。当前安装运行0.1.13，先保持正式服务正常，再做隔离验收与最终升级。

- 整合：保留main的30条单页限制、追加去重、独立说话人错误和NativeAudio；原生入口移除旧entry_drag而保留playback_output。定向分页/四边原生/React交互回归通过。108文件逐项去向已记录，83份可移植资料纳入，25份本地保留；原型构建与固定历史SQL复现通过。

- 完整离线21检查PASS；真实浏览器465精句+4粗尾共469行无重复，失败手动重试、刷新文本且scrollTop48477保持；Agent连接与复制回归PASS。四边原生生产函数覆盖通过，正在构建签名候选做实际桌面闭环。

- 当前组合候选0.1.14完成CUA：上/左/下/右吸附及屏内菜单可见；四次拖动与各一次已计划AX标记后合成moments依次3/4/5/6（每次仅标记加1，拖动不加），最终普通物理点击7→8；退出重开后配置逐字相同、8条合成记录保持。证据build/integration-0.1.14/desktop-acceptance.json及四边截图；实际内置屏，外接屏虽枚举到但未做手势验收。原生几何回归覆盖负坐标。正式0.1.13短暂停机后已恢复原enabled=true、paused=false录制意图。
- requirements/adversarial并行固定tree4180756相对9c9cbd8均代码PASS，无阻塞；各独立复跑13定向测试PASS，对抗10,000组数据215,929个前缀投影PASS。两位指出的当前候选CUA缺口现已闭环；最终架构、clean发行/本机更新/main合并仍待办。

- 最终architecture对完整119文件最终候选tree358c2d1审查PASS，无阻塞/新增架构岔路，独立复跑6测试PASS。三路审查门禁完成；历史media编码harness按固定2c81ebd源码复跑32组合成音调PASS，原型/SQL/编码三处可移植入口均复核。

- 交付完成：整合提交62b320a保留五个原提交，PR33以merge commit 2ef2b478f2ce4821f99936b26f315e3bc866e77f合入main；PR18由祖先关系自动标记MERGED。本地main工作树先复核干净且HEAD仍bf161cc，仅fast-forward至2ef2b47；当前整合工作树亦同步。最终审计22本地/17实际远端分支，0组真实缺失代码、0个开放PR；108原件哈希/链接保持。旧压缩/cherry等价引用保留，不删除分支。
- 已发行v0.1.14：tag源码62b320aa8a9abc549d168d2b5cadd1abc1cba023，四组件clean/协议23/epoch1793423677；Universal2 arm64+x86_64，Developer ID PMSG2Y8C6Z，Apple accepted 975b5faf-b359-4d47-80ac-07386abb82c0，装订与Gatekeeper PASS。DMG202087685字节，SHA256 dfe0cb230b73f97a9df1743a8ed856ac5ebe034b71d1db49852653b7653b6132；GitHub正式Release三资源已发布。
- 本机升级前已备份0.1.13应用、一致DB、媒体及GUI配置至Library/Application Support/hear-update-backups/20261031-to-0.1.14，原子交换应用后实际GUI关于显示0.1.14，主窗口可重开，Agent默认通用JSON/本机可用/实际复制成功。实时Core lifecycle返回同一clean62b320a身份，VAD/ASR与speaker ready，双路recording、revision79、原8:00–23:00计划/容量/意图保持；新启动日志无Warn/Error。
- 数据保全：635事件、42986精转、2修正、2944说话人绑定、4人物、21重点六表逐行相同；2266个已接受用量远端ID与2954个已提交/完成任务远端ID保持，DB quick_check=ok。未重跑付费历史任务，未外传正文/录音/凭据。
- 完整离线21项、PR33 CI（2m48s）、三路独立审查、合成浏览器/当前macOS CUA及签名安装门槛均已闭环。记录位于specs/research/2026-10-31-integration，私有/本机证据在build/integration-0.1.14。此后仅交付文档收尾，不改发行源码；macOS13/15/Intel/外接屏完整实机验收未声称完成。无剩余mandatory项或待用户决策，台账implemented。
