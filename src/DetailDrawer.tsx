import { useCallback, useEffect, useRef, useState } from "react";
import {
	type CoarseSegment,
	type CoreResponse,
	core,
	type LocalSpeaker,
	type Moment,
	manualAsrSummary,
	type Person,
	type PreciseSegment,
	processingLabel,
	type Sources,
	type TimelineItem,
	time,
	userError,
} from "./api";

import { type Chunk, ContinuousPlayback } from "./playback";
import { type SpeakerAction, SpeakerPicker } from "./SpeakerPicker";

type Props = {
	item: TimelineItem;
	people: Person[];
	onClose: () => void;
	onEdit: (item: TimelineItem) => void;
	onTools: (item: TimelineItem) => void;
	onQueue: () => void;
	onDataAction: (kind: "export" | "cleanup", item: TimelineItem) => void;
	onChanged: (manualRangeId?: string) => void;
	onMessage: (message: string) => void;
	refreshToken: number;
};

function sameLocalDay(first: number, second: number): boolean {
	const a = new Date(first);
	const b = new Date(second);
	return (
		a.getFullYear() === b.getFullYear() &&
		a.getMonth() === b.getMonth() &&
		a.getDate() === b.getDate()
	);
}

function wallClock(
	timestamp: number,
	seconds: boolean,
	showDate: boolean,
): string {
	const value = new Date(timestamp);
	const clock = seconds
		? `${time(timestamp)}:${String(value.getSeconds()).padStart(2, "0")}`
		: time(timestamp);
	return showDate
		? `${value.getMonth() + 1}/${value.getDate()} ${clock}`
		: clock;
}

function uniqueSegments<T extends { id: string }>(segments: T[]): T[] {
	return [
		...new Map(segments.map((segment) => [segment.id, segment])).values(),
	];
}

