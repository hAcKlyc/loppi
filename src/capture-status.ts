import type { EntrySnapshot } from "./entry-api";
import { nextScheduleAction } from "./recording-schedule.ts";

export type CaptureStatusAction =
	| "pause"
	| "resume"
	| "start"
	| "settings"
	| "retry";
export type CaptureStatusView = {
	tone: "recording" | "paused" | "waiting" | "warning";
	label: string;
	subtitle: string;
	detail: string;
	action: CaptureStatusAction | null;
};

// The host already projects real capture into appearance/control. This only
// supplies readable main-window copy; it does not infer state from error text.
export function captureStatusView(
	snapshot: EntrySnapshot | null,
): CaptureStatusView {
	const state = snapshot?.state;
	if (!state || snapshot?.error) {
		return {
			tone: "waiting",
			label: snapshot?.error ? "状态待确认" : "正在连接",
			subtitle: "暂时无法确认记录状态",
			detail: "重新连接后确认当前状态，不会自动开始录音",
			action: "retry",
		};
	}
	const detail = snapshot.detail ?? "";
	const control = snapshot.control;
	const sources = [
		state.microphone_recording && "麦克风",
		state.system_recording && "电脑声音",
	]
		.filter(Boolean)
		.join(" · ");
	if (snapshot.appearance === "listening") {
		return {
			tone: "recording",
			label: "正在记录",
			subtitle: sources,
			detail,
			action: control,
		};
	}
	if (snapshot.appearance === "partial") {
		return {
			tone: "warning",
			label: "部分记录",
			subtitle: `${sources}仍在记录`,
			detail,
			action: control,
		};
	}
	if (snapshot.appearance === "interrupted") {
		return {
			tone: "warning",
			label: state.storage_paused ? "空间不足" : "记录中断",
			subtitle: state.storage_paused ? "清理空间后继续" : "检查权限或声音设备",
			detail,
			action: "settings",
		};
	}
	if (snapshot.appearance === "sleeping") {
		return {
			tone: "paused",
			label: state.paused_intent
				? state.recording_schedule?.paused_by_schedule
					? "定时暂停"
					: "已暂停"
				: "未开始",
			subtitle: state.paused_intent
				? state.recording_schedule?.paused_by_schedule
					? nextScheduleAction(state.recording_schedule)
					: "声音记录已暂停"
				: "准备好后，开始记录",
			detail,
			action: control,
		};
	}
	if (
		state.microphone_phase === "stopping" ||
		state.system_phase === "stopping"
	) {
		return {
			tone: "waiting",
			label: "正在暂停",
			subtitle: "等待声音记录停止",
			detail,
			action: null,
		};
	}
	if (state.capture_blocker === "maintenance") {
		return {
			tone: "waiting",
			label: "正在整理",
			subtitle: "处理完成后恢复记录",
			detail,
			action: null,
		};
	}
	if (["query_only", "session_stopped"].includes(state.capture_blocker)) {
		return {
			tone: "paused",
			label: "尚未开始",
			subtitle: "继续记录这一刻",
			detail,
			action: control,
		};
	}
	if (
		["model_not_ready", "setup_incomplete", "sources_disabled"].includes(
			state.capture_blocker,
		)
	) {
		return {
			tone: "waiting",
			label: "尚未就绪",
			subtitle: "在设置中完成准备",
			detail,
			action: "settings",
		};
	}
	return {
		tone: "waiting",
		label: "正在连接",
		subtitle: "等待声音来源就绪",
		detail,
		action: control,
	};
}
