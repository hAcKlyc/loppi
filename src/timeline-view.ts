import type { Moment, TimelineItem } from "./api";

export function dayMapPoint(
	clientX: number,
	left: number,
	width: number,
	from: number,
	to: number,
) {
	if (
		![clientX, left, width, from, to].every(Number.isFinite) ||
		width <= 0 ||
		to <= from
	)
		return null;
	const x = Math.max(0, Math.min(width, clientX - left));
	const at = from + Math.round((x / width) * (to - from));
	const date = new Date(at);
	return {
		x,
		at,
		label:
			x === width
				? "24:00"
				: `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`,
	};
}

/** Content spans are navigation ranges, not proof of continuous audio coverage. */
export function daySpan(
	start: number,
	end: number,
	from: number,
	to: number,
	now: number,
) {
	const left = Math.max(start, from);
	const right = Math.min(end, to, now);
	if (
		![start, end, from, to, now].every(Number.isFinite) ||
		to <= from ||
		right <= left
	)
		return null;
	return {
		left: `${((left - from) / (to - from)) * 100}%`,
		width: `${((right - left) / (to - from)) * 100}%`,
	};
}

export function cardExcerpt(
	item: TimelineItem,
	moments: Moment[],
	hideCoarse: boolean,
) {
	if (item.id) {
		if (item.summary?.trim())
			return {
				text: item.summary.trim(),
				label: `内容摘要 · ${item.sources === "both" ? "双路声音" : item.sources === "system" ? "电脑声音" : "麦克风"}`,
			};
		return { text: "暂无内容摘要，打开查看转录", label: "" };
	}
	const showTranscript =
		!hideCoarse || item.id !== null || item.manual_range_id !== null;
	const preview = showTranscript ? item.preview : null;
	if (preview?.text.trim())
		return {
			text: preview.text,
			label: `${preview.is_precise ? "精转" : "粗转"} · ${preview.source === "microphone" ? "麦克风" : "电脑声音"}`,
		};
	const moment = moments.find((entry) =>
		(entry.text || entry.ocr_text)?.trim(),
	);
	if (moment) {
		const text = (moment.text || moment.ocr_text || "").trim();
		const chars = Array.from(text);
		return {
			text: chars.slice(0, 160).join("") + (chars.length > 160 ? "…" : ""),
			label: moment.kind === "screenshot" ? "截图文字" : "现场笔记",
		};
	}
	return {
		text:
			moments.length > 0
				? "已保存现场标记，打开查看详情"
				: "暂无文字摘录，打开查看详情",
		label: "",
	};
}

export function monthBoundaries(date: Date): number[] {
	const year = date.getFullYear(),
		month = date.getMonth();
	const count = new Date(year, month + 1, 0).getDate();
	return Array.from({ length: count + 1 }, (_, day) =>
		new Date(year, month, day + 1).getTime(),
	);
}

export function liveCard(
	items: TimelineItem[],
	state: import("./api").ServiceState | null,
	date: Date,
	now: number,
): TimelineItem | null {
	if (
		state?.capture_state !== "recording" ||
		state.paused_intent ||
		state.storage_paused ||
		date.toDateString() !== new Date(now).toDateString()
	)
		return null;
	const newest = [...items].sort((a, b) => b.started_ms - a.started_ms)[0];
	return newest?.recording_sources?.some(
		(source) =>
			state[`${source}_phase`] === "recording" && state[`${source}_recording`],
	)
		? newest
		: null;
}
