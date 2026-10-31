import type { CoarseSegment, PreciseSegment, TimelineItem } from "./api";

export type TranscriptMore = { coarse: boolean; precise: boolean };

function coveredByPrecise(
	coarse: CoarseSegment,
	precise: PreciseSegment[],
): boolean {
	const intervals = precise
		.filter(
			(part) =>
				part.source === coarse.source &&
				part.ended_ms > coarse.started_ms &&
				part.started_ms < coarse.ended_ms,
		)
		.sort((a, b) => a.started_ms - b.started_ms);
	let cursor = coarse.started_ms;
	let covered = 0;
	for (const part of intervals) {
		const start = Math.max(coarse.started_ms, part.started_ms, cursor);
		const end = Math.min(coarse.ended_ms, part.ended_ms);
		covered += Math.max(0, end - start);
		cursor = Math.max(cursor, end);
	}
	return covered >= (coarse.ended_ms - coarse.started_ms) / 2;
}

// Pages are sorted by start time. The last start is only a lower bound on
// unseen rows (including ties), never proof that the rest of that audio is empty.
export function transcriptPageEnd(
	detail: TimelineItem,
	more: TranscriptMore,
): number {
	const precise = detail.precise_segments;
	const preciseThrough = more.precise
		? (precise[precise.length - 1]?.started_ms ?? -Infinity)
		: Infinity;
	if (detail.processing_state === "transcribed" && precise.length > 0)
		return preciseThrough;
	const coarse = detail.coarse_segments;
	let through = Math.min(
		preciseThrough,
		more.coarse
			? (coarse[coarse.length - 1]?.started_ms ?? -Infinity)
			: Infinity,
	);
	// Hold an unresolved boundary sentence and the following text together.
	for (const row of coarse) {
		if (row.ended_ms > preciseThrough && !coveredByPrecise(row, precise))
			through = Math.min(through, row.started_ms);
	}
	return through;
}

export function bestSegments(
	detail: TimelineItem,
	more: TranscriptMore,
): (CoarseSegment | PreciseSegment)[] {
	const precise = detail.precise_segments;
	if (detail.processing_state === "transcribed" && precise.length > 0)
		return [...precise].sort((a, b) => a.started_ms - b.started_ms);
	if (precise.length === 0 && !more.precise)
		return [...detail.coarse_segments].sort(
			(a, b) => a.started_ms - b.started_ms,
		);
	const through = transcriptPageEnd(detail, more);
	return [
		...precise,
		...detail.coarse_segments.filter((row) => !coveredByPrecise(row, precise)),
	]
		.filter((row) => row.started_ms < through)
		.sort((a, b) => a.started_ms - b.started_ms);
}
