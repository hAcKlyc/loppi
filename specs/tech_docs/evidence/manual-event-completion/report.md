# 人工确认结束：合成界面验收

2026-11-02，macOS ARM，in-app browser，1280×720。使用 `scripts/test_event_completion.html` 挂载实际 DetailDrawer、样式、Modal，IPC为隔离合成响应；不接触用户真实数据或云端。

- [confirm.png](confirm.png)：点击顶部等待状态，确认弹窗默认当前事件起止、双端可编辑，按钮与内容无遮挡。
- [menu.png](menu.png)：管理操作收进右上更多菜单，转录、播放与现场记录可直接使用。
- 直接确认：顶部变为「已确认结束 · 待精转」，内容保留。
- [uncertain.png](uncertain.png)：模拟提交回执丢失，时间输入冻结，按钮变为「确认上次提交」；再次点击后进入已确认状态，fixture校验复用同一请求ID。
- [reorganizing.png](reorganizing.png)：使用原生日期控件将结束分钟30调为31，提交后顶部显示「重新整理中」，原有效内容保留。

Node组件回归另覆盖默认毫秒精度、起止校验、取消与焦点返回、字段更改提交、失败/进行中刷新、菜单既有操作和Escape。Core回归覆盖重新整理实际发布、多结果全部完成、冲突/失败事务回滚、迟到自动结果保护、迟到音频补精转与回执重启。

边界：这是实际组件的合成运行验收，不代表真实云端模型或录音权限验收；本次未改变这些能力。

## 最终组合态验证

- `npm run verify -- --report /tmp/loppi-verify-completion-final.json`：21项检查全部退出0。含typecheck、lint、workspace check/test/build、121项前端回归、素材和Python回归、本机native回归、前端构建、四组件身份、隔离Core/诊断/test-owner运行检查。Core 402通过/21个既有私有或云端测试按原规则忽略；协议14通过。本次完成专门回归8项通过。
- 临时隔离真实Core IPC：未改确认、同请求回执回放、详情完成事实、改时参考任务、显式重试取得当前修订、无效请求拒绝，全通过。临时脚本用后删除。
- `./build_dev.sh --target aarch64-apple-darwin`：最终修复后完整构建成功，包内只读模型、Unicode路径与准备失败重试烟测通过。
- 开发包：`build/dev/0.1.20/macos-arm64/loppi_0.1.20_macos-arm64-dev.dmg`，SHA256 `009de8dd625c463726ad9d0289370befd3d414e86dc13b723269554ddbf0e951`。构建source为批次基线 `deb4ddf9efeca7062e7717fb26ae4715e280d541` + 本次未提交候选（dirty=true），协议26，macOS arm64，ad-hoc签名，无Developer ID/公证；这是开发包，未执行正式安装发布。
- 独立requirements/adversarial及最终architecture均PASS；发现并根因修复修订冲突后无法重试的问题，[审查记录](../../../research/2026-11-02-manual-event-completion-review.md)。

未覆盖：真实付费云端语义质量、真实录音与权限、Intel/Windows/最低OS实机、正式签名安装。本次未改变这些能力，无新增mandatory真机门槛。
