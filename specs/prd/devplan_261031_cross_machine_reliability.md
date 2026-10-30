---
status: implemented
created: 2026-10-31
updated: 2026-10-31
---

# hear 两机可靠性整合与修复

用户已于 2026 年 10 月 31 日授权：先整合 GitHub issues 与本机全天审查，再自主完成全面修复、独立审查、Codex Computer Use 验收、版本提升、签名公证打包与本机升级。目标版本 0.1.13；源码基线 3651aa14ece9206fdfa499eb90086ad57129ad6f。工作分支 codex/cross-machine-reliability-0.1.13。

需求源为同轮对话、本机 2026 年 10 月 30 日审查 F01–F12、仓库所有 19 个 issues 及评论。合并后的逐项登记见 specs/research/2026-10-31-consolidated-issues.md。CLAUDE.md 和 AGENTS.md 在当前仓库不存在；以 specs/ARCHITECTURE.md、DESIGN.md、各 owner 技术文档及现有代码为不变量来源。

## 执行台账

### 开发契约

- 必赢场景：代表性历史库的日期页稳定打开；空间恢复后可手动恢复；语音任务在安全资源预算内有进展；输入拒绝不销毁健康模型；云账号故障有明确提示与抑制/恢复；采集在显示器和电源状态变化后独立、及时、可解释地恢复；没有静默丢帧、时间造假或付费历史回填；新版 GUI、CLI、Core、worker 一致。
- 原子交付：一份 devplan 一次发布。18 项登记逐项形成修复证据或有界调查/排除结论后统一发布；开发行动分组不形成中间产品阶段。
- 必须保证：Core/SQLite 保持唯一业务状态 owner；已有人工修正、媒体、远端 ASR ID 和用量历史保留；真实旧数据迁移幂等；暂停意图与供应商暂停、资源延期互不冒充；低内存/热压力仍有界；Core 和 GUI 启动/停止生命周期正确；每个源码缺陷有必要回归。
- 外部条件：账号充值不是软件修复，也不擅自执行；另一台电脑的私有原始证据及 macOS 15 真机不能假称已取得。对原问题的强结论必须基于能实际执行的验证；不能靠多等几秒、关闭保护或只降低日志等级宣称解决。
- 明确不做：重写产品、换模型/阈值、自动补跑历史付费任务、上传私人录音/库、复制或放宽 Keychain ACL、无界队列/缓存/重试、降低时间轴与持久性保护、静默删除音频、批量按进程名杀服务、自动关闭 GitHub issues 或给他人发消息。
- 本次负责：登记 C01–C18；#1–#4/#7/#8 的已修复基线只作回归，除非复现回归不重新打开原问题。
- 状态来源：录音意图/容量门闩/计划在 Store；云任务与队列在 events/cloud_queues；声纹在 knowledge/voice_observations；资源准入在 Governor；采集真实帧/故障在 source_process/record/native；GUI EntryRuntime 只作宿主投影；运行身份来自 Core 生命周期并传给既有日志 owner。
- 复用的抽象：reads::Pool/Snapshot、events::timeline/calendar_days、events::asr::local_analysis_failures、knowledge::VoiceWork/VoiceMatcher、resources::Governor/Work/Permit/Resident、AnalysisClient/OwnedChild、events::task_failed_with_retry/retry_task_in/take_task_fair、capture::AudioSender/AudioReceiver、source_process::Progress、capture_supervisor、LogWorker/UniLogs、EntryRuntime、EntryRequests、captureStatusView、src/playback.ts。
- 允许的最小架构扩展：在现有 owner 中修正 SQL/索引和统计维护、资源准入和任务进展；在既有 cloud_queues 中表达账号级阻塞及显式恢复，不借用用户 paused；必要的原生电源事实和旧系统音频排除能力仍归 capture owner；诊断只增加有界数值/枚举/关联 ID，复用 LogWorker，不建立第二调度器或业务存储；测试专用父进程绑定/离线隔离必须显式，不改变正式 Login 后台存活。
- 架构准则：架构正确、简单、可依赖；修 owner 和状态边界，避免为症状新增 cache/flag/fallback 壳。保持 macOS 最低版本及 Universal2 发行契约；原生兼容性和晚启动 GUI 播放排除必须分别验证。

### 验收与结束条件

