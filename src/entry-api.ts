import { invoke } from "@tauri-apps/api/core";
import type { ServiceState } from "./api";
export type EntryMode = "top" | "floating";
export type RabbitState =
	| "listening"
	| "sleeping"
	| "waiting"
	| "partial"
	| "interrupted";
export interface EntrySnapshot {
	sequence: number;
	state: ServiceState | null;
	error: string | null;
	mode: EntryMode;
	appearance: RabbitState;
	detail: string | null;
	control: "pause" | "resume" | "start" | null;
	notice: { id: number; message: string } | null;
}
// Both WebViews show the host's projection; a failed observation stays awake.
export const entryAppearance = (snapshot: EntrySnapshot | null): RabbitState =>
	snapshot?.error ? "waiting" : (snapshot?.appearance ?? "waiting");
export interface EntryGeometry {
	width: number;
	height: number;
	band: number;
	rabbit_x: number;
	rabbit_y: number;
	top_band: number;
	top_shift: number;
}
export type EntryView = "collapsed" | "panel" | "comment";
export type EntryKind = "mark" | "comment" | "screenshot" | "control";
export interface EntryAction {
	id: string;
	kind: EntryKind;
	anchor_ms: number;
	text: string | null;
}
export interface EntryResult {
	outcome: "confirmed" | "rejected" | "uncertain";
	message: string | null;
	response: unknown;
}
export const getEntrySnapshot = (refresh = false) =>
	invoke<EntrySnapshot>("entry_snapshot", { refresh });

// One immutable payload per unresolved operation, retained only for this GUI session.
export class EntryRequests {
	private slots = new Map<EntryKind, { action: EntryAction; busy: boolean }>();
	isBusy(kind: EntryKind) {
		return this.slots.get(kind)?.busy ?? false;
	}
	isUncertain(kind: EntryKind) {
		const slot = this.slots.get(kind);
		return !!slot && !slot.busy;
	}
	async send(
		kind: EntryKind,
		anchor: number,
		text: string | null,
	): Promise<EntryResult> {
		let slot = this.slots.get(kind);
		if (slot?.busy) throw new Error("操作正在处理，请稍候");
		if (!slot) {
			slot = {
				action: { id: crypto.randomUUID(), kind, anchor_ms: anchor, text },
				busy: false,
			};
			this.slots.set(kind, slot);
		}
		slot.busy = true;
		try {
			const result = await invoke<EntryResult>("entry_action", {
				action: slot.action,
			});
			if (result.outcome !== "uncertain") this.slots.delete(kind);
			return result;
		} finally {
			slot.busy = false;
		}
	}
}
export function commentCanSubmit(text: string, composing: boolean) {
	return (
		!composing &&
		text.trim().length > 0 &&
		Array.from(text.trim()).length <= 5000
	);
}

export function commentEnterSubmits(
	event: {
		key: string;
		shiftKey: boolean;
		keyCode: number;
		isComposing: boolean;
	},
	composing: boolean,
) {
	return (
		event.key === "Enter" &&
		!event.shiftKey &&
		!event.isComposing &&
		!composing &&
		event.keyCode !== 229
	);
}

// Position is a function of the native viewport, not a second animation clock.
// Both directions (and an interrupted reversal) therefore use the same path.
export function topRabbitMotion(
	topBand: number,
	topShift: number,
	feedback: boolean,
) {
	const compact = feedback ? 280 : 44;
	const along = (from: number, to: number) =>
		`calc(${from}px + ${(to - from) / (412 - compact)} * (100vw - ${compact}px))`;
	const startX = compact / 2 - 14;
	const startY = (Math.max(32, topBand) - 28) / 2;
	// Hold the screen x until the rabbit clears the physical notch. Compensate
	// for the native window's simultaneous translation, then join the endpoint.
	const clear = Math.min(
		0.9,
		(topBand - startY) / (topBand + 24 - startY) + 0.04,
	);
	const x =
		topBand > 0
			? `calc(${startX}px + ${topShift / (412 - compact)} * (100vw - ${compact}px) + ${(20 - startX - topShift) / ((1 - clear) * (412 - compact))} * max(0px, calc(100vw - ${compact + clear * (412 - compact)}px)))`
			: along(startX, 20);
	return {
		"--entry-compact-width": `${compact}px`,
		"--entry-rabbit-x": x,
		"--entry-rabbit-y": along(startY, topBand + 24),
		"--entry-rabbit-size": along(28, 72),
	};
}
