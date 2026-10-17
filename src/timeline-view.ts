import type { Moment, TimelineItem } from "./api";

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