- C01/C02：在本机 35.6 万块冻结库及非零失败合成集上验证结果一致、冷/热/并发读取不超三秒；保留取消预算，日期/DST/手工范围回归；同日期在途请求合并。
- C03/C11/C12：Normal/Constrained/Recovering/Critical 下轻量匹配与推理分别验证，长期等待有原因与恢复；不破坏 Critical 上限；坏输入保留健康模型；原声音质策略不擅自放宽。
- C04：仍不足/已恢复/恢复点击时再次不足/重启保持手动恢复，主窗口与入口一致，不伪造已录音。
- C05/C06/C07：真实本机录制与 native 故障注入覆盖灭屏、显示器恢复、停止/重启、零帧、溢出、时钟差、Sleep/DarkWake；保留自回放排除和正确缺口；若某平台实机证据不可得，如实记录，不能等同于全平台实测。
- C08/C09：模拟账号错误、显式恢复、过时目标、慢头/慢体/断连/持续失败，有限预算与用量 unknown 语义正确；不通过真实历史付费重试验收。
- C10：隐藏且状态未变时不无条件广播，前台恢复取得当前状态；原生状态入口仍更新，用户动作反馈不丢；对照观察 WebKit 重复错误与唤醒。
- C13：畸形 ASR 结构只记录安全的具体分支；保留严格时间与原请求；部分完成准确展示；回归重新识别与人工修正。
- C14：同一 run_id、单条 started、声纹 backlog、故障/模式/容量上下文、关键后台失败分支可查；有界脱敏且不保存正文/路径/凭据；不靠末态反推过去。
- C15–C18：明确测试实例所有权/退出/离线凭据；分解写入测量并判断是否实际额外开销；20ms 边界与客户端取消用受控用例结案，不编造缺陷。
- 静态与自动：前端 typecheck/lint，Rust check，受影响的有意义回归，最终完整离线 verify；真实模型及隔离 Core/IPC 必要集成；无私有付费调用。
- 独立审查：一轮 cross-review repair；requirements/adversarial 两个全新上下文并行，再对修复后候选做全新 architecture 审查；全部阻塞项关闭且 architecture PASS 才提交。
- GUI/OS：使用用户指定的 Codex Computer Use，实际观察并操作新版日期页、队列、人物、容量/控制和相关恢复场景，保存具体证据；只验证进程活着不算验收。
- 发布：0.1.13 四组件同源同 epoch、Universal2、模型校验/加载、同签名身份、公证/装订/Gatekeeper；升级前备份当前 app 和一致 DB，再替换 /Applications/hear.app，核实运行核心身份与实际 UI。
- 结束条件：所有代码项满足对应验证和独立审查；调查项给出证据边界与结论；外部账号和不可操作的异机环境单列，不把它们伪装成已修复。台账如实写 implemented 或保留 in-progress，最终说明实际安装/产物/剩余项。

### 开发行动清单

一个原子开发批次，下面分组只组织实施，不单独发布。

- [x] 拉取全部 issues 与评论，区分两台机器、版本、历史已关闭项。
- [x] 完成 C01–C18 合并登记、优先级、评估与逐项验收表。
- [x] 查询 SQL、优化统计、前端合并刷新与容量恢复。
- [x] 声纹调度、模型生命周期、覆盖边界和对应诊断。
- [x] 云端账号错误、慢请求/有限重试及部分 ASR 展示。
- [x] 原生采集、电源状态、恢复和实时消费者故障定位。
- [x] 有界运行诊断、测试实例清理、写入量与剩余调查结论。
- [x] 对应回归、冻结库验证和完整离线门禁。
- [x] cross-review、裁决、定向复核、架构 PASS。
- [x] Codex Computer Use 与本机 OS/模型/采集验收。
- [x] 提升版本、提交、签名公证 release、安装升级与最终运行验证。
- [x] 更新合并登记与最终台账并形成交付记录。

### 当前批次 Review 基线

- 批次：两机可靠性完整修复。
- Batch base：3651aa14ece9206fdfa499eb90086ad57129ad6f。
- 范围：C01–C18 所属的现有 Core/native/协议/GUI/测试/owner 文档和版本文件；review 前列出实际改动清单。
- 不在范围：原 /Users/ethan/Projects/hear 分支上的额外界面提交、108 个原有本地文件及所有用户历史内容。

### 待用户决策

无已知产品岔路。用户已授权自主完成修复、验收和本机发布。账户充值与无法访问的异机实测只记录真实边界。

### 进展日志

- 2026-10-31：基线和源码身份确认；19 issues（13 open、6 closed）及全部评论已保存为本机 intake。现有机器运行 0.1.12，原数据备份仍在。本轮尚未改生产代码。

- 2026-10-31：合并登记已完成，19个issue均有去重/历史映射，当前18项保留独立证据与验收。进入一个原子修复批次。

