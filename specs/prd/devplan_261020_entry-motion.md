---
type: devplan
status: implemented
created: 2026-10-20
updated: 2026-10-20
---

# 顶部入口连续动效

保留现有Tauri NSWindow + React/AppKit，按本次会话把展开调整为轻微回弹、收起改为短促过渡，兔子在两端位置间连续移动，并做可执行的真机验证。此为当前入口动效定向修正，不替换安装版，不操作用户录音或业务数据，不复刻Apple私有动画参数。

## 执行台账

### 开发契约
- 必赢场景：顶部收起→面板有一次小幅超调后稳定；面板→收起更短且不回弹；兔子沿同一窗口运动进度连续移动/变大/缩回，不在两处卸载重挂。操作、焦点释放立即生效，不等待视觉收起。
- 原子交付：动效、布局、命中、焦点/取消回归与可用本机验证一次交付；继续单窗口、单WebView、既有业务owner和generation取消语义。
- 复用：HearEntry.layoutAnimated/finishLayout、NSAnimationContext/NSWindow animator、Geometry、FloatingWidget.panelReady/layoutQueue、Rabbit组件及现有回归。
- 正确owner：AppKit驱动窗口几何及唯一运动时序；React用实际viewport尺寸派生兔子的位置/大小及背景显隐，不另开JS逐帧计时或轮询。原生管理命中与focus；core继续拥有业务事实。
- 新增机制仅限现有Geometry传递真实顶部遮挡高度与窗口横向位移、原生保存目标frame以区分视觉收起与已关闭命中；CSS的仿射位置映射是几何派生值。没有新owner、持久状态、队列、窗口或激活路径。
- 展开初值约300ms、单次小幅超调；收起约140ms。参数可按真机观察微调，不把贝塞尔近似声称为Apple原版物理弹簧。评论进入、屏幕/模式改变、首次显示及减少动态效果仍立即布局；收起动作可以有尾部视觉过渡但不保留旧操作区点击。
- 无刘海与刘海各端点保持现有布局；反馈出现/消失和后台重布局可立即归位，不承诺为所有消息变化追加形变。普通展开/收起必须连续，快速反向不得旧完成揭示新面板。
- 反向边界：不修改角色素材/业务动作、全局激活策略、浮动拖动行为、安装版、打包脚本或并行onboarding/定时录制工作。不扩大旧入口的硬件/跨系统发布门槛。

### 验收与结束条件
- 必须：全量typecheck/lint、入口JS/React回归、原生动效策略/命中/取消测试、Rust入口测试、构建与签名验证，requirements/adversarial后architecture独立审查PASS。
- 本机mandatory：隔离新包采样证实展开超调后稳定、收起时长更短且终点准确；观察兔子真实AX位置或界面中间帧，确认两端连续；评论输入/取消、快速重开不被旧完成破坏。不能只用mock替代本机窗口证据。
- 可用设备尽力：当前刘海2×显示器；外接无刘海/不同缩放/真实鼠标后台激活/IME/macOS13/睡眠仍需如实记录能力边界，沿用历史整体入口发布门槛，不属于本次额外承诺。
- 按影响重跑：生产代码变化重跑静态与相关回归；窗口/CSS变化重新构建并真机检查；文档不重跑构建。全部本次必须项通过后提交并关闭，不继续扩张。

### 行动清单
- [x] 窗口回弹/收起与连续兔子实现
- [x] 回归、构建与签名
- [x] 新包真机验收与边界记录
- [x] 独立三路review与必要修复
- [x] 提交及最终证据

### Review基线
- Batch base：`290dc751ad6fb56f89004387fbcdb1ee956a7dbc`
- 单Batch；范围：entry.m、entry.rs、build.rs、FloatingWidget.tsx、entry.css、entry-api.ts、相关入口测试与架构/RFC/本计划。
- 共享工作树其它文件全部排除。

### 待用户决策
无。

### 进展日志
- 2026-10-20：核对现有动画、原生命中及React生命周期。现有兔子在两个条件分支间重挂，无法连续；窗口和兔子需共享实际几何，避免新增第二个动画时钟。安装版不重启。

- 2026-10-20：轨迹检查发现直线经过物理刘海会遮住兔子；刘海屏先抵消窗口平移，保持兔子屏幕x直到其顶部落到安全带下方，再连续接到面板位置。仍只由实际viewport派生，无第二时钟。无刘海保持直接轨迹。

- 2026-10-20：首轮真机：兔子AX坐标从(833,2,28)连续经过刘海下方抵达(549,56,72)，收起约126ms；但NSWindow animator截断超调曲线，未形成回弹。改为同一个原生完成链：200ms到相对位移1.8%的超调frame，再100ms归位；沿用layoutSerial取消两段，没有新计时owner。必须重跑原生/真机构建与采样。

