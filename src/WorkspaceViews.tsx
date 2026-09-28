import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import {
	type CleanupPreview,
	type CloudProvider,
	type CloudQueue,
	type CloudTask,
	core,
	dateTitle,
	dayBounds,
	executeCleanup,
	type Person,
	type PreciseSegment,
	type ReviewItem,
	type ServiceState,
	type Sources,
	type TimelineItem,
	time,
	type VoiceSample,
} from "./api";

type Mutation = (
	request: { type: string; [key: string]: unknown },
	success: string,
) => Promise<boolean>;

function localInput(timestamp: number): string {
	const d = new Date(timestamp);
	return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}T${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function taskStatus(task: CloudTask): string {
	const states: Record<string, string> = {
		pending: "等待处理",
		processing: "处理中",
		submitted: "等待识别结果",
		complete: "已完成",
		failed: "处理失败",
		superseded: "已由新范围替代",
	};
	const state = states[task.state] ?? "状态待确认";
	if (task.state !== "failed" || !task.error_code) return state;
	const reason = task.error_code;
	if (reason === "credential_missing" || reason === "provider_not_configured")
		return `${state} · 请检查云端配置`;
	if (reason === "audio_unavailable" || reason === "partial_audio")
		return `${state} · 原声不可用`;
	if (reason.startsWith("provider_http_429"))
		return `${state} · 服务繁忙，请稍后重试`;
	if (reason.startsWith("provider_")) return `${state} · 云端服务异常`;
	return state;
}

export function QueueView({
	queues,
	tasks,
	reviews,
	busy,
	onRefresh,
	onMutate,
}: {
	queues: CloudQueue[];
	tasks: CloudTask[];
	reviews: ReviewItem[];
	busy: boolean;
	onRefresh: () => Promise<void>;
	onMutate: Mutation;
}) {
	const [activeReview, setActiveReview] = useState<string | null>(null);
	const [candidates, setCandidates] = useState<PreciseSegment[]>([]);
	const [finding, setFinding] = useState(false);
	const [reviewError, setReviewError] = useState("");
	const findCandidates = async (review: ReviewItem) => {
		setActiveReview(review.id);
		setCandidates([]);
		setReviewError("");
		setFinding(true);
		try {
			const [from_ms, to_ms] = dayBounds(new Date(review.started_ms));
			const timeline = await core<{
				type: "timeline";
				id: string;
				items: TimelineItem[];
			}>({
				type: "timeline",
				from_ms,
				to_ms,
			});
			const found: PreciseSegment[] = [];
			for (const item of timeline.items.filter(
				(entry) =>
					entry.id &&
					entry.started_ms < review.ended_ms + 60_000 &&
					entry.ended_ms > review.started_ms - 60_000,
			)) {
				for (let offset = 0; ; offset += 200) {
					const detail = await core<{
						type: "event";
						id: string;
						event: TimelineItem;
					}>({
						type: "event_detail",
						event_id: item.id,
						offset,
						limit: 200,
					});
					found.push(...detail.event.precise_segments);
					if (
						detail.event.precise_segments.length +
							detail.event.coarse_segments.length <
						200
					)
						break;
				}
			}
			const range = await core<{
				type: "event";
				id: string;
				event: TimelineItem;
			}>({
				type: "range_detail",
				started_ms: Math.max(0, review.started_ms - 60_000),
				ended_ms: review.ended_ms + 60_000,
				sources: review.source,
				offset: 0,
				limit: 200,
			});
			found.push(...range.event.precise_segments);
			setCandidates(
				[
					...new Map(
						found
							.filter(
								(segment) =>
									segment.source === review.source &&
									segment.started_ms < review.ended_ms + 60_000 &&
									segment.ended_ms > review.started_ms - 60_000,
							)
							.map((segment) => [segment.id, segment]),
					).values(),
				].sort((a, b) => a.started_ms - b.started_ms),
			);
		} catch (error) {
			setReviewError(String(error));
		} finally {
			setFinding(false);
		}
	};
	const resolve = async (review: ReviewItem, segmentId: string | null) => {
		if (
			await onMutate(
				{
					type: "resolve_review",
					item_id: review.id,
					target_segment_id: segmentId,
					expected_revision: review.revision,
				},
				"核对已完成",
			)
		) {
			setActiveReview(null);
			await onRefresh();
		}
	};
	return (
		<section className="page-container">
			<div className="page-heading">
				<h1>处理队列</h1>
				<button
					type="button"
					className="quiet-button"
					onClick={() => void onRefresh()}
				>
					刷新
				</button>
			</div>
			<div className="queue-grid">
				{queues.map((queue) => (
					<div className="panel-card" key={queue.provider}>
						<div className="panel-header">
							<div>
								<h2>{queue.provider === "llm" ? "事件整理" : "精确转录"}</h2>
								<p>
									{queue.configured
										? queue.paused
											? "已暂停"
											: "运行中"
										: "尚未配置"}
								</p>
							</div>
							<button
								type="button"
								className={`switch ${!queue.paused && queue.configured ? "on" : ""}`}
								aria-label={`${queue.paused ? "继续" : "暂停"}${queue.provider === "llm" ? "事件整理" : "精确转录"}`}
								disabled={busy || !queue.configured}
								onClick={() =>
									void onMutate(
										{
											type: "set_cloud_paused",
											provider: queue.provider,
											paused: !queue.paused,
										},
										queue.paused ? "队列已继续" : "队列已暂停",
									)
								}
							/>
						</div>
						<div className="queue-counts">
							<span>
								{queue.pending}
								<small>待处理</small>
							</span>
							<span>
								{queue.processing}
								<small>处理中</small>
							</span>
							<span>
								{queue.failed}
								<small>失败</small>
							</span>
						</div>
					</div>
				))}
			</div>
			<div className="section-heading">
				<h2>任务</h2>
			</div>
			<div className="panel-card list-panel">
				{tasks.length === 0 ? (
					<div className="list-empty">暂无任务</div>
				) : (
					tasks.map((task) => (
						<div className="task-row" key={task.id}>
							<div>
								<strong>
									{task.provider === "llm" ? "事件整理" : "精确转录"} ·{" "}
									{time(task.started_ms)}
								</strong>
								<p title={task.error_code ?? undefined}>{taskStatus(task)}</p>
							</div>
							{task.state === "failed" && (
								<button
									type="button"
									className="quiet-button"
									onClick={() =>
										void onMutate(
											{ type: "retry_cloud_task", task_id: task.id },
											"任务已重新排队",
										)
									}
								>
									重试
								</button>
							)}
						</div>
					))
				)}
			</div>
			<div className="section-heading">
				<h2>待核对</h2>
				<span className="review-count">{reviews.length} 条</span>
			</div>
			<div className="panel-card list-panel">
				{reviews.length === 0 && (
					<div className="list-empty">没有待核对的修正</div>
				)}
				{reviews.map((review) => (
					<div className="review-row" key={review.id}>
						<div>
							<strong>
								{review.kind === "text" ? "文字修正" : "人物归属"} ·{" "}
								{time(review.started_ms)}
							</strong>
							<p>{review.text || review.person_name || "原归属"}</p>
						</div>
						<button
							type="button"
							className="quiet-button"
							onClick={() => void findCandidates(review)}
						>
							核对
						</button>
						{activeReview === review.id && (
							<div className="review-candidates">
								{finding && <span>正在查找当前句段…</span>}
								{reviewError && (
									<span className="inline-error">{reviewError}</span>
								)}
								{!finding && candidates.length === 0 && (
									<span>没有可对应的精转句段</span>
								)}
								{candidates.map((segment) => (
									<button
										type="button"
										key={segment.id}
										onClick={() => void resolve(review, segment.id)}
									>
										<time>{time(segment.started_ms)}</time>
										<span>{segment.text}</span>
									</button>
								))}
								<button
									type="button"
									className="review-dismiss"
									onClick={() => void resolve(review, null)}
								>
									{review.kind === "text"
										? "放弃这条文字修正"
										: "放弃这条人物归属"}
								</button>
							</div>
						)}
					</div>
				))}
			</div>
		</section>
	);
}

export function PeopleView({
	people,
	busy,
	onMutate,
}: {
	people: Person[];
	busy: boolean;
	onMutate: Mutation;
}) {
	const [creating, setCreating] = useState(false);
	const [editing, setEditing] = useState<string | null>(null);
	const [name, setName] = useState("");
	const [expanded, setExpanded] = useState<string | null>(null);
	const [samples, setSamples] = useState<VoiceSample[]>([]);
	const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
	const loadSamples = async (personId: string) => {
		try {
			const response = await core<{
				type: "voice_samples";
				id: string;
				samples: VoiceSample[];
			}>({
				type: "voice_samples",
				person_id: personId,
			});
			setSamples(response.samples);
			setExpanded(personId);
		} catch {
			setSamples([]);
		}
	};
	return (
		<section className="page-container">
			<div className="page-heading">
				<h1>人物库</h1>
				<button
					type="button"
					className="quiet-button"
					onClick={() => {
						setCreating(true);
						setName("");
					}}
				>
					＋ 新建人物
				</button>
			</div>
			<div className="panel-card list-panel">
				{creating && (
					<div className="person-edit-row">
						<input
							aria-label="人物姓名"
							value={name}
							onChange={(event) => setName(event.target.value)}
							placeholder="输入姓名"
						/>
						<button
							type="button"
							className="quiet-button"
							onClick={() => setCreating(false)}
						>
							取消
						</button>
						<button
							type="button"
							className="primary-button"
							disabled={busy || !name.trim()}
							onClick={async () => {
								const saved = await onMutate(
									{ type: "create_person", display_name: name.trim() },
									"人物已添加",
								);
								if (saved) setCreating(false);
							}}
						>
							保存
						</button>
					</div>
				)}
				{people.length === 0 && !creating && (
					<div className="list-empty">还没有人物。可在转录中为说话人命名。</div>
				)}
				{people.map((person) => (
					<div className="person-entry" key={person.id}>
						<div className="person-row">
							<span className="person-avatar">
								{person.display_name.slice(0, 1)}
							</span>
							{editing === person.id ? (
								<div className="person-edit-row">
									<input
										aria-label="修改姓名"
										value={name}
										onChange={(event) => setName(event.target.value)}
									/>
									<button
										type="button"
										className="quiet-button"
										onClick={() => setEditing(null)}
									>
										取消
									</button>
									<button
										type="button"
										className="primary-button"
										disabled={busy || !name.trim()}
										onClick={async () => {
											const saved = await onMutate(
												{
													type: "rename_person",
													person_id: person.id,
													display_name: name.trim(),
													expected_revision: person.revision,
												},
												"姓名已更新",
											);
											if (saved) setEditing(null);
										}}
									>
										保存
									</button>
								</div>
							) : (
								<>
									<div>
										<strong>{person.display_name}</strong>
										<p>{person.sample_count} 个声纹样例</p>
									</div>
									<button
										type="button"
										className="quiet-button"
										onClick={() => {
											setEditing(person.id);
											setName(person.display_name);
										}}
									>
										编辑
									</button>
									<button
										type="button"
										className="quiet-button"
										onClick={() =>
											expanded === person.id
												? setExpanded(null)
												: void loadSamples(person.id)
										}
									>
										样例
									</button>
									<button
										type="button"
										className="text-action"
										onClick={() => setConfirmDelete(person.id)}
									>
										删除
									</button>
								</>
							)}
						</div>
						{expanded === person.id && (
							<div className="sample-list">
								{samples.length === 0 && <span>暂无可用样例</span>}
								{samples.map((sample) => (
									<div className="sample-row" key={sample.id}>
										<span>
											{new Date(sample.started_ms).toLocaleString("zh-CN")} ·{" "}
											{sample.source === "system" ? "电脑声音" : "麦克风"}
										</span>
										<button
											type="button"
											className="text-action"
											disabled={busy}
											onClick={async () => {
												if (
													await onMutate(
														{
															type: "remove_voice_sample",
															sample_id: sample.id,
															expected_revision: sample.revision,
														},
														"声纹样例已移除",
													)
												)
													await loadSamples(person.id);
											}}
										>
											移除
										</button>
									</div>
								))}
							</div>
						)}
						{confirmDelete === person.id && (
							<div className="person-delete-confirm">
								<span>
									删除 {person.display_name} 及其样例？历史转录仍保留。
								</span>
								<button
									type="button"
									className="quiet-button"
									onClick={() => setConfirmDelete(null)}
								>
									取消
								</button>
								<button
									type="button"
									className="danger-button"
									disabled={busy}
									onClick={async () => {
										if (
											await onMutate(
												{
													type: "delete_person",
													person_id: person.id,
													expected_revision: person.revision,
												},
												"人物已删除",
											)
										) {
											setConfirmDelete(null);
											setExpanded(null);
										}
									}}
								>
									删除人物
								</button>
							</div>
						)}
					</div>
				))}
			</div>
		</section>
	);
}

export function SettingsView({
	date,
	state,
	selection,
	privacy,
	loginStatus,
	models,
	busy,
	onStatus,
	onContext,
	onTimeline,
	onMutate,
	onMessage,
}: {
	date: Date;
	state: ServiceState | null;
	selection: { kind: "export" | "cleanup"; item: TimelineItem } | null;
	privacy: { microphone: number; screen_and_system_audio: number } | null;
	loginStatus: number | null;
	models: {
		phase: string;
		speaker_phase: string;
		progress_percent: number;
	} | null;
	busy: boolean;
	onStatus: () => Promise<void>;
	onContext: () => Promise<void>;
	onTimeline: () => Promise<void>;
	onMutate: Mutation;
	onMessage: (message: string) => void;
}) {
	const [provider, setProvider] = useState<CloudProvider>("llm");
	const [apiKey, setApiKey] = useState("");
	const [model, setModel] = useState("");
	const [cleanupOpen, setCleanupOpen] = useState(selection?.kind === "cleanup");
	const [cleanupPreview, setCleanupPreview] = useState<CleanupPreview | null>(
		null,
	);
	const [affectedTitles, setAffectedTitles] = useState<string[]>([]);
	const [cleanupMode, setCleanupMode] = useState<"raw_audio" | "all_data">(
		"raw_audio",
	);
	const [exportOpen, setExportOpen] = useState(selection?.kind === "export");
	const [exportPath, setExportPath] = useState("");
	const [exportAudio, setExportAudio] = useState(false);
	const [exportScreenshots, setExportScreenshots] = useState(false);
	const [working, setWorking] = useState(false);
	const [audioLimitGiB, setAudioLimitGiB] = useState("");
	useEffect(() => {
		if (state?.audio_limit_bytes)
			setAudioLimitGiB(
				String(+(state.audio_limit_bytes / 1_073_741_824).toFixed(2)),
			);
	}, [state?.audio_limit_bytes]);
	const [dayStart, dayEnd] = dayBounds(date);
	const [scope, setScope] = useState<"selection" | "custom" | "all">(
		selection ? "selection" : "custom",
	);
	const [from, setFrom] = useState(
		localInput(selection?.item.started_ms ?? dayStart),
	);
	const [to, setTo] = useState(localInput(selection?.item.ended_ms ?? dayEnd));
	const [sources, setSources] = useState<Sources>(
		selection?.item.sources ?? "both",
	);
	const selectedRange = (): {
		from_ms: number;
		to_ms: number;
		sources: Sources;
	} => {
		if (scope === "all")
			return { from_ms: 0, to_ms: Date.now() + 1_000, sources: "both" };
		if (scope === "selection" && selection)
			return {
				from_ms: selection.item.started_ms,
				to_ms: selection.item.ended_ms,
				sources: selection.item.sources,
			};
		const from_ms = new Date(from).getTime();
		const to_ms = new Date(to).getTime();
		if (
			!Number.isFinite(from_ms) ||
			!Number.isFinite(to_ms) ||
			from_ms < 0 ||
			to_ms <= from_ms
		)
			throw new Error("请选择有效的时间范围");
		return { from_ms, to_ms, sources };
	};

	const configureCloud = async () => {
		if (!apiKey.trim()) {
			onMessage("请输入 API Key");
			return;
		}
		const saved = await onMutate(
			{
				type: "configure_cloud",
				provider,
				api_key: apiKey.trim(),
				model: model.trim() || null,
			},
			"云端能力已配置",
		);
		if (saved) setApiKey("");
	};

	const preview = async () => {
		setWorking(true);
		try {
			const chosen = selectedRange();
			const response = await core<{
				type: "cleanup_preview";
				id: string;
				preview: CleanupPreview;
			}>({
				type: "preview_cleanup",
				...chosen,
				mode: cleanupMode,
			});
			const names = await Promise.all(
				response.preview.affected_events.slice(0, 8).map(async (event_id) => {
					try {
						const found = await core<{
							type: "event";
							id: string;
							event: TimelineItem;
						}>({ type: "event_detail", event_id, offset: 0, limit: 1 });
						return found.event.title || "未命名事件";
					} catch {
						return "受影响事件";
					}
				}),
			);
			setAffectedTitles(names);
			setCleanupPreview(response.preview);
		} catch (error) {
			onMessage(String(error));
		} finally {
			setWorking(false);
		}
	};

	const cleanup = async () => {
		if (!cleanupPreview) return;
		setWorking(true);
		try {
			await executeCleanup(
				cleanupPreview.token,
				cleanupPreview.mode === "all_data",
			);
			onMessage("清理已完成");
			setCleanupOpen(false);
			setCleanupPreview(null);
			await Promise.all([onStatus(), onTimeline(), onContext()]);
		} catch (error) {
			onMessage(String(error));
		} finally {
			setWorking(false);
		}
	};

	const exportHistory = async () => {
		setWorking(true);
		try {
			const chosen = selectedRange();
			await core({
				type: "export_range",
				...chosen,
				include_audio: exportAudio,
				include_screenshots: exportScreenshots,
				destination: exportPath.trim(),
			});
			onMessage("导出已完成");
			setExportOpen(false);
		} catch (error) {
			onMessage(String(error));
		} finally {
			setWorking(false);
		}
	};

	const chooseExportPath = async () => {
		try {
			const path = await save({
				defaultPath: `hear-${localInput(dayStart).slice(0, 10)}.tar`,
				filters: [{ name: "hear 归档", extensions: ["tar"] }],
			});
			if (path) setExportPath(path);
		} catch (error) {
			onMessage(String(error));
		}
	};

	return (
		<section className="page-container settings-page">
			<div className="page-heading">
				<h1>设置</h1>
			</div>
			<h2>记录</h2>
			<div className="panel-card setting-panel">
				<div className="setting-row">
					<div>
						<strong>登录时启动</strong>
						<p>
							{loginStatus === 1
								? "已开启"
								: loginStatus === 2
									? "需要在系统设置中批准"
									: "未开启"}
						</p>
					</div>
					<button
						type="button"
						className="quiet-button"
						onClick={async () => {
							try {
								await invoke(
									loginStatus === 1
										? "unregister_login_item"
										: "register_login_item",
								);
								await onContext();
							} catch (error) {
								onMessage(String(error));
							}
						}}
					>
						{loginStatus === 1 ? "关闭" : "开启"}
					</button>
				</div>
				<div className="setting-row">
					<div>
						<strong>麦克风</strong>
						<p>
							{privacy?.microphone === 3
								? state?.microphone_recording
									? "录制中"
									: "已授权"
								: "需要系统授权"}
						</p>
					</div>
					{privacy?.microphone === 3 ? (
						<button
							type="button"
							className="quiet-button"
							disabled={busy}
							onClick={() =>
								void onMutate(
									{
										type: "set_source",
										source: "microphone",
										enabled: !state?.microphone_enabled,
										expected_revision: state?.revision,
									},
									"声音来源已更新",
								)
							}
						>
							{state?.microphone_enabled ? "关闭" : "开启"}
						</button>
					) : (
						<button
							type="button"
							className="quiet-button"
							onClick={async () => {
								await invoke("request_microphone_permission");
								await onContext();
							}}
						>
							允许访问
						</button>
					)}
				</div>
				<div className="setting-row">
					<div>
						<strong>电脑声音</strong>
						<p>
							{privacy?.screen_and_system_audio === 1
								? state?.system_recording
									? "录制中"
									: "已授权"
								: "需要系统授权"}
						</p>
					</div>
					{privacy?.screen_and_system_audio === 1 ? (
						<button
							type="button"
							className="quiet-button"
							disabled={busy}
							onClick={() =>
								void onMutate(
									{
										type: "set_source",
										source: "system",
										enabled: !state?.system_enabled,
										expected_revision: state?.revision,
									},
									"声音来源已更新",
								)
							}
						>
							{state?.system_enabled ? "关闭" : "开启"}
						</button>
					) : (
						<button
							type="button"
							className="quiet-button"
							onClick={async () => {
								await invoke("request_system_audio_permission");
								await onContext();
							}}
						>
							允许访问
						</button>
					)}
				</div>
				<div className="setting-row">
					<div>
						<strong>本地模型</strong>
						<p>
							{models?.phase === "ready"
								? "语音识别已就绪"
								: models?.phase === "downloading"
									? `语音识别下载中 · ${models.progress_percent}%`
									: "语音识别尚未就绪"}
							{" · "}
							{models?.speaker_phase === "ready"
								? "声纹已就绪"
								: models?.speaker_phase === "downloading"
									? "声纹下载中"
									: "声纹尚未就绪"}
						</p>
					</div>
					<button
						type="button"
						className="quiet-button"
						disabled={busy}
						onClick={() =>
							void onMutate({ type: "prepare_models" }, "正在准备模型")
						}
					>
						准备模型
					</button>
				</div>
				<div className="setting-row">
					<div>
						<strong>音频存储上限</strong>
						<p>
							已用 {((state?.audio_used_bytes ?? 0) / 1_073_741_824).toFixed(2)}{" "}
							GB
							{state?.storage_paused
								? " · 录音已暂停"
								: state?.storage_warning
									? " · 空间即将不足"
									: ""}
						</p>
					</div>
					<div className="setting-value-action">
						<input
							type="number"
							min="0.0625"
							max="1024"
							step="0.25"
							aria-label="音频存储上限，GB"
							value={audioLimitGiB}
							onChange={(event) => setAudioLimitGiB(event.target.value)}
						/>
						<span>GB</span>
						<button
							type="button"
							className="quiet-button"
							disabled={busy}
							onClick={() => {
								const value = Number(audioLimitGiB);
								if (!Number.isFinite(value) || value < 0.0625 || value > 1024) {
									onMessage("请输入 0.0625 至 1024 GB 的上限");
									return;
								}
								void onMutate(
									{
										type: "set_audio_limit",
										limit_bytes: Math.round(value * 1_073_741_824),
										expected_revision: state?.revision,
									},
									"存储上限已更新",
								);
							}}
						>
							保存
						</button>
					</div>
				</div>
			</div>
			<h2>云端能力</h2>
			<div className="panel-card cloud-config">
				<div className="cloud-switcher">
					<button
						type="button"
						className={provider === "llm" ? "active" : ""}
						onClick={() => setProvider("llm")}
					>
						LLM
					</button>
					<button
						type="button"
						className={provider === "asr" ? "active" : ""}
						onClick={() => setProvider("asr")}
					>
						ASR
					</button>
				</div>
				<label>
					API Key
					<input
						type="password"
						autoComplete="off"
						value={apiKey}
						onChange={(event) => setApiKey(event.target.value)}
						placeholder="输入密钥"
					/>
				</label>
				<label>
					模型或资源标识
					<input
						value={model}
						onChange={(event) => setModel(event.target.value)}
						placeholder={
							provider === "llm" ? "使用默认模型" : "volc.seedasr.auc"
						}
					/>
				</label>
				<button
					type="button"
					className="primary-button"
					disabled={busy}
					onClick={() => void configureCloud()}
				>
					保存并验证
				</button>
			</div>
			<h2>数据</h2>
			<div className="panel-card setting-panel">
				<div className="setting-row">
					<div>
						<strong>导出记录</strong>
						<p>导出 {dateTitle(date)} 的文字和选定音频</p>
					</div>
					<button
						type="button"
						className="quiet-button"
						onClick={() => setExportOpen(true)}
					>
						导出
					</button>
				</div>
				<div className="setting-row">
					<div>
						<strong>清理记录</strong>
						<p>默认清除原始音频</p>
					</div>
					<button
						type="button"
						className="quiet-button"
						onClick={() => {
							setCleanupMode("raw_audio");
							setCleanupPreview(null);
							setCleanupOpen(true);
						}}
					>
						清理
					</button>
				</div>
			</div>
			{(cleanupOpen || exportOpen) && (
				<div className="modal-layer">
					<button
						type="button"
						className="modal-scrim"
						aria-label="关闭"
						onClick={() => {
							setCleanupOpen(false);
							setExportOpen(false);
							setCleanupPreview(null);
						}}
					/>
					<div className="modal">
						<div className="modal-title">
							<h2>{cleanupOpen ? "清理记录" : "导出记录"}</h2>
							<button
								type="button"
								className="icon-button"
								onClick={() => {
									setCleanupOpen(false);
									setExportOpen(false);
									setCleanupPreview(null);
								}}
							>
								×
							</button>
						</div>
						{cleanupPreview ? (
							<>
								<p className="preview-scope">
									{scope === "selection" && selection
										? selection.item.title || "未整理时间块"
										: scope === "all"
											? "全部历史"
											: "所选时间范围"}{" "}
									· {new Date(cleanupPreview.from_ms).toLocaleString("zh-CN")} —{" "}
									{new Date(cleanupPreview.to_ms).toLocaleString("zh-CN")}
								</p>
								<p>
									此范围包含 {cleanupPreview.audio_blocks} 段音频，约{" "}
									{(cleanupPreview.audio_bytes / 1_048_576).toFixed(1)} MB。
									{cleanupPreview.mode === "all_data"
										? `还将移除 ${cleanupPreview.coarse_segments + cleanupPreview.precise_segments} 段文字、${cleanupPreview.moment_count} 条时刻记录（含 ${cleanupPreview.screenshot_count} 张截图）及 ${cleanupPreview.voice_samples} 个声纹样例。时刻记录不属于某一路声音，也会按所选时间清理。`
										: "文字与声纹特征会保留。"}
								</p>
								{cleanupPreview.affected_event_count > 0 && (
									<p>
										涉及 {cleanupPreview.affected_event_count} 个事件
										{affectedTitles.length > 0
											? `：${affectedTitles.join("、")}${cleanupPreview.affected_event_count > affectedTitles.length ? "等" : ""}`
											: ""}
										。
									</p>
								)}
								{cleanupPreview.asr_tasks_awaiting_upload > 0 && (
									<p className="warning-text">
										{cleanupPreview.asr_tasks_awaiting_upload}{" "}
										项待上传精转将无法再使用原音频。
									</p>
								)}
								{cleanupPreview.voice_extractions_awaiting_audio > 0 && (
									<p className="warning-text">
										{cleanupPreview.voice_extractions_awaiting_audio}{" "}
										项说话人声纹提取仍需要原音频。
									</p>
								)}
								{cleanupPreview.overlap_may_remove_whole_sentences && (
									<p className="warning-text">边界内的句子可能需要整句移除。</p>
								)}
								<div className="modal-actions">
									<button
										type="button"
										className="quiet-button"
										onClick={() => setCleanupPreview(null)}
									>
										返回
									</button>
									<button
										type="button"
										className="danger-button"
										disabled={working}
										onClick={() => void cleanup()}
									>
										{cleanupPreview.mode === "all_data"
											? "继续清除全部数据"
											: "清除原始音频"}
									</button>
								</div>
							</>
						) : (
							<>
								<label>
									范围
									<select
										value={scope}
										onChange={(event) =>
											setScope(
												event.target.value as "selection" | "custom" | "all",
											)
										}
									>
										{selection && (
											<option value="selection">
												{selection.item.title || "未整理时间块"}
											</option>
										)}
										<option value="custom">自定时间</option>
										<option value="all">全部历史</option>
									</select>
								</label>
								{scope === "selection" && selection && (
									<p className="preview-scope">
										{new Date(selection.item.started_ms).toLocaleString(
											"zh-CN",
										)}{" "}
										—{" "}
										{new Date(selection.item.ended_ms).toLocaleString("zh-CN")}
									</p>
								)}
								{scope === "all" && (
									<p className="preview-scope">
										从第一条记录至现在，包含全部声音来源与时刻动作。
									</p>
								)}
								{scope === "custom" && (
									<div className="field-grid">
										<label>
											开始
											<input
												type="datetime-local"
												value={from}
												onChange={(event) => setFrom(event.target.value)}
											/>
										</label>
										<label>
											结束
											<input
												type="datetime-local"
												value={to}
												onChange={(event) => setTo(event.target.value)}
											/>
										</label>
									</div>
								)}
								{scope === "custom" && (
									<label>
										声音来源
										<select
											value={sources}
											onChange={(event) =>
												setSources(event.target.value as Sources)
											}
										>
											<option value="both">双路声音</option>
											<option value="microphone">麦克风</option>
											<option value="system">电脑声音</option>
										</select>
									</label>
								)}
								{cleanupOpen ? (
									<>
										<div className="choice-list">
											<label
												className={
													cleanupMode === "raw_audio" ? "selected" : ""
												}
											>
												<input
													type="radio"
													checked={cleanupMode === "raw_audio"}
													onChange={() => setCleanupMode("raw_audio")}
												/>
												<span>
													<strong>清除原始音频</strong>
													<small>文字和已提取声纹特征继续保留</small>
												</span>
											</label>
											<label
												className={cleanupMode === "all_data" ? "selected" : ""}
											>
												<input
													type="radio"
													checked={cleanupMode === "all_data"}
													onChange={() => setCleanupMode("all_data")}
												/>
												<span>
													<strong>清除全部数据</strong>
													<small>还会移除文字、截图和相关声纹样例</small>
												</span>
											</label>
										</div>
										<div className="modal-actions">
											<button
												type="button"
												className="quiet-button"
												onClick={() => setCleanupOpen(false)}
											>
												取消
											</button>
											<button
												type="button"
												className="primary-button"
												disabled={working}
												onClick={() => void preview()}
											>
												查看影响
											</button>
										</div>
									</>
								) : (
									<>
										<label>
											保存位置
											<div className="export-picker">
												<input
													value={exportPath}
													readOnly
													placeholder="选择保存位置"
												/>
												<button
													type="button"
													className="quiet-button"
													onClick={() => void chooseExportPath()}
												>
													选择…
												</button>
											</div>
										</label>
										<label className="check-line">
											<input
												type="checkbox"
												checked={exportAudio}
												onChange={(event) =>
													setExportAudio(event.target.checked)
												}
											/>
											包含原始音频
										</label>
										<label className="check-line">
											<input
												type="checkbox"
												checked={exportScreenshots}
												onChange={(event) =>
													setExportScreenshots(event.target.checked)
												}
											/>
											包含截图
										</label>
										<div className="modal-actions">
											<button
												type="button"
												className="quiet-button"
												onClick={() => setExportOpen(false)}
											>
												取消
											</button>
											<button
												type="button"
												className="primary-button"
												disabled={working || !exportPath.trim()}
												onClick={() => void exportHistory()}
											>
												导出
											</button>
										</div>
									</>
								)}
							</>
						)}
					</div>
				</div>
			)}
		</section>
	);
}