- 2026-10-31 01:45：C01/C02/C04 已完成首轮实现：失败子集索引、整月流式日历、启动/每小时有界 optimize、同日在途合并并保留变更后的新快照、容量恢复派生状态与手动继续入口。事件/日历 60 项、状态界面 10 项及真实 App 模拟 IPC 合并/失效回归通过。冻结库重测与原生最终验收仍待完成。
- 2026-10-31 01:45：C03/C11 首轮实现为安全降载模式中的有界声纹进展与输入拒绝不销毁模型；资源回归 8 项通过。C08/C09 增加账号阻塞事实、显式解除、8 次网络失败预算、流式响应和独立总时限；C13 增加畸形结果子码及部分失败展示。当前 Core 离线回归 368 passed / 19 ignored；尚未进入独立审查。
- 2026-10-31 01:45：C07 通过公开 IOKit 睡眠/完成唤醒通知抑制 DarkWake 重试，睡眠边界前的实际失败缺口单独保留。C14 合并 run_id、单条 started、容量/故障数值与声纹积压/模式历史。C15 显式测试运行者绑定与禁止测试实例访问钥匙串/生产云端已实现，集成验收待二进制构建。

- 2026-10-31：C05 根因实测补充：Codex Computer Use 实际启动/暂停短回放，Core Audio 进程事实显示活动输出为独立 com.apple.WebKit.GPU，hear GUI 本体不输出；GPU 的 PPID 为 1。仅排除 GUI PID 或降低 Tap 版本门槛均不能守住自回放排除，亦不使用私有 responsible-PID API 或排除所有 WebKit。用户已明确授权自主技术架构修复：必要实现为在现有播放渲染边界以统一原生输出替换 WebAudio 输出，继续由 ContinuousPlayback 管理双块预读、真实时间、清理与取消；原生层仅持有至多两个播放块和播放时钟，不拥有业务事实/任务。主窗口回放与声纹样例复用同一路输出；Tap 复用公开 14.2 进程排除和既有 source Progress 确认后才允许可闻回放。增加原生音频输出与排除准备的必要契约/验收，完整平台版本证据仍如实区分。

- 2026-10-31 02:18：356001 块冻结库在隔离 Core 上完成旧版/候选对照，凭据引用移除、两云队列停用、不读取正文到报告。日历、三个历史日和更早日期页、三个详情在已批准的部分精转标签归一后结果一致；候选四路并发所有请求小于 0.84 秒，无查询错误。详情比对脚本最初未移除 response type=event 的随机请求 ID，已校正，非数据差异。
- 2026-10-31 02:18：Rust workspace 再验证通过（Core 371 passed / 20 ignored，其余组件/协议/GUI库通过）；随后针对当前事件模块 132 项通过。一次完整门禁曾揭露新预算列与 transcript_tasks 的 SELECT * 列数耦合，已根因改为显式内容列投影。
- 2026-10-31 02:18：真实原生回放硬件时钟/双缓冲/完成/暂停/过期命令验收通过；前端原生输出取消与会话归属 3 项、连续回放 9 项和真实 App 刷新合并通过。原生电源通知受控注入验证不阻止睡眠，早期 Wake 不恢复，完成 Wake 才恢复。
- 2026-10-31 02:18：C15 运行者死亡/离线钥匙串隔离、Core/CLI 生命周期、诊断单条 started 与同 run_id 的实际 IPC 烟测通过。C12 首批128段音质拒绝复算均 RMS<0.005、无削波；门槛未改，正在补覆盖缺口样本。C17 20ms 尾部保留严格真实覆盖，未据此伪造填音。
- 2026-10-31 02:18：旧安装版隐藏主窗口 35 秒只读采样，Core 写入约184320字节（约5.3KB/s），GUI/WebKit无磁盘写入；这不能替代录制/积压负载测量，也不能证明异机10/29文件缓存脏页告警为10/30实际物理写入。候选/真实模型与采集分组测量待完成。

- 2026-10-31：本机隔离候选GUI实际验收完成：三历史日期、队列余额提示、人物样例、31秒跨块回放/文本定位/自然结束、容量不足到恢复再手动继续、双路真实录制与暂停。后台先录制后启动GUI，公开Tap排除代次更新且采集PID不变。灭屏13秒采样持续双路同session零丢帧；精确终止隔离麦克风helper后8.37秒恢复，系统声不中断。完整Sleep/DarkWake、macOS15/Intel真实硬件仍如约记录为不可得，不冒充已测。
- 2026-10-31：最终音质复核64段均仍低于RMS0.005，10段原声覆盖缺70–9999ms；文件存在不等于全区间覆盖。真实模型成功→3拒绝→成功同PID。C16按空闲/录制/声纹阶段分解；空闲旧新Core均约5.3KB/s，录制有SQLite与媒体工作，未据旧系统脏页报告进行猜测性缓存改造。
- 2026-10-31：候选完整离线门禁通过，补充C18观察取消与真实写故障分别验证；慢头/慢体/断连/不返回、部分精转失败、删除统计信息后启动均已验。冻结当前范围进入required独立审查；正式版本将在通过审查及提交后重新构建，当前dirty候选不当作最终发行身份。

