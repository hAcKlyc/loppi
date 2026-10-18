type Timed = { started_ms: number; ended_ms: number; source?: string };

export function entryTime(item: Timed, at?: number): number {
	return Math.max(
		item.started_ms,
		Math.min(
			at !== undefined && Number.isFinite(at) ? at : item.started_ms,
			item.ended_ms - 1,
		),
	);
}

// Both transcript streams are independently paginated in ascending start order.
export function pageCoversTime(
	segments: Timed[],
	at: number,
	limit: number,
): boolean {
	return (
		segments.length < limit || segments[segments.length - 1].started_ms > at
	);
}

export function nearestSegmentIndex(
	segments: Timed[],
	at: number,
	source = "both",
): number {
	let best = -1;
	let distance = Infinity;
	for (let index = 0; index < segments.length; index++) {
		const segment = segments[index];
		if (source !== "both" && segment.source !== source) continue;
		if (at >= segment.started_ms && at < segment.ended_ms) return index;
		const next = Math.max(segment.started_ms - at, at - segment.ended_ms, 0);
		if (next < distance) {
			best = index;
			distance = next;
		}
	}
	return best;
}
