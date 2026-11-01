// Presentation only. Device readiness and permission facts still come from Core.
export function desktopPlatform(
	platform = typeof navigator === "undefined" ? "" : navigator.platform,
): "windows" | "macos" {
	return /^Win/i.test(platform) ? "windows" : "macos";
}

export function audioPermissionHelp(): string {
	return desktopPlatform() === "windows"
		? "在 Windows 设置 → 隐私和安全性 → 麦克风中允许桌面应用访问麦克风；在系统 → 声音中检查输入和输出设备。返回后自动复查；受系统管理限制时联系管理员。"
		: "打开系统设置 → 隐私与安全性 → 麦克风或屏幕与系统音频录制，允许 loppi。返回后自动复查；受系统管理限制时联系管理员。";
}
