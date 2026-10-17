import { type CoreResponse, core, type Sources, type VoiceSample } from "./api";
import { type Chunk, ContinuousPlayback } from "./playback";

export function sampleParts(sample: VoiceSample) {
	return sample.parts?.length
		? sample.parts
		: [{ started_ms: sample.started_ms, ended_ms: sample.ended_ms }];
}

/** Reuse bounded, cleanup-aware playback, advancing only after a complete part.
 * Gaps between enrollment parts must never play as if they were sample audio. */
export class SamplePlayback {
	private player: ContinuousPlayback;
	private current: VoiceSample | null = null;
	private index = 0;
	private position = 0;
	private epoch = 0;
	constructor(
		private changed: (sampleId: string | null, elapsed: number) => void,
		private error: (cause: unknown) => void,
	) {
		this.player = new ContinuousPlayback(
			async (from, to, source) => {
				const chunk = await core<CoreResponse & Chunk>({
					type: "playback_chunk",
					from_ms: from,
					to_ms: to,
					sources: source as Sources,
				});
				if (!chunk.has_audio) throw new Error("这份声纹的原音已不可用");
				return chunk;
			},
			(at) => {
				this.position = at;
				if (!this.current) return;
				const parts = sampleParts(this.current);
				const elapsed =
					parts
						.slice(0, this.index)
						.reduce((sum, part) => sum + part.ended_ms - part.started_ms, 0) +
					at -
					parts[this.index].started_ms;
				this.changed(this.current.id, elapsed);
			},
			() => {
				const sample = this.current;
				if (!sample) return;
				const parts = sampleParts(sample);
				if (
					this.position < parts[this.index].ended_ms ||
					this.index + 1 >= parts.length
				) {
					this.stop();
					return;
				}
				const epoch = this.epoch;
				// An error callback follows done on failure; it invalidates this advance.
				queueMicrotask(() => {
					if (epoch === this.epoch && this.current) {
						this.index++;
						this.playPart();
					}
				});
			},
			(cause) => {
				this.stop();
				this.error(cause);
			},
			undefined,
			async (from, to, source, generation) => {
				const response = await core<CoreResponse & { valid: boolean }>({
					type: "playback_valid",
					from_ms: from,
					to_ms: to,
					sources: source as Sources,
					generation,
				});
				return response.valid;
			},
		);
	}
	start(sample: VoiceSample) {
		this.stop();
		this.current = sample;
		this.index = 0;
		this.changed(sample.id, 0);
		this.playPart();
	}
	private playPart() {
		if (!this.current) return;
		const part = sampleParts(this.current)[this.index];
		this.position = part.started_ms;
		this.player.start(part.started_ms, part.ended_ms, this.current.source);
	}
	stop() {
		this.epoch++;
		this.current = null;
		this.player.stop();
		this.changed(null, 0);
	}
	dispose() {
		this.current = null;
		this.epoch++;
		this.player.dispose();
	}
}
