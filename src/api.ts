import { invoke, isTauri } from "@tauri-apps/api/core";

export type Source = "microphone" | "system";
export type Sources = Source | "both";
export type CloudProvider = "llm" | "asr";

const errorLabels: Record<string, string> = {
	range_no_audio: "所选时间没有可精转的原声",
	range_invalid: "请选择有效的精转时间和声音来源",
	invalid_time_range: "请选择有效的时间范围",
	revision_conflict: "内容已更新，请重新确认后再试",
	setup_incomplete: "请先完成云端服务设置",
	model_not_ready: "本地语音模型尚未准备好",
	storage_limit: "录音空间已满，请在设置中清理或调整上限",
	source_disabled: "这个声音来源尚未开启",
	source_busy: "声音来源正在切换，请稍后再试",
	playback_unavailable: "原声读取失败，请检查素材或稍后重试",
	event_detail_unavailable: "事件已变化，请返回时间线重新选择",
	range_detail_unavailable: "这段素材已变化，请重新选择",
	cleanup_preview_invalid: "清理预览已过期，请重新查看影响范围",
	cleanup_invalid: "清理范围已变化，请重新确认",
	cleanup_failed: "清理未完成，请稍后重试",
	export_failed: "导出未完成，请检查目标位置后重试",
	screenshot_unavailable: "暂时无法读取这张截图",
	screenshot_failed: "截屏未完成，请重试",
	protocol_version: "应用组件版本不一致，请重新打开 hear",
	provider_transport: "无法连接云端服务，请检查网络后重试",
	provider_verification_failed: "密钥验证未通过，请检查后重试",
	provider_verification_timeout: "云端验证超时，请稍后重试",
};

export function isEventStale(error: unknown): boolean {
	const message = error instanceof Error ? error.message : String(error);
	return (
		message.startsWith("event_invalid:") &&
		(message.includes("event revision conflict") ||
			message.includes("event not found"))
	);
}

export function userError(error: unknown): string {
	const message = error instanceof Error ? error.message : String(error);
	const code = /^([a-z_0-9]+):/.exec(message)?.[1];
	if (code === "provider_http_401") return "API Key 无效或已失效";
	if (code === "provider_http_403") return "API Key 无权使用这项服务";
	if (code === "provider_http_429") return "云端服务请求过多，请稍后重试";
	if (code?.startsWith("provider_http_"))
		return "云端服务暂时无法验证，请稍后重试";
	if (message.startsWith("keychain_save_"))
		return "无法将密钥保存到系统钥匙串，请稍后重试";
	if (code === "event_invalid") {
		if (isEventStale(error)) {
			return "事件已更新，请在时间线查看最新内容后再试";
		}
		return message.includes("overlaps another named event")
			? "时间与已有事件重叠，请调整边界或先编辑已有事件"
			: "事件操作未完成，请检查时间和声音来源";
	}
	return (code && errorLabels[code]) || message.replace(/^Error:\s*/, "");
}

export function processingLabel(value: string): string {
	switch (value) {
		case "raw_audio":
			return "原声已保留";
		case "moment_only":
			return "时刻记录";
		case "unorganized":
			return "未整理";
		case "fitting":
			return "整理中";
		case "pending_fit":
			return "待整理";
		case "fit_failed":
			return "整理失败";
		case "pending_asr":
			return "待精转";
		case "transcribing":
			return "精转中";
		case "asr_failed":
			return "精转失败";
		case "transcribed":
			return "精转完成";
		case "partial_transcript":
			return "部分精转";
		case "partial_audio":
			return "音频不完整";
		case "audio_unavailable":
			return "无可用原声";
		case "no_transcript":
			return "无可识别文字";
		default:
			return "待处理";
	}
}

export function manualAsrLabel(value: string): string {
	switch (value) {
		case "pending":
			return "待精转";
		case "processing":
		case "submitted":
			return "精转中";
		case "complete":
			return "精转完成";
		case "failed":
			return "精转失败";
		case "no_transcript":
			return "无可识别文字";
		default:
			return "精转待处理";
	}
}

export interface ServiceState {
	revision: number;
	enabled_intent: boolean;
	paused_intent: boolean;
	storage_paused: boolean;
	audio_used_bytes: number;
	audio_limit_bytes: number;
	disk_free_bytes: number;
	storage_warning: boolean;
	microphone_enabled: boolean;
	system_enabled: boolean;
	microphone_recording: boolean;
	system_recording: boolean;
	microphone_phase:
		| "disabled"
		| "idle"
		| "starting"
		| "recording"
		| "stopping"
		| "unavailable";
	system_phase:
		| "disabled"
		| "idle"
		| "starting"
		| "recording"
		| "stopping"
		| "unavailable";
	capture_blocker:
		| "none"
		| "sources_disabled"
		| "model_not_ready"
		| "setup_incomplete"
		| "query_only"
		| "session_stopped"
		| "maintenance"
		| "storage_limit"
		| "source_unavailable";
	microphone_reason: string | null;
	system_reason: string | null;
	capture_state:
		| "not_ready"
		| "stopped"
		| "starting"
		| "stopping"
		| "paused"
		| "recording";
	reason: string | null;
}

