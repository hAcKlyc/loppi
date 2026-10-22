# 0.1.5 Integration Review Contract

1. Mode repair；fresh reviewers只读不修改、不派遣。Phase1固定候选，主Agent裁决/验证后final architecture审完整最终diff。
2. 需求：用户要求审查PR#14、没问题合并，连同今天设计优化升版并合main、release打包。范围与保证取 `specs/prd/devplan_261023_release-0.1.5.md`、PR的`devplan_261023_pipeline-reliability.md`和设计`devplan_261022_design-system.md`。
3. Exact base `0666e0262ce45ae051bd08c4d7fdf2beb26e3c32` → 本工作树完整候选。HEAD `73e07e9`，MERGE_HEAD `37cec75dfc4c12776dc97f3c6ac180a565c11006`。查看`git diff 0666e02`包括staged/unstaged；当前merge尚未提交。
4. In scope：上述git diff的tracked文件、release devplan和本contract。PR原始diff为`0666e02..37cec75`，设计为`0666e02..73e07e9`。node_modules/target是ignored缓存symlink，build为本地日志，不进Git。主工作区其他未跟踪素材/历史资料完全不在范围。
5. Review必须独立检查PR#14代码与两批组合，不以原三路PASS或CI代替审查。设计未受整合影响的行为可沿用已有审查及证据，重点核实跨批邻接回归。版本变为0.1.5，helper build5，不升级依赖。
6. 必须保证：样本连续性有同session和样本证明；真实gap/清理边界不抹平；原提交输入/remote ID稳定；观察完整同来源覆盖或明确未整理；旧窗口不因短窗策略自动重跑；派生空结果及秒级来源回听真实。保留设计系统Dialog/TabPanel/非模态、busy、最小窗口、对比度和长文本可读性。
7. 不新增业务owner/store/队列/轮询/持久化/重试器/主题/兼容壳。既有PR允许请求内覆盖契约及可选派生统计、原有有界重试；设计允许局部焦点及只读预览generation。业务状态仍Core/既有hook/播放器拥有。
8. Owned缺陷：本PR/设计/整合引入或显著放大，或两份契约明确要求。非目标：任意录音的语义保证、供应商必然有效输出、全VoiceOver、其它OS、重新付费评测或重复历史恢复；不把未变历史问题升级blocker。
9. 组合解决：唯一冲突DetailDrawer从设计版本保留整体结构，移入PR的playFrom指定来源、两处processingLabel统计、空段回听列表；回听Button复用公共原语，10px用space5。夹具复用现有fixture-controls使运行入口位于native dialog内。DESIGN增加空结果契约。
10. 验证：typecheck/lint已通过；`build/release-0.1.5-verification.{log,json}`记录正在进行的全量offline门禁。浏览器需设计11组/9尺寸/18场景+PR partial/all-empty实际回听定位与键盘；主Agent执行后补证据，不因还在运行立即判gap。原PR真实模型/冻结库/系统声与设计原生证据按代码影响沿用；不读取/复制私有正文/录音/凭据。仅清单版本变化不使原生焦点证据失效。
11. Required：requirements + adversarial并行，裁决后fresh final architecture；没有blocker/gap且检查通过才提交/合并。GitHub合并与签名公证属于此门禁之后执行的交付动作，不要求此刻已经发生。后续仅Git ancestry或状态文档变化不重开完整审查。

报告简洁给 PASS 或具体 blocker/required evidence gap，包含文件行、触发路径、契约依据，分开additional/pre-existing/out-of-scope。不要为更多假设无限扩大验证。