function localDateTime(timestamp: number): string {
	const value = new Date(timestamp);
	return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}T${String(value.getHours()).padStart(2, "0")}:${String(value.getMinutes()).padStart(2, "0")}:${String(value.getSeconds()).padStart(2, "0")}`;
}

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

function bestSegments(
	detail: TimelineItem,
): (CoarseSegment | PreciseSegment)[] {
	if (
		detail.processing_state === "transcribed" &&
		detail.precise_segments.length > 0
	) {
		return [...detail.precise_segments].sort(
			(a, b) => a.started_ms - b.started_ms,
		);
	}
	return [
		...detail.precise_segments,
		...detail.coarse_segments.filter(
			(coarse) => !coveredByPrecise(coarse, detail.precise_segments),
		),
	].sort((a, b) => a.started_ms - b.started_ms);
}

function speakerName(
	segment: PreciseSegment,
	labels: Map<string, string>,
): string {
	return segment.person_name || labels.get(segment.speaker ?? "") || "未确定";
}

export function DetailDrawer({
	item,
	people,
	onClose,
	onEdit,
	onTools,
	onQueue,
	onDataAction,
	onChanged,
	onMessage,
	refreshToken,
}: Props) {
	const [detail, setDetail] = useState<TimelineItem>(item);
	const [localSpeakers, setLocalSpeakers] = useState<LocalSpeaker[]>([]);
	const [detailReady, setDetailReady] = useState(false);
	const [tab, setTab] = useState<"transcript" | "info">("transcript");
	const [offset, setOffset] = useState(0);
	const [hasMore, setHasMore] = useState(false);
	const [editing, setEditing] = useState<string | null>(null);
	const [draft, setDraft] = useState("");
	const [assigning, setAssigning] = useState<string | null>(null);
	const speakerAnchor = useRef<HTMLButtonElement | null>(null);
	const [busy, setBusy] = useState(false);
	const [infoError, setInfoError] = useState<string | null>(null);
	const [moments, setMoments] = useState<Moment[]>([]);
	const [transcriptSource, setTranscriptSource] = useState<"best" | "coarse">(
		"best",
	);
	const [note, setNote] = useState("");
	const [anchor, setAnchor] = useState(item.started_ms);
	const [playhead, setPlayhead] = useState(item.started_ms);
	const [playing, setPlaying] = useState(false);
	const [playSource, setPlaySource] = useState<Sources>(item.sources);
	const [shownImage, setShownImage] = useState<string | null>(null);
	const [imageBusy, setImageBusy] = useState(false);
	const [asrFrom, setAsrFrom] = useState(
		localDateTime(Math.ceil(item.started_ms / 1000) * 1000),
	);
	const [asrTo, setAsrTo] = useState(
		localDateTime(Math.floor(item.ended_ms / 1000) * 1000),
	);
	const [asrSources, setAsrSources] = useState<Sources>(item.sources);
	const player = useRef<ContinuousPlayback | null>(null);
	const messageRef = useRef(onMessage);
	messageRef.current = onMessage;
	const sourceRef = useRef<Sources>(item.sources);
	const momentsRequest = useRef(0);
	const detailRequest = useRef(0);
	const asrDefaults = useRef({
		from: localDateTime(Math.ceil(item.started_ms / 1000) * 1000),
		to: localDateTime(Math.floor(item.ended_ms / 1000) * 1000),
		sources: item.sources,
	});

	const load = useCallback(
		async (page: number) => {
			const token = ++detailRequest.current;
			try {
				const allCoarse: CoarseSegment[] = [];
				const allPrecise: PreciseSegment[] = [];
				let response: {
					type: "event";
					id: string;
					event: TimelineItem;
				} | null = null;
				for (let attempt = 0; attempt < 2; attempt++) {
					allCoarse.length = 0;
					allPrecise.length = 0;
					for (let offset = 0; offset <= page; offset += 100) {
						const request = item.id
							? {
									type: "event_detail",
									event_id: item.id,
									offset,
									limit: 100,
								}
							: {
									type: "range_detail",
									started_ms: item.started_ms,
									ended_ms: item.ended_ms,
									sources: item.sources,
									offset,
									limit: 100,
								};
						response = await core<{
							type: "event";
							id: string;
							event: TimelineItem;
						}>(request);
						if (token !== detailRequest.current) return;
						allCoarse.push(...response.event.coarse_segments);
						allPrecise.push(...response.event.precise_segments);
					}
					if (
						uniqueSegments(allCoarse).length === allCoarse.length &&
						uniqueSegments(allPrecise).length === allPrecise.length
					)
						break;
				}
				if (!response) return;
				const speakers = allPrecise[0]
					? await core<{
							type: "session_speakers";
							id: string;
							speakers: LocalSpeaker[];
						}>({
							type: "session_speakers",
							segment_id: allPrecise[0].id,
						})
					: { speakers: [] };
				if (token !== detailRequest.current) return;
				setLocalSpeakers(speakers.speakers);
				setPlayhead((current) =>
					Math.max(
						response.event.started_ms,
						Math.min(current, response.event.ended_ms),
					),
				);
				setAnchor((current) =>
					Math.max(
						response.event.started_ms,
						Math.min(current, response.event.ended_ms - 1),
					),
				);
				setDetail({
					...response.event,
					coarse_segments: uniqueSegments(allCoarse),
					precise_segments: uniqueSegments(allPrecise),
				});
				setDetailReady(true);
				setHasMore(
					response.event.coarse_segments.length +
						response.event.precise_segments.length >=
						100,
				);
				setInfoError(null);
			} catch (error) {
				if (token === detailRequest.current) setInfoError(userError(error));
			}
		},
		[item.id, item.started_ms, item.ended_ms, item.sources],
	);

	useEffect(() => {
		if (refreshToken >= 0 && editing === null && assigning === null)
			void load(offset);
	}, [load, refreshToken, offset, editing, assigning]);
	useEffect(() => {
		const next = {
			from: localDateTime(Math.ceil(item.started_ms / 1000) * 1000),
			to: localDateTime(Math.floor(item.ended_ms / 1000) * 1000),
			sources: item.sources,
		};
		setAsrFrom((current) =>
			current === asrDefaults.current.from ? next.from : current,
		);
		setAsrTo((current) =>
			current === asrDefaults.current.to ? next.to : current,
		);
		setAsrSources((current) =>
			current === asrDefaults.current.sources ? next.sources : current,
		);
		asrDefaults.current = next;
	}, [item.started_ms, item.ended_ms, item.sources]);

	const loadMoments = useCallback(async () => {
		const request = ++momentsRequest.current;
		try {
			const all: Moment[] = [];
			while (true) {
				const response = await core<{
					type: "moments";
					id: string;
					moments: Moment[];
				}>({
					type: "moments",
					from_ms: detail.started_ms,
					to_ms: detail.ended_ms,
					offset: all.length,
					limit: 200,
				});
				all.push(...response.moments);
				if (response.moments.length < 200) break;
			}
			if (request === momentsRequest.current) setMoments(all);
		} catch (error) {
			if (request === momentsRequest.current) setInfoError(userError(error));
		}
	}, [detail.started_ms, detail.ended_ms]);

	useEffect(() => {
		if (detailReady && refreshToken >= 0) void loadMoments();
		return () => {
			momentsRequest.current += 1;
		};
	}, [detailReady, loadMoments, refreshToken]);

	useEffect(() => {
		const playback = new ContinuousPlayback(
			(from, to, source) =>
				core<CoreResponse & Chunk>({
					type: "playback_chunk",
					from_ms: from,
					to_ms: to,
					sources: source as Sources,
				}),
			setPlayhead,
			() => setPlaying(false),
			(cause) => messageRef.current(userError(cause)),
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
		player.current = playback;
		return () => {
			playback.dispose();
			player.current = null;
		};
	}, []);
	const stopAudio = () => {
		player.current?.stop();
		setPlaying(false);
	};
	useEffect(() => {
		player.current?.stop();
		setPlaying(false);
		setPlayhead((at) =>
			Math.max(detail.started_ms, Math.min(at, detail.ended_ms)),
		);
	}, [detail.started_ms, detail.ended_ms]);
	const playFrom = (at: number) => {
		at = Math.max(detail.started_ms, Math.min(at, detail.ended_ms - 1));
		stopAudio();
		setPlayhead(at);
		setAnchor(at);
		setPlaying(true);
		player.current?.start(at, detail.ended_ms, sourceRef.current);
	};

	const sendNote = async () => {
		if (!note.trim()) return;
		setBusy(true);
		try {
			await core({
				type: "add_moment",
				kind: "comment",
				anchor_ms: Math.max(
					detail.started_ms,
					Math.min(anchor, detail.ended_ms - 1),
				),
				text: note.trim(),
			});
			setNote("");
			await loadMoments();
			onChanged();
		} catch (error) {
			onMessage(userError(error));
		} finally {
			setBusy(false);
		}
	};

	const showScreenshot = async (id: string) => {
		setImageBusy(true);
		try {
			const response = await core<{
				type: "screenshot_data";
				id: string;
				png_base64: string;
			}>({
				type: "screenshot_data",
				moment_id: id,
			});
			setShownImage(`data:image/png;base64,${response.png_base64}`);
		} catch (error) {
			onMessage(userError(error));
		} finally {
			setImageBusy(false);
		}
	};

	const mutate = async (
		request: { type: string; [key: string]: unknown },
		success: string | ((response: CoreResponse) => string),
	) => {
		setBusy(true);
		try {
			const response = await core(request);
			await load(offset);
			onChanged(
				request.type === "transcribe_range" &&
					!detail.id &&
					typeof response.range_id === "string"
					? response.range_id
					: undefined,
			);
			onMessage(typeof success === "string" ? success : success(response));
			setEditing(null);
			setAssigning(null);
		} catch (error) {
			onMessage(userError(error));
		} finally {
			setBusy(false);
		}
	};

	const transcribe = async () => {
		const started_ms =
			asrFrom === localDateTime(Math.ceil(detail.started_ms / 1000) * 1000)
				? detail.started_ms
				: new Date(asrFrom).getTime();
		const ended_ms =
			asrTo === localDateTime(Math.floor(detail.ended_ms / 1000) * 1000)
				? detail.ended_ms
				: new Date(asrTo).getTime();
		const sources = asrSources;
		if (
			!Number.isFinite(started_ms) ||
			!Number.isFinite(ended_ms) ||
			started_ms < detail.started_ms ||
			ended_ms > detail.ended_ms ||
			ended_ms <= started_ms ||
			(detail.sources !== "both" && sources !== detail.sources)
		) {
			setInfoError("请选择这段素材内有效的声音时间范围和来源");
			setTab("info");
			return;
		}
		await mutate(
			{
				type: "transcribe_range",
				started_ms,
				ended_ms,
				sources,
			},
			(response) => {
				const tasks = response.tasks as { state: string }[] | undefined;
				if (response.type !== "transcription_queued" || !tasks?.length) {
					return "精转请求未返回任务，请检查处理队列";
				}
				if (tasks.every((task) => task.state === "no_transcript")) {
					return "这段声音未识别出文字";
				}
				if (tasks.every((task) => task.state === "complete")) {
					return "这段声音已完成精转";
				}
				if (
					tasks.every(
						(task) =>
							task.state === "complete" || task.state === "no_transcript",
					)
				) {
					return "精转已结束 · 部分声音未识别出文字";
				}
				return `精转任务已保存 · ${tasks.length} 段`;
			},
		);
	};

	const updateSpeaker = async (
		segment: PreciseSegment,
		action: SpeakerAction,
	) => {
		setBusy(true);
		try {
			let creationMessage: string | null = null;
			if (action.kind === "sample") {
				await core({
					type: "add_voice_sample",
					segment_id: segment.id,
					expected_revision: segment.revision,
				});
			} else {
				const scope = {
					segment_id: segment.id,
					entire_local_speaker: action.entire,
					expected_revision: action.entire
						? segment.speaker_revision
						: segment.revision,
				};
				if (action.kind === "unbind")
					await core({ type: "unbind_speaker", ...scope });
				else if (action.choice.kind === "local") {
					const groupId = action.choice.groupId;
					const target = localSpeakers.find(
						(s) => s.group_id === groupId && s.anonymous,
					);
					if (!target) throw new Error("说话人已变化，请重新选择");
					await core({
						type: "assign_local_speaker",
						target_segment_id: target.segment_id,
						target_revision: target.revision,
						...scope,
					});
				} else if (action.choice.kind === "create") {
					const response = await core<{
						type: "person_changed";
						id: string;
						sample_count: number;
						person_id: string;
					}>({
						type: "create_and_bind_speaker",
						display_name: action.choice.name,
						...scope,
					});
					creationMessage =
						response.sample_count > 0
							? "人物与声纹已保存"
							: "人物已保存 · 待补充声纹";
				} else
					await core({
						type: "bind_speaker",
						person_id: action.choice.personId,
						...scope,
					});
			}
			await load(offset);
			onChanged();
			onMessage(
				creationMessage ??
					(action.kind === "sample"
						? "已提交声纹筛选"
						: action.kind === "unbind"
							? "已取消人物归属"
							: "说话人已更新"),
			);
			setAssigning(null);
		} catch (error) {
			if (String(error).includes("revision_conflict")) await load(offset);
			const message = userError(error);
			onMessage(message);
			throw new Error(message);
		} finally {
			setBusy(false);
		}
	};

	const segments =
		transcriptSource === "coarse"
			? detail.coarse_segments
			: bestSegments(detail);
	const labels = new Map(localSpeakers.map((s) => [s.group_id, s.name]));
	const manualStatus = manualAsrSummary(detail);
	const queueRelevant =
		(detail.id || detail.manual_range_id) &&
		[
			"settling_asr",
			"pending_asr",
			"transcribing",
			"asr_failed",
			"no_transcript",
			"partial_transcript",
		].includes(detail.processing_state);
	const sourceText =
		detail.processing_state === "moment_only"
			? "时刻记录"
			: detail.sources === "both"
				? "双路声音"
				: detail.sources === "microphone"
					? "麦克风"
					: "电脑声音";
	const shortEvent = detail.ended_ms - detail.started_ms < 60_000;
	const crossesDay = !sameLocalDay(detail.started_ms, detail.ended_ms);
	const playbackProgress =
		detail.ended_ms > detail.started_ms
			? Math.max(
					0,
					Math.min(
						100,
						((playhead - detail.started_ms) /
							(detail.ended_ms - detail.started_ms)) *
							100,
					),
				)
			: 0;

	return (
		<div className="drawer-layer">
			<button
				className="drawer-scrim"
				type="button"
				aria-label="关闭详情"
				onClick={onClose}
			/>
			<section className="detail-drawer" aria-label="事件详情">
				<header className="detail-header">
					<div>
						<span className="eyebrow">
							{wallClock(detail.started_ms, shortEvent, crossesDay)} —{" "}
							{wallClock(detail.ended_ms, shortEvent, crossesDay)}
						</span>
						<h2>
							{detail.title ||
								(detail.manual_range_id ? "未命名事件" : "未整理")}
						</h2>
					</div>
					<div className="detail-header-actions">
						<button
							type="button"
							className="quiet-button"
							onClick={() => onDataAction("export", detail)}
						>
							导出
						</button>
						<button
							type="button"
							className="quiet-button"
							onClick={() => {
								stopAudio();
								onDataAction("cleanup", detail);
							}}
						>
							清理
						</button>
						{detail.id && (
							<>
								<button
									type="button"
									className="quiet-button"
									onClick={() => onEdit(detail)}
								>
									编辑
								</button>
								<button
									type="button"
									className="icon-button"
									aria-label="更多事件操作"
									onClick={() => onTools(detail)}
								>
									···
								</button>
							</>
						)}
						<button
							type="button"
							className="icon-button"
							aria-label="关闭详情"
							onClick={() => {
								stopAudio();
								onClose();
							}}
						>
							×
						</button>
					</div>
				</header>
				<div className="detail-columns">
					<div className="detail-main">
						<div className="audio-player">
							<button
								type="button"
								className="play-button"
								aria-label={playing ? "暂停播放" : "播放录音"}
								onClick={() =>
									playing
										? stopAudio()
										: playFrom(
												playhead >= detail.ended_ms
													? detail.started_ms
													: playhead,
											)
								}
							>
								{playing ? "Ⅱ" : "▷"}
							</button>
							<div className="player-timeline">
								<input
									className="player-track"
									style={{
										background: `linear-gradient(to right, var(--color-brand) ${playbackProgress}%, var(--color-brand-border) ${playbackProgress}%)`,
									}}
									type="range"
									min={detail.started_ms}
									max={detail.ended_ms}
									value={Math.min(playhead, detail.ended_ms)}
									aria-label="定位录音"
									aria-valuetext={wallClock(playhead, true, crossesDay)}
									onChange={(event) => {
										const at = Number(event.target.value);
										setPlayhead(at);
										setAnchor(at);
										if (playing) playFrom(at);
									}}
								/>
								<span>
									当前位置 {wallClock(playhead, true, crossesDay)} /{" "}
									{wallClock(detail.ended_ms, true, crossesDay)}
								</span>
							</div>
							<select
								aria-label="播放来源"
								value={playSource}
								onChange={(event) => {
									const source = event.target.value as Sources;
									setPlaySource(source);
									sourceRef.current = source;
									if (playing) playFrom(playhead);
								}}
							>
								<option value="both">混合</option>
								<option value="microphone">麦克风</option>
								<option value="system">电脑声音</option>
							</select>
						</div>
						<div className="detail-toolbar">
							<div className="detail-tabs" role="tablist" aria-label="详情内容">
								<button
									type="button"
									role="tab"
									disabled={busy}
									aria-selected={tab === "transcript"}
									className={tab === "transcript" ? "active" : ""}
									onClick={() => setTab("transcript")}
								>
									文本转录
								</button>
								<button
									type="button"
									role="tab"
									disabled={busy}
									aria-selected={tab === "info"}
									className={tab === "info" ? "active" : ""}
									onClick={() => {
										setAssigning(null);
										setTab("info");
									}}
								>
									音频信息
								</button>
							</div>
							{tab === "transcript" && (
								<div className="transcript-toolbar">
									{detail.coarse_segments.length > 0 &&
										detail.precise_segments.length > 0 && (
											<button
												type="button"
												className="quiet-button"
												disabled={busy}
												onClick={() => {
													setAssigning(null);
													setTranscriptSource(
														transcriptSource === "best" ? "coarse" : "best",
													);
												}}
											>
												{transcriptSource === "best"
													? "查看本地粗转"
													: "返回精转"}
											</button>
										)}
									{!detail.id &&
										!detail.manual_range_id &&
										detail.processing_state !== "moment_only" &&
										segments.length > 0 && (
											<button
												type="button"
												className="quiet-button"
												onClick={() => {
													setAssigning(null);
													setTab("info");
												}}
											>
												选择范围精转
											</button>
										)}
								</div>
							)}
						</div>
						{tab === "transcript" ? (
							<div
								className={`detail-scroll transcript-list${crossesDay ? " crosses-day" : ""}`}
								key="transcript"
							>
								{segments.length === 0 && (
									<div className="detail-empty">
										<strong>还没有转录文字</strong>
										{(detail.id || detail.manual_range_id) && (
											<span>{processingLabel(detail.processing_state)}</span>
										)}
										{!detail.id &&
											!detail.manual_range_id &&
											detail.processing_state !== "moment_only" && (
												<button
													type="button"
													className="secondary-button"
													onClick={() => {
														setAssigning(null);
														setTab("info");
													}}
												>
													选择范围精转
												</button>
											)}
									</div>
								)}
								{segments.map((segment) => {
									const precise =
										"revision" in segment ? (segment as PreciseSegment) : null;
									const current =
										playhead >= segment.started_ms &&
										playhead < segment.ended_ms &&
										playhead < detail.ended_ms &&
										(playSource === "both" || playSource === segment.source);
									return (
										<div
											className={`transcript-row${current ? " is-current" : ""}`}
											aria-current={current ? "time" : undefined}
											key={segment.id}
										>
											<button
												type="button"
												className="transcript-time"
												title={current ? "当前位置 · 点击回听" : "从这里播放"}
												onClick={() => playFrom(segment.started_ms)}
											>
												{wallClock(segment.started_ms, true, crossesDay)}
											</button>
											<div className="transcript-content">
												<div className="transcript-meta">
													{precise ? (
														<button
															type="button"
															className="speaker-chip"
															aria-haspopup="dialog"
															aria-expanded={assigning === segment.id}
															disabled={busy}
															onClick={(event) => {
																speakerAnchor.current = event.currentTarget;
																setAssigning(
																	assigning === segment.id ? null : segment.id,
																);
															}}
														>
															{speakerName(precise, labels)}⌄
														</button>
													) : (
														<span className="speaker-chip static">
															粗转 ·{" "}
															{segment.source === "system"
																? "电脑声音"
																: "麦克风"}
														</span>
													)}
													{precise && (
														<span className="transcript-source">
															{precise.source === "system"
																? "电脑声音"
																: "麦克风"}
															{precise.person_origin === "automatic"
																? " · 自动匹配"
																: precise.person_origin === "manual"
																	? " · 已确认"
																	: ""}
														</span>
													)}
													{precise?.corrected && (
														<span className="corrected-label">已修正</span>
													)}
													{precise && (
														<button
															type="button"
															className="text-action"
															onClick={() => {
																setEditing(segment.id);
																setDraft(segment.text);
															}}
														>
															修正
														</button>
													)}
												</div>
												{editing === segment.id && precise ? (
													<div className="inline-editor">
														<textarea
															aria-label="修正这句文字"
															value={draft}
															onChange={(event) => setDraft(event.target.value)}
														/>
														<div>
															<button
																type="button"
																onClick={() => setEditing(null)}
															>
																取消
															</button>
															<button
																type="button"
																disabled={busy || !draft.trim()}
																onClick={() =>
																	void mutate(
																		{
																			type: "correct_text",
																			segment_id: segment.id,
																			text: draft.trim(),
																			expected_revision: precise.revision,
																		},
																		"文字已修正",
																	)
																}
															>
																保存
															</button>
														</div>
													</div>
												) : (
													<p>{segment.text}</p>
												)}
												{precise?.corrected && precise.original_text && (
													<details className="original-text">
														<summary>查看原识别文字</summary>
														<p>{precise.original_text}</p>
														<button
															type="button"
															disabled={busy}
															onClick={() =>
																void mutate(
																	{
																		type: "revert_text",
																		segment_id: segment.id,
																		expected_revision: precise.revision,
																	},
																	"已恢复原识别文字",
																)
															}
														>
															撤销修正
														</button>
													</details>
												)}
												{assigning === segment.id &&
													precise &&
													speakerAnchor.current && (
														<SpeakerPicker
															key={segment.id}
															anchor={speakerAnchor.current}
															people={people}
															localSpeakers={localSpeakers}
															currentGroup={precise.speaker}
															personId={precise.person_id}
															confirmed={precise.person_origin === "manual"}
															onClose={() => setAssigning(null)}
															onApply={(action) =>
																updateSpeaker(precise, action)
															}
														/>
													)}
											</div>
										</div>
									);
								})}
								{hasMore && (
									<button
										type="button"
										className="load-more"
										onClick={() => setOffset((current) => current + 100)}
									>
										显示更多文字
									</button>
								)}
							</div>
						) : (
							<div className="detail-scroll information-panel" key="info">
								<div>
									<span>时间范围</span>
									<strong>
										{new Date(detail.started_ms).toLocaleString("zh-CN")} —{" "}
										{new Date(detail.ended_ms).toLocaleString("zh-CN")}
									</strong>
								</div>
								<div>
									<span>声音来源</span>
									<strong>{sourceText}</strong>
								</div>
								<div>
									<span>处理状态</span>
									<strong>{processingLabel(detail.processing_state)}</strong>
								</div>
								{manualStatus && (
									<div>
										<span>手动精转</span>
										<strong>{manualStatus}</strong>
									</div>
								)}
								<div>
									<span>文字</span>
									<strong>
										{detail.precise_segments.length > 0
											? "精转"
											: detail.coarse_segments.length > 0
												? "本地粗转"
												: "尚无文字"}
									</strong>
								</div>
								{infoError && <p className="inline-error">{infoError}</p>}
								{!detail.id &&
									!detail.manual_range_id &&
									detail.processing_state !== "moment_only" && (
										<div className="asr-range-form">
											<label>
												开始时间
												<input
													type="datetime-local"
													step="1"
													value={asrFrom}
													onChange={(event) => setAsrFrom(event.target.value)}
												/>
											</label>
											<label>
												结束时间
												<input
													type="datetime-local"
													step="1"
													value={asrTo}
													onChange={(event) => setAsrTo(event.target.value)}
												/>
											</label>
											{detail.sources === "both" && (
												<label>
													声音来源
													<select
														value={asrSources}
														onChange={(event) =>
															setAsrSources(event.target.value as Sources)
														}
													>
														<option value="both">双路声音</option>
														<option value="microphone">麦克风</option>
														<option value="system">电脑声音</option>
													</select>
												</label>
											)}
										</div>
									)}
								{queueRelevant && (
									<button
										type="button"
										className="secondary-button"
										onClick={onQueue}
									>
										查看处理队列
									</button>
								)}
								{detail.id && (
									<button
										type="button"
										className="secondary-button"
										disabled={busy}
										onClick={() =>
											void mutate(
												{
													type: "retranscribe_event",
													event_id: detail.id,
													expected_revision: detail.revision,
												},
												"已加入精转队列",
											)
										}
									>
										重新精转
									</button>
								)}
								{!detail.id &&
									!detail.manual_range_id &&
									detail.processing_state !== "moment_only" && (
										<button
											type="button"
											className="secondary-button"
											onClick={() => void transcribe()}
											disabled={busy}
										>
											{manualStatus || detail.precise_segments.length > 0
												? "重新精转"
												: "开始精转"}
										</button>
									)}
							</div>
						)}
					</div>
					<aside className="notes-panel">
						<h3>现场笔记</h3>
						<div className="notes-stream">
							{moments.length === 0 && (
								<div className="notes-empty">还没有笔记或重点标记</div>
							)}
							{moments.map((moment) => (
								<div className={`note-message ${moment.kind}`} key={moment.id}>
									<button
										className="note-time"
										type="button"
										onClick={() => playFrom(moment.anchor_ms)}
									>
										{wallClock(moment.anchor_ms, true, crossesDay)}
									</button>
									{moment.kind === "mark" && <p>◆ 重点</p>}
									{moment.kind === "comment" && <p>{moment.text}</p>}
									{moment.kind === "screenshot" && (
										<>
											<button
												type="button"
												className="screenshot-card"
												disabled={imageBusy}
												onClick={() => void showScreenshot(moment.id)}
											>
												▧ 截屏 <span>查看画面</span>
											</button>
											{moment.ocr_text && (
												<p className="ocr-excerpt">{moment.ocr_text}</p>
											)}
											{moment.ocr_state === "failed" && (
												<small>图片已保存 · 文字识别失败</small>
											)}
										</>
									)}
								</div>
							))}
						</div>
						<div className="note-composer">
							<textarea
								aria-label="写一条笔记"
								placeholder="写一条笔记…"
								value={note}
								onChange={(event) => setNote(event.target.value)}
							/>
							<button
								type="button"
								disabled={busy || !note.trim()}
								onClick={() => void sendNote()}
							>
								发送
							</button>
						</div>
					</aside>
				</div>
			</section>
			{shownImage && (
				<div className="image-preview" role="dialog" aria-label="截图预览">
					<button
						className="image-preview-close"
						type="button"
						onClick={() => setShownImage(null)}
					>
						关闭
					</button>
					<img src={shownImage} alt="主动截取的屏幕画面" />
				</div>
			)}
		</div>
	);
}