- 2026-10-20：requirements/adversarial各发现反馈气泡错误匹配展开样式；adversarial另发现原生外部点击预先写view=0导致收起时长归零。两项均裁决为本次阻断：选择器限定非collapsed；外部点击立即释放焦点/发取消，但保留presentation起点由entry_layout收起。新增真实CSS计算样式端点及原生pointer→layout时长回归。两路定向复查PASS；最终全量architecture审查PASS，没有新增owner或冗余状态。
- 2026-10-20：最终候选全量typecheck/lint、6项入口测试、React交互/节点身份/反馈样式回归、原生布局/命中/外部点击/取消测试、9项隔离Rust入口测试全部通过。早期共享工作树lint被并行首次设置文件阻塞；收尾时共享树typecheck/lint也已通过。新签名debug包构建与codesign --verify --deep --strict通过；只有既有Vite大chunk提示。

### 真机验收（最终候选）
- 本机为内建刘海Retina：1470×956pt、2×、safeTop32pt。独立bundle id `app.hear.entry-check`，独立HEAR_DATA_DIR，没有更改安装版、模型/权限或采集开关。
- 原生窗口+AX只读采样：最终普通展开从44×32到419×163，再回412×160，顶边始终y=0。首个移动样本31.450s、峰值31.601s、稳定31.755s，观察到约305ms运动（采样并非精确帧率计时）；两段回弹形态成立，最大宽度超调约7pt。
- 收起采样：上一轮相同动效参数从379×149(7.535s)经276×113、144×67、51×35到44×32(7.668s)，约133ms；最终包评论取消也从412×184连续收回44×32。140ms设定比展开短，不等待视觉结束才释放输入。
- 兔子真实AX框：收起(833,2,28,28)，先保持屏幕x约832–833向下移动，再经过(789,37,58,58)、(676,46,65,65)、(579,53,71,71)，最终(549,56,72,72)。超调时约(536,57,74,74)再回终点；收起反向连续返回。AX与窗口查询顺序造成少数样本不同帧，不把它们作为逐像素同步证明。
- 最终包连续执行打开→Escape→Tab重开，采样显示中途换向后仍稳定回412×160；操作区最终可用，旧完成没有重新揭示错误状态。取消代次和双阶段失效另有自动回归。
- 最终包评论实际聚焦textarea，粘贴测试文字成功；取消后焦点离开文本框，重开保留草稿。在隔离测试数据目录保存该测试评论，以检查真实反馈气泡；屏幕显示“评论已保存”，280×80收起窗口下气泡两侧各8pt，未出现偏右/窄化。没有写入用户日常数据目录。测试note保留于隔离目录，未截图/录音。
- 退出最终测试GUI和其隔离core；安装版GUI/core仍为原PID11609/11614，未替换或重启。

### 能力边界与复验
- 无刘海外接屏、混合缩放、macOS13、真实鼠标跨应用首次激活、真实中文IME组合输入、第三方全屏/Space和睡眠，本次未获得新证据，沿用旧入口整体发布mandatory；不把这次动效完成当作整体发布许可。无刘海及反馈轨迹端点有生产几何/生成CSS轨迹回归。
- CUA不移动真实系统指针，因此外部物理点击的命中无法用定点点击冒充；本次外部点击的140ms路径由原生产函数事件顺序测试覆盖。具体物理鼠标跨应用验收仍按历史清单人工完成。
- 本次完成定义是三项动效与可执行本机验证，不承诺所有外部输入设备/系统版本。没有重写窗口、JS逐帧动画、持久状态或激活重试；仅增加Geometry中派生几何和原生目标frame。
- 原生回归：`clang -fobjc-arc -mmacosx-version-min=13.0 -framework AppKit -framework CoreGraphics -framework QuartzCore scripts/test_native_entry.m -o /tmp/hear-native-entry-test && /tmp/hear-native-entry-test`。其它检查：`npm run typecheck`、`npm run lint`、`node --experimental-strip-types --test scripts/test_entry.mjs`、`node scripts/test_entry_interaction.mjs`、`cargo test -p hear-desktop entry::tests --lib --locked`。
- 隔离源码/包/数据目录：`/var/folders/n5/pzrmblj11dbc2ltb0ncd36s00000gn/T/hear-entry-polish-5lv855d_`。最终签名包在`target/debug/bundle/macos/hear Entry Check.app`；只读采样为`motion-first.tsv`、`motion-rebound.tsv`、`motion-final.tsv`。源码使用Batch base加本次拥有改动；entry.rs只带Geometry字段，避免混入同时开发的录制计划/首次设置接口。

- 2026-10-20：收尾在当前共享HEAD组合态再次通过全量typecheck/lint与9项Rust入口测试（另2项非入口测试未选）；已核对签名构建的本次原生/前端/测试源码逐字节一致。独立测试进程全部退出，三项动效、本次mandatory真机证据与三路审查闭环。

- 2026-10-20：实现提交`50e988b`。单Batch整体复核完成，本次动效与可执行真机验收已闭合；状态implemented仅指本devplan，不替代旧入口整体发布门槛。收尾只提交本计划证据，保留所有并行首次设置/打包等改动。
