import { invoke, isTauri } from "@tauri-apps/api/core";

export type Source = "microphone" | "system";
export type Sources = Source | "both";
export type CloudProvider = "llm" | "asr";

export function processingLabel(value: string): string {
	switch (value) {
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
