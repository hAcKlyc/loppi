import type { CloudTask } from "./api";

export function taskStatus(task: CloudTask): string {
	const states: Record<string, string> = {
		pending: "等待处理",
		processing: "处理中",
		submitted: "等待识别结果",
		complete: "已完成",
		no_transcript: "无可识别文字",
		failed: "处理失败",
		superseded: "已由新范围替代",
	};
	const retrying =
		(task.state === "pending" || task.state === "submitted") &&
		!!task.error_code;
	const state = retrying ? "等待重试" : (states[task.state] ?? "状态待确认");
	if ((!retrying && task.state !== "failed") || !task.error_code) return state;
	const reason = task.error_code;
	if (reason.startsWith("keychain_read_"))
		return `${state} · 无法读取钥匙串密钥，请重新打开 hear；若仍失败，请在设置中重新保存密钥`;
	if (reason === "credential_missing" || reason === "provider_not_configured")
		return `${state} · 请检查云端配置`;
	if (reason === "audio_unavailable" || reason === "partial_audio")
		return `${state} · 原声不可用`;
	if (reason === "provider_unmapped_timing")
		return `${state} · 识别时间与原声不匹配`;
	if (reason === "provider_invalid_timing")
		return `${state} · 识别时间超出录音范围`;
	if (reason === "provider_invalid_utterance")
		return `${state} · 识别结果格式异常`;
	if (reason === "invalid_fit_boundary")
		return `${state} · 整理结果时间范围异常`;
	if (reason === "provider_status_45000092")
		return `${state} · 云端未找到识别任务，可重试`;
	if (reason.startsWith("provider_http_429"))
		return `${state} · 服务繁忙，请稍后重试`;
	if (reason.startsWith("provider_")) return `${state} · 云端服务异常`;
	return state;
}
