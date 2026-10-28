# 人物指派回归证据

根因：WebKit 点击按钮或单选标签时默认不把焦点移到该控件。搜索框的 blur/focusout.relatedTarget 为外层 `dialog-surface drawer-layer`；SpeakerPicker.onBlur 误认为用户离开并关闭浮层，选项的 click 和后续 IPC 都没有执行。Chromium 不复现此默认焦点行为。

修复：SpeakerPicker 在左键 mousedown 时取消控件的默认焦点转移并显式聚焦 button 或 label.control，保留原生 click、单选选择、键盘离开、外部点击和 Escape。仅在用户确认后调用原有 await IPC 链路。

## 数据与版本排除

- 真实库只读：local_speaker_assignments=0；该表用于匿名组的合并。人物归属保存在 speaker_bindings。
- 真实库没有近期新增人工绑定，也没有本次尝试的新姓名。没有写真实业务库、改录音或声纹。
- 日志 request_id `810fea63-840c-4fd3-b7bb-209a5a199d94` 仅有 ipc_execute_failed，没有命令名，不能据此认定它就是指派失败。
- 见 ipc-matrix.json：已安装 0.1.10 / protocol 21 / bf161cc，以及 release 会话构建的 0.1.11 / protocol 22 / b4301d1，分别在全新库及已迁移真实库的隔离副本上测试。
- 四组都通过已有姓名绑定、新建姓名（没有音频，sample_count=0）、speaker_bindings 落库及 event_detail 身份投影读回。排除回滚版本与迁移库组合是本问题根因。
- 隔离 Core 仅 on-demand，关闭云队列并移除副本的凭据引用，不采集录音、不调用云服务。

## 验证

- `node --test scripts/test_speaker_picker.mjs`：真实 DetailDrawer / SpeakerPicker / IPC adapter，模拟 WebKit 的 mousedown 默认焦点事件；已有/新建/范围、await、错误保留、重试、标签刷新、关闭行为通过。
- `scripts/test_speaker_picker.html`：WebKit 与 Chromium 各 47 项通过；另以 Playwright 真实 locator.click 验证已有/新建/范围切换，真实 keyboard Escape 验证仅关闭 picker 并恢复入口焦点。
- `npm run typecheck`、`biome check src/SpeakerPicker.tsx` 通过。
- HTML fixture 的合成 Escape 事件已设 cancelable=true，业务失败模拟改为真实 Response::Error 结构。

浏览器测试使用模拟数据；IPC 矩阵使用独立 Core 和数据库。最终安装包仍需按下述路径做 GUI 验收。

验收路径：打开目标事件的文本转录 → 点说话人 → 选人物（浮层应保留）→ 确认 → 标签更新；或输入新名字 → 选新建 → 确认。缺声纹时显示“人物已保存 · 待补充声纹”。
