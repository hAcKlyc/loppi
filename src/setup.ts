import { invoke } from "@tauri-apps/api/core";
import {
	type CloudProvider,
	type CloudQueue,
	type CoreResponse,
	core,
	type ServiceState,
} from "./api.ts";
export interface ModelState {
	phase: string;
	speaker_phase: string;
	progress_percent: number;
	error: string | null;
	speaker_error: string | null;
}
export interface PrivacyStatus {
	microphone: number;
	microphone_available: boolean;
	screen_and_system_audio: number;
	tap_required: boolean;
	tap_ready: boolean;
}
export function setupBlocker(
	state: ServiceState | null,
	models: ModelState | null,
	privacy: PrivacyStatus | null,
	queues: CloudQueue[],
	error = "",
): string {
	if (error) return "请先重试读取失败的项目";
	if (!state || !models || !privacy) return "正在读取准备状态…";
	if (
		state.disk_free_bytes <= 1_073_741_824 ||
		state.audio_used_bytes >= state.audio_limit_bytes
	)
		return "录音空间不足，请释放磁盘空间或到设置中调整音频上限";
	if (models.phase !== "ready")
		return "请等待本地语音组件检查完成；失败时可重新检查";
	if (!state.microphone_enabled && !state.system_enabled)
		return "请至少选择一种声音来源";
	if (
		state.microphone_enabled &&
		(privacy.microphone !== 3 ||
			!privacy.microphone_available ||
			state.microphone_phase === "unavailable")
	)
		return "请授权或取消选择麦克风";
	if (
		state.system_enabled &&
		(!(privacy.tap_required
			? privacy.tap_ready
			: privacy.screen_and_system_audio === 1) ||
			state.system_phase === "unavailable")
	)
		return "请授权或取消选择电脑声音";
	if (
		!["llm", "asr"].every((p) =>
			queues.some((q) => q.provider === p && q.configured),
		)
	)
		return "请配置两项云服务";
	return "";
}
// Only operation identity survives view changes. No credentials are cached or
// persisted here. Unknown results are queried, never automatically resubmitted.
const cloudOperations = new Map<CloudProvider, string>();
export function cloudOperation(provider: CloudProvider) {
	return cloudOperations.get(provider);
}
export async function saveCloud(
	provider: CloudProvider,
	key: string,
): Promise<void> {
	if (cloudOperations.has(provider)) throw new Error("请先确认上一次验证结果");
	const id = crypto.randomUUID();
	cloudOperations.set(provider, id);
	// Keep transport errors distinct from Core's definite rejection.
	const result = await invoke<CoreResponse>("core_request", {
		requestId: id,
		request: {
			type: "configure_cloud",
			provider,
			api_key: key,
			model: provider === "llm" ? "deepseek-flash" : null,
		},
	});
	if (!["error", "cloud_status"].includes(result.type))
		throw new Error("验证结果待确认");
	cloudOperations.delete(provider);
	if (result.type === "error")
		throw new Error(`${result.code}: ${result.message}`);
}
export async function confirmCloud(
	provider: CloudProvider,
): Promise<"saved" | "pending" | "unknown"> {
	const request_id = cloudOperations.get(provider);
	if (!request_id) return "unknown";
	const result = await core<{
		type: string;
		id: string;
		pending: boolean;
		response: CoreResponse | null;
	}>({ type: "cloud_configuration_receipt", provider, request_id });
	if (result.response) {
		cloudOperations.delete(provider);
		if (result.response.type === "error")
			throw new Error(`${result.response.code}: ${result.response.message}`);
		return "saved";
	}
	if (result.pending) return "pending";
	cloudOperations.delete(provider);
	return "unknown";
}
export const modelLabels: Record<string, string> = {
	missing: "组件文件缺失",
	checking: "正在检查文件…",
	downloading: "正在准备…",
	verifying: "正在校验…",
	installing: "正在准备…",
	loading: "正在检查模型能否加载…",
	ready: "已就绪",
	failed: "准备失败",
};
export const modelErrors: Record<string, string> = {
	model_bundle_missing: "组件文件缺失，请重新安装完整的 Hear 应用",
	model_bundle_unreadable: "无法读取组件文件，请检查应用权限或重新安装 Hear",
	model_manifest: "组件版本不匹配，请重新安装完整的 Hear 应用",
	model_checksum: "组件文件损坏，请重新安装完整的 Hear 应用",
	model_resources_busy: "电脑资源繁忙，请稍后重试加载检查",
	model_worker_missing: "语音组件无法启动，请重新安装完整的 Hear 应用",
	model_load_timeout: "模型加载检查超时，请稍后重试",
	model_load_failed: "模型加载检查未通过，请重试；如仍失败，请重新安装 Hear",
};
