import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type PlaybackBuffer = {
	duration: number;
	sampleRate: number;
	numberOfChannels: number;
	buffer?: number;
};
export interface PlaybackNode {
	buffer: PlaybackBuffer | null;
	onended: ((success?: boolean, cancelled?: boolean) => void) | null;
	connect(destination: unknown): void;
	disconnect(): void;
	start(when: number): Promise<number> | number;
	stop(): void;
}
export interface PlaybackAudio {
	readonly currentTime: number;
	readonly destination: unknown;
	resume(): Promise<void>;
	suspend(): Promise<void>;
	close(): Promise<void>;
	decodeWav(wav: string): Promise<PlaybackBuffer>;
	createBufferSource(): PlaybackNode;
}
// One ordered renderer command stream. Stale contexts/epochs are independently
// rejected by the host; a late stop cannot stop a newer player's output.
let commands: Promise<unknown> = Promise.resolve();
function output<T>(command: Record<string, unknown>): Promise<T> {
	const result = commands.then(() => invoke<T>("playback_output", { command }));
	commands = result.catch(() => {});
	return result;
}
export class NativeAudio implements PlaybackAudio {
	readonly destination = {};
	private opening: Promise<number> | null = null;
	private epoch = 0;
	private disposed = false;
	private clock = 0;
	private observed = performance.now();
	private running = false;
	private nodes = new Map<number, NativeNode>();
	private subscription: Promise<() => void>;
	constructor() {
		this.subscription = listen<{
			context: number;
			epoch: number;
			buffer: number;
			status: number;
		}>("hear-playback-ended", async ({ payload }) => {
			if (this.disposed || payload.epoch !== this.epoch) return;
			if (
				payload.context !== (await this.opening) ||
				payload.epoch !== this.epoch ||
				this.disposed
			)
				return;
			const node = this.nodes.get(payload.buffer);
			this.nodes.delete(payload.buffer);
			if (node) {
				node.finished(payload.status);
			}
		});
	}
	get currentTime() {
		return (
			this.clock +
			(this.running ? (performance.now() - this.observed) / 1000 : 0)
		);
	}
	private observe(clock: number) {
		this.clock = clock;
		this.observed = performance.now();
	}
	async resume() {
		if (this.disposed) throw new Error("playback_stale");
		const epoch = ++this.epoch;
		await this.subscription;
		if (epoch !== this.epoch || this.disposed) return;
		this.opening ??= output<{ context: number }>({ type: "open" }).then(
			(r) => r.context,
		);
		const context = await this.opening;
		if (epoch !== this.epoch || this.disposed) return;
		const result = await output<{ clock: number }>({
			type: "resume",
			context,
			epoch,
		});
		if (epoch === this.epoch && !this.disposed) {
			this.nodes.clear();
			this.observe(result.clock);
			this.running = true;
		}
	}
	async suspend() {
		const epoch = ++this.epoch;
		this.clock = this.currentTime;
		this.running = false;
		this.nodes.clear();
		if (this.opening)
			await output({ type: "suspend", context: await this.opening, epoch });
	}
	async close() {
		this.disposed = true;
		const epoch = ++this.epoch;
		this.running = false;
		this.nodes.clear();
		(await this.subscription)();
		if (this.opening)
			await output({ type: "close", context: await this.opening, epoch });
	}
	async decodeWav(wav: string): Promise<PlaybackBuffer> {
		const epoch = this.epoch;
		const result = await output<PlaybackBuffer & { clock: number }>({
			type: "load",
			context: await this.opening,
			epoch,
			wav,
		});
		if (epoch !== this.epoch || this.disposed)
			throw new Error("playback_stale");
		this.observe(result.clock);
		return result;
	}
	createBufferSource() {
		return new NativeNode(this);
	}
	async schedule(node: NativeNode, when: number) {
		const buffer = node.buffer?.buffer;
		if (buffer === undefined) throw new Error("playback_invalid_audio");
		const epoch = this.epoch;
		this.nodes.set(buffer, node);
		const result = await output<{ when: number }>({
			type: "schedule",
			context: await this.opening,
			epoch,
			buffer,
			when,
		});
		return result.when;
	}
	remove(buffer: number) {
		this.nodes.delete(buffer);
		const epoch = this.epoch;
		void this.opening
			?.then((context) => output({ type: "remove", context, epoch, buffer }))
			.catch(() => {});
	}
}
class NativeNode implements PlaybackNode {
	onended: ((success?: boolean, cancelled?: boolean) => void) | null = null;
	private value: PlaybackBuffer | null = null;
	private context: NativeAudio;
	constructor(context: NativeAudio) {
		this.context = context;
	}
	get buffer() {
		return this.value;
	}
	set buffer(value: PlaybackBuffer | null) {
		if (this.value?.buffer !== undefined && this.value !== value)
			this.context.remove(this.value.buffer);
		this.value = value;
	}
	connect(_destination: unknown) {}
	disconnect() {}
	start(when: number) {
		return this.context.schedule(this, when);
	}
	stop() {
		if (this.value?.buffer !== undefined)
			this.context.remove(this.value.buffer);
	}
	finished(status: number) {
		this.onended?.(status === 1, status === -1);
	}
}