- 独立Phase1完成：requirements报告C11输入阶段仍可能错误回收模型及C16活动积压证据缺口；adversarial报告C07来源已失败、无slot等待重试时睡眠缺口未拆分。均裁决为当前契约内阻塞/必要证据，未接受额外产品保证。C16已补真实有声18条声纹积压和公开语音双源6块/6任务计量；C11把回收放到实际RPC错误边界，输入准备错误统一保留健康模型；C07在Store观察原生电源代次时扫描已打开的session尾缺口，不依赖slot存活。定向验证/复查及最终架构审查进行中。

- C07定向复查CLOSED：Store原生电源观察先处理无slot的已打开尾缺口，六项定向gap/retry/sleep/journal回归通过。C11真实成功→坏头/截断拒绝→实际模型终止/重试→正常成功通过，输入拒绝PID保持，真实RPC故障换新PID。最终完整离线21项再次通过（Core375 passed/20 ignored）。C16活动语音/积压计量已补，等待需求定向复查和最终架构裁决。

- 最终独立审查完成：requirements定向复查R1/C11、E1/C16均CLOSED，adversarial定向复查A1/C07 CLOSED，全新architecture对最终完整生产差异给出PASS，无新增架构阻塞。唯一旧AudioContext文档句已按要求改为NativeAudio facade。补验隐藏主窗口后31秒回放，7块继续读取至结束，从Finder重开主窗后显示终点和可再次播放。开始clean commit发行阶段；签名公证/最终安装身份仍待实际产物完成后记录。

- 发行前新增C08竞态已用失败回归确认：HTTP402从process_cloud_task释放outbound gate后，旧错误可能晚于新配置/显式恢复落库。暂停旧2b740c0发行（已提交Apple公证但未安装/分发，产物标记obsolete），不把它当正式交付。后续小批次基线2b740c0，仅修main/events账号状态顺序：在已有gate内原子记录402，外层不重复回写，终态重复失败不再阻塞新配置。没有新锁、版本、凭据快照或状态owner；按新窄契约重新三路审查。原2b740c0的GitHub CI全21项通过，含macos15 runner实际原生输出测试；这不扩大物理采集平台实测声明。

- C08后续窄批次三路fresh-context审查全部PASS，无阻塞或必要证据缺口；新失败回归先红后绿、四项账号回归及最终完整21项通过（Core376 passed/20 ignored）。在现有outbound gate内记录账号事实，common handler跳过第二次402状态写，events只对真正变更的任务写阻塞，保留显式ASR重试和原付费ID。准备提交并从该clean来源重建正式包。

- 最终交付完成：生产提交2b740c0与56f6802，发行源码为clean 56f6802331c92562f296fae9df27b8b0cb331fb3，v0.1.13，协议23，四组件epoch1793388482一致。Universal2/最低macOS13、模型和Unicode只读路径烟测通过；Apple公证dd6c02af-7fec-4833-9024-85381a494f3a Accepted，装订及DMG/app Gatekeeper通过。正式DMG SHA256 a1d7a693bce7053b5c05e15702414f2691fc727a62a02d2b88436b7824c8a49a，GitHub发行已发布https://github.com/hAcKlyc/hear/releases/tag/v0.1.13，三附件完整且远端digest与本机一致。
- 已原子交换安装/Applications/hear.app；Codex Computer Use关于面板显示0.1.13，原声授权准备成功，历史日期正常打开。运行Core PID52631确认为上述clean构建，模型/声纹ready、Normal，当前run唯一started且验收窗口无warn/error。原08:00–23:00规则、计划暂停、10GiB上限与revision78保留，下一次10/31 08:00开始；未手动提前录制。保护的事件/精转/人工纠错/绑定/人物/现场记录六表与升级前逐行哈希相同，DB quick_check=ok。备份位于/Users/ethan/Library/Application Support/hear-update-backups/20261031-to-0.1.13，含0.1.12app、一致DB和媒体。最终记录specs/research/2026-10-31-release-0.1.13.md；其后文档提交仅留存已完成的交付事实，发行来源仍以v0.1.13/manifest为准。
