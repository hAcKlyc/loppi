export type Chunk = {
	wav_base64: string;
	has_audio: boolean;
	next_audio_ms: number | null;
};
type Scheduled = {
	node: AudioBufferSourceNode;
	at: number;
	until: number;
	when: number;
};

/** A single request in flight and at most two five-second decoded buffers.
 * Playback uses the audio clock; UI timers never determine a sample boundary. */
export class ContinuousPlayback {
	private context: AudioContext | null = null;
	private nodes: Scheduled[] = [];
	private epoch = 0;
	private active = false;
	private disposed = false;
	private next = 0;
	private end = 0;
	private source = "both";
	private emptyTail = false;
	private pendingError: { cause: unknown } | null = null;
	private loading: Promise<void> | null = null;
	private timer: ReturnType<typeof setInterval> | null = null;
	private fetchChunk: (
		from: number,
		to: number,
		source: string,
	) => Promise<Chunk>;
	private position: (at: number) => void;
	private done: () => void;
	private error: (cause: unknown) => void;
	private createContext: () => AudioContext;

	constructor(
		fetchChunk: (from: number, to: number, source: string) => Promise<Chunk>,
		position: (at: number) => void,
		done: () => void,
		error: (cause: unknown) => void,
		createContext = () => new AudioContext({ sampleRate: 48_000 }),
	) {
		this.fetchChunk = fetchChunk;
		this.position = position;
		this.done = done;
		this.error = error;
		this.createContext = createContext;
	}
	start(from: number, end: number, source: string) {
		this.stop();
		from = Math.round(from);
		end = Math.round(end);
		if (
			this.disposed ||
			!Number.isSafeInteger(from) ||
			!Number.isSafeInteger(end) ||
			end <= from
		)
			return;
		this.context ??= this.createContext();
		this.active = true;
		this.next = from;
		this.end = end;
		this.source = source;
		this.emptyTail = false;
		const epoch = this.epoch;
		void this.context
			.resume()
			.then(() => {
				if (this.active && epoch === this.epoch) this.pump();
			})
			.catch((cause) => this.fail(epoch, cause));
		this.timer = setInterval(() => this.tick(), 100);
	}
	stop() {
		this.tick();
		this.active = false;
		this.pendingError = null;
		this.epoch++;
		for (const { node } of this.nodes) {
			node.onended = null;
			node.stop();
			node.disconnect();
			node.buffer = null;
		}
		this.nodes = [];
		if (this.timer !== null) clearInterval(this.timer);
		this.timer = null;
		void this.context?.suspend().catch(() => {});
	}
	dispose() {
		this.stop();
		this.disposed = true;
		void this.context?.close().catch(() => {});
		this.context = null;
	}
	private tick() {
		const time = this.context?.currentTime ?? 0;
		const current = this.nodes.find(
			(item) =>
				item.when <= time && time < item.when + (item.until - item.at) / 1000,
		);
		if (this.active && current)
			this.position(
				Math.min(
					current.until,
					Math.round(current.at + (time - current.when) * 1000),
				),
			);
	}
	private fail(epoch: number, cause: unknown) {
		if (epoch !== this.epoch || !this.active) return;
		this.stop();
		this.done();
		this.error(cause);
	}
	private pump() {
		if (!this.active || this.loading || !this.context) return;
		const epoch = this.epoch;
		const context = this.context;
		this.loading = (async () => {
			while (
				this.active &&
				epoch === this.epoch &&
				this.nodes.length < 2 &&
				this.next < this.end
			) {
				const at = this.next;
				const until = Math.min(at + 5000, this.end);
				const response = await this.fetchChunk(at, until, this.source);
				if (!this.active || epoch !== this.epoch) return;
				if (!response.has_audio) {
					const next = response.next_audio_ms;
					// Preserve the existing player behavior: skip wholly unavailable
					// chunks, but retain real gaps inside an available chunk as silence.
					this.next =
						next !== null && next >= until && next < this.end ? next : this.end;
					this.emptyTail = this.next === this.end;
					continue;
				}
				if (response.wav_base64.length > 1_500_000)
					throw new Error("playback_unavailable: oversized chunk");
				const binary = atob(response.wav_base64);
				const bytes = Uint8Array.from(binary, (value) => value.charCodeAt(0));
				const buffer = await context.decodeAudioData(bytes.buffer);
				if (!this.active || epoch !== this.epoch) return;
				if (
					buffer.numberOfChannels !== 1 ||
					Math.abs(buffer.duration - (until - at) / 1000) >
						1 / buffer.sampleRate
				) {
					throw new Error("playback_unavailable: invalid audio duration");
				}
				const prior = this.nodes[this.nodes.length - 1];
				const when = Math.max(
					context.currentTime + 0.015,
					prior ? prior.when + (prior.until - prior.at) / 1000 : 0,
				);
				const node = context.createBufferSource();
				node.buffer = buffer;
				node.connect(context.destination);
				const scheduled = { node, at, until, when };
				this.nodes.push(scheduled);
				this.next = until;
				node.onended = () => {
					if (epoch !== this.epoch || !this.active) return;
					this.nodes = this.nodes.filter((item) => item !== scheduled);
					node.disconnect();
					node.buffer = null;
					this.position(until);
					this.pump();
				};
				node.start(when);
			}
		})()
			.catch((cause) => {
				if (epoch !== this.epoch || !this.active) return;
				if (this.nodes.length === 0) this.fail(epoch, cause);
				else {
					// A failed future chunk must not discard audio already scheduled.
					// Report at its boundary; seek/stop still cancels immediately.
					this.pendingError = { cause };
					this.next = this.end;
				}
			})
			.finally(() => {
				this.loading = null;
				if (!this.active) return;
				if (this.next >= this.end && this.nodes.length === 0) {
					const pending = this.pendingError;
					this.stop();
					this.done();
					if (pending) this.error(pending.cause);
					else if (this.emptyTail)
						this.error(new Error("这段时间没有可播放的声音"));
				} else if (this.nodes.length < 2 && this.next < this.end) this.pump();
			});
	}
}
