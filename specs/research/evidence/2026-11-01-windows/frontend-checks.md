# 共享前端阶段证据

84项前端Node回归通过（设计导出待后端ICO/最终资源合并后重建，未把旧manifest当作最终通过）；含4项平台/真实setup组与截图失败检查及跨凭据后端错误映射。typecheck/lint/frontend build通过。

Mac浏览器中真实App+隔离IPC，模拟Win32 navigator的18场景×3窗口及减少动态通过；Mac呈现分支同样通过。Windows首次设置4项回归通过，未触及真实录音或云接口。权限事实仍由privacy/Core提供，platform只影响文案/样式。

上述仅为前端呈现证据。Windows原生标题栏/字体/DPI/入口/音频/凭据/安装验收仍待远端实机证据；所有发布硬门槛仍in-progress。

锁屏事实回归：Windows `system_audio=1` 且截图字段为0时，声音仍可准备；截图字段为1但缺少音频事实时不能冒充系统声就绪。真实Onboarding组件在截图不可用的合成privacy输入下保持完成按钮可用。Windows Agent接入样板使用PowerShell及Windows路径（只是合成展示，不证明本机CLI）。已向Windows Agent交付当前root前端精确补丁（SHA256 `7a5557d189357f4f39c66757ba9e2a5c1a89729b6d0c0f4fc0c3d198d2e920e1`）供原生组合测试，正式发布仍需干净统一源码。

最终本轮前端静态/构建：`npm run lint`、`npm run build`（含typecheck）退出0；84项Node退出0。仅受影响的首次设置/入口呈现另作定向验证，未将未执行的原生安装门槛标为通过。

原生候选合并后完整Mac verify通过，包括89项Node（含5项正式设计导出）。正式字标/ICON指纹/token/文件清单均通过；Windows ICO从批准母版派生并与运行资源同字节。Mac/Windows实际原生证据分目录记录，不以浏览器mock代替。