export interface CoarseSegment {
	id: string;
	source: Source;
	started_ms: number;
	ended_ms: number;
	text: string;
}

export interface PreciseSegment extends CoarseSegment {
	speaker: string | null;
	original_text: string | null;
	corrected: boolean;
	person_id: string | null;
	person_name: string | null;
	person_origin: string | null;
	revision: number;
	speaker_revision: number;
}

export interface TimelineItem {
	id: string | null;
	manual_range_id: string | null;
	started_ms: number;
	ended_ms: number;
	title: string | null;
	sources: Sources;
	revision: number | null;
	fit_state: string | null;
	processing_state: string;
	manual_asr_state: string | null;
	manual_asr_started_ms: number | null;
	manual_asr_ended_ms: number | null;
	manual_asr_source: Source | null;
	recording_sources?: Source[];
	preview?: { text: string; source: Source; is_precise: boolean } | null;
	coarse_segments: CoarseSegment[];
	precise_segments: PreciseSegment[];
}

export function manualAsrSummary(item: TimelineItem): string | null {
	if (!item.manual_asr_state) return null;
	if (
		item.manual_asr_started_ms == null ||
		item.manual_asr_ended_ms == null ||
		item.manual_asr_source == null
	) {
		return manualAsrLabel(item.manual_asr_state);
	}
	const source = item.manual_asr_source === "system" ? "电脑声音" : "麦克风";
	return `${source} ${time(item.manual_asr_started_ms)}—${time(item.manual_asr_ended_ms)} ${manualAsrLabel(item.manual_asr_state)}`;
}

export interface TimeGap {
	source: Source;
	started_ms: number;
	ended_ms: number;
	reason: string;
}

export interface Person {
	id: string;
	display_name: string;
	sample_count: number;
	created_ms: number;
	revision: number;
}

export interface VoiceSample {
	id: string;
	person_id: string;
	source: Source;
	started_ms: number;
	ended_ms: number;
	model_version: string;
	created_ms: number;
	revision: number;
}

export interface ReviewItem {
	id: string;
	kind: "text" | "speaker";
	original_segment_id: string;
	source: Source;
	started_ms: number;
	ended_ms: number;
	text: string | null;
	person_id: string | null;
	person_name: string | null;
	revision: number;
}

export interface CloudQueue {
	provider: CloudProvider;
	configured: boolean;
	paused: boolean;
	pending: number;
	processing: number;
	failed: number;
}

export interface CloudTask {
	id: string;
	provider: CloudProvider;
	target_kind: string;
	target_id: string;
	source: Source | null;
	started_ms: number;
	ended_ms: number;
	state: string;
	error_code: string | null;
	manual: boolean;
}

export interface CleanupPreview {
	token: string;
	mode: "raw_audio" | "all_data";
	from_ms: number;
	to_ms: number;
	sources: Sources;
	audio_blocks: number;
	audio_bytes: number;
	coarse_segments: number;
	precise_segments: number;
	voice_samples: number;
	asr_tasks_awaiting_upload: number;
	voice_extractions_awaiting_audio: number;
	affected_event_count: number;
	affected_events: string[];
	moment_count: number;
	screenshot_count: number;
	overlap_may_remove_whole_sentences: boolean;
	voice_features_retained: boolean;
}

export interface Moment {
	id: string;
	kind: "mark" | "comment" | "screenshot";
	anchor_ms: number;
	created_ms: number;
	captured_ms: number | null;
	text: string | null;
	ocr_text: string | null;
	ocr_state: "complete" | "failed" | null;
}

export type CoreRequest = { type: string; [key: string]: unknown };
export type CoreResponse = { type: string; id: string; [key: string]: unknown };

export async function core<T extends CoreResponse = CoreResponse>(
	request: CoreRequest,
): Promise<T> {
	if (!isTauri()) throw new Error("请在 hear 桌面应用中查看记录");
	const response = await invoke<T>("core_request", { request });
	if (response.type === "error") {
		throw new Error(`${String(response.code)}: ${String(response.message)}`);
	}
	return response;
}

export async function executeCleanup(
	previewToken: string,
	allData: boolean,
): Promise<CoreResponse> {
	const response = await invoke<CoreResponse>("cleanup_request", {
		request: {
			type: "execute_cleanup",
			preview_token: previewToken,
			confirmed_all_data: allData,
		},
	});
	if (response.type === "error") {
		throw new Error(`${String(response.code)}: ${String(response.message)}`);
	}
	return response;
}

export function dayBounds(date: Date): [number, number] {
	const start = new Date(
		date.getFullYear(),
		date.getMonth(),
		date.getDate(),
	).getTime();
	const end = new Date(
		date.getFullYear(),
		date.getMonth(),
		date.getDate() + 1,
	).getTime();
	return [start, end];
}

export function addDays(date: Date, days: number): Date {
	return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

export function time(timestamp: number): string {
	return new Intl.DateTimeFormat("zh-CN", {
		hour: "2-digit",
		minute: "2-digit",
		hour12: false,
	}).format(timestamp);
}

export function dateTitle(date: Date): string {
	return `${date.getMonth() + 1}月${date.getDate()}日`;
}
