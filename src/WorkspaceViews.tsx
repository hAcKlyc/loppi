import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useRef, useState } from "react";
import { AudioStorageSettings } from "./AudioStorageSettings";
import {
	type CleanupPreview,
	type CloudQueue,
	type CloudTask,
	core,
	dateTitle,
	dayBounds,
	executeCleanup,
	type PreciseSegment,
	type ReviewItem,
	type ServiceState,
	type Sources,
	type TimelineItem,
	time,
	userError,
} from "./api";
import { CloudServiceSettings } from "./CloudServiceSettings";
import {
	Button,
	Modal,
	SegmentedOptions,
	SettingRow,
	Switch,
	TabPanel,
	Tabs,
} from "./Controls";
import { taskNeedsRecognition, taskStatus } from "./cloud-task-status";
import {
	type EntryMode,
	type EntrySnapshot,
	getEntrySnapshot,
} from "./entry-api";
import { LoadBoundary, LoadingSkeleton } from "./LoadingState";
import { RecordingScheduleSettings } from "./RecordingScheduleSettings";
import { ModelPreparation, SourcePreparation } from "./SetupGroups";
import type { ModelState, PrivacyStatus } from "./setup";
import { UsageStatisticsView } from "./UsageStatistics";

type Mutation = (
	request: { type: string; [key: string]: unknown },
	success: string,
) => Promise<boolean>;

function localInput(timestamp: number): string {
	const d = new Date(timestamp);
	return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}T${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function taskRange(task: CloudTask): string {
	const started = new Date(task.started_ms);
	const ended = new Date(task.ended_ms);
	const date = `${started.getMonth() + 1}/${started.getDate()}`;
	const endDate =
		started.toDateString() === ended.toDateString()
			? ""
			: `${ended.getMonth() + 1}/${ended.getDate()} `;
	const sources = task.sources ?? task.source;
	const source =
		sources === "both"
			? " · 双路声音"
			: sources === "microphone"
				? " · 麦克风"
				: sources === "system"
					? " · 电脑声音"
					: "";
	return `${date} ${time(task.started_ms)}—${endDate}${time(task.ended_ms)}${source}`;
}

export function QueueView({
	queuesReady = true,
	queuesError,
	tasksReady = true,
	tasksError,
	reviewsReady = true,
	reviewsError,
	queues,
	tasks,
	reviews,
	busy,
	onRefresh,
	onMutate,
}: {
	queues: CloudQueue[];
	queuesReady?: boolean;
	queuesError?: string;
	tasksReady?: boolean;
	tasksError?: string;
	reviewsReady?: boolean;
	reviewsError?: string;
	tasks: CloudTask[];
	reviews: ReviewItem[];
	busy: boolean;
	onRefresh: () => Promise<void>;
	onMutate: Mutation;
}) {
	const tabsId = useId();
	const [queueTab, setQueueTab] = useState<"queue" | "statistics">("queue");
	const [statisticsRefresh, setStatisticsRefresh] = useState(0);
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
			setReviewError(userError(error));
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
			<div className="page-heading" data-tauri-drag-region>
				<h1>处理队列</h1>
				<Button
					type="button"
					tone="quiet"
					onClick={() => {
						setStatisticsRefresh((value) => value + 1);
						void onRefresh();
					}}
				>
					刷新
				</Button>
			</div>
			<Tabs
				id={tabsId}
				label="处理队列页面"
				value={queueTab}
				className="queue-tabs"
				items={[
					{ value: "queue", label: "队列" },
					{ value: "statistics", label: "统计数据" },
				]}
				onChange={setQueueTab}
			/>
			<TabPanel tabsId={tabsId} value="statistics" selected={queueTab}>
				{queueTab === "statistics" && (
					<UsageStatisticsView refresh={statisticsRefresh} />
				)}
			</TabPanel>
			<TabPanel tabsId={tabsId} value="queue" selected={queueTab}>
				<LoadBoundary
					ready={queuesReady}
					error={queuesError}
					retry={() => void onRefresh()}
					label="队列状态"
					rows={2}
				>
					<div className="queue-grid">
						{queues.map((queue) => (
							<div className="panel-card" key={queue.provider}>
								<div className="panel-header">
									<div>
										<h2>
											{queue.provider === "llm" ? "事件与图片理解" : "精确转录"}
										</h2>
										<p>
											{queue.configured
												? queue.blocked_reason === "provider_http_402"
													? "余额不足，充值后继续；失败项需手动重试"
													: queue.paused
														? "已暂停"
														: "运行中"
												: "尚未配置"}
										</p>
									</div>
									<button
										type="button"
										className={`switch ${!queue.paused && !queue.blocked_reason && queue.configured ? "on" : ""}`}
										aria-label={`${queue.paused || queue.blocked_reason ? "继续" : "暂停"}${queue.provider === "llm" ? "事件与图片理解" : "精确转录"}`}
										disabled={busy || !queue.configured}
										onClick={() =>
											void onMutate(
												{
													type: "set_cloud_paused",
													provider: queue.provider,
													paused: !queue.paused && !queue.blocked_reason,
												},
												queue.paused || queue.blocked_reason
													? "队列已继续"
													: "队列已暂停",
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
				</LoadBoundary>
				<div className="section-heading">
					<h2>任务</h2>
				</div>
				<div className="panel-card list-panel">
					<LoadBoundary
						ready={tasksReady}
						error={tasksError}
						retry={() => void onRefresh()}
						label="任务"
					>
						{tasks.length === 0 ? (
							<div className="list-empty">暂无任务</div>
						) : (
							tasks.map((task) => (
								<div className="task-row" key={task.id}>
									<div>
										<strong>
											{task.target_kind === "reorganization"
												? "用户发起的事件整理"
												: task.target_kind === "screenshot"
													? "图片理解"
													: task.provider === "llm"
														? "事件整理"
														: "精确转录"}{" "}
											· {taskRange(task)}
										</strong>
										<p title={task.error_code ?? undefined}>
											{taskStatus(task)}
										</p>
										{taskNeedsRecognition(task) && (
											<p>将用保留原声重新识别，产生新的识别用量</p>
										)}
									</div>
									{task.state === "failed" && (
										<Button
											type="button"
											tone="quiet"
											disabled={busy}
											onClick={() =>
												void onMutate(
													{ type: "retry_cloud_task", task_id: task.id },
													taskNeedsRecognition(task)
														? "已加入重新识别队列"
														: "任务已重新排队",
												)
											}
										>
											{taskNeedsRecognition(task) ? "重新识别" : "重试"}
										</Button>
									)}
								</div>
							))
						)}
					</LoadBoundary>
				</div>
				<div className="section-heading">
					<h2>待核对</h2>
					{reviewsReady && (
						<span className="review-count">{reviews.length} 条</span>
					)}
				</div>
				<div className="panel-card list-panel">
					<LoadBoundary
						ready={reviewsReady}
						error={reviewsError}
						retry={() => void onRefresh()}
						label="待核对记录"
					>
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
								<Button
									type="button"
									tone="quiet"
									onClick={() => void findCandidates(review)}
								>
									核对
								</Button>
								{activeReview === review.id && (
									<div className="review-candidates">
										{finding && (
											<LoadingSkeleton label="正在查找当前句段" rows={2} />
										)}
										{reviewError && (
											<span className="inline-error">{reviewError}</span>
										)}
										{!finding && !reviewError && candidates.length === 0 && (
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
					</LoadBoundary>
				</div>
			</TabPanel>
		</section>
	);
}

export function SettingsView({
	queuesReady = true,
	loadErrors = {},
	date,
	state,
	queues,
	selection,
	privacy,
	loginStatus,
	models,
	onStatus,
	onContext,
	onTimeline,
	onMessage,
}: {
	date: Date;
	state: ServiceState | null;
	queues: CloudQueue[];
	queuesReady?: boolean;
	loadErrors?: Partial<
		Record<"queues" | "models" | "privacy" | "login" | "state", string>
	>;
	selection: { kind: "export" | "cleanup"; item: TimelineItem } | null;
	privacy: PrivacyStatus | null;
	loginStatus: number | null;
	models: ModelState | null;
	busy: boolean;
	onStatus: () => Promise<void>;
	onContext: () => Promise<void>;
	onTimeline: () => Promise<void>;
	onMutate: Mutation;
	onMessage: (message: string) => void;
}) {
	const [dialogError, setDialogError] = useState("");
	const previewBack = useRef<HTMLButtonElement>(null);
	const [entryMode, setEntryMode] = useState<EntryMode | null>(null);
	const [entryError, setEntryError] = useState("");
	const [entryReload, setEntryReload] = useState(0);
	const [switchingEntry, setSwitchingEntry] = useState(false);
	const [switchingLogin, setSwitchingLogin] = useState(false);
	useEffect(() => {
		void entryReload;
		let disposed = false;
		const subscription = listen<EntrySnapshot>(
			"loppi-entry-snapshot",
			(event) => {
				if (!disposed) {
					setEntryMode(event.payload.mode);
					setEntryError("");
				}
			},
		);
		setEntryError("");
		void getEntrySnapshot()
			.then((snapshot) => {
				if (!disposed) setEntryMode(snapshot.mode);
			})
			.catch((cause) => {
				if (!disposed) setEntryError(userError(cause));
			});
		return () => {
			disposed = true;
			void subscription.then((unlisten) => unlisten());
		};
	}, [entryReload]);
	const changeEntryMode = async (mode: EntryMode) => {
		setSwitchingEntry(true);
		try {
			await invoke("entry_mode", { mode });
			setEntryMode(mode);
		} catch (error) {
			setDialogError(userError(error));
			onMessage(userError(error));
		} finally {
			setSwitchingEntry(false);
		}
	};
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
	const [previewing, setPreviewing] = useState(false);
	const previewGeneration = useRef(0);
	const closeDataDialog = () => {
		previewGeneration.current++;
		setPreviewing(false);
		setCleanupOpen(false);
		setExportOpen(false);
		setCleanupPreview(null);
	};
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

	const preview = async () => {
		const generation = ++previewGeneration.current;
		setDialogError("");
		setPreviewing(true);
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
			if (generation !== previewGeneration.current) return;
			setAffectedTitles(names);
			setCleanupPreview(response.preview);
			requestAnimationFrame(() => previewBack.current?.focus());
		} catch (error) {
			if (generation !== previewGeneration.current) return;
			setDialogError(userError(error));
			onMessage(userError(error));
		} finally {
			if (generation === previewGeneration.current) setPreviewing(false);
		}
	};

	const cleanup = async () => {
		setDialogError("");
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
			setDialogError(userError(error));
			onMessage(userError(error));
		} finally {
			setWorking(false);
		}
	};

	const exportHistory = async () => {
		setDialogError("");
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
			setDialogError(userError(error));
			onMessage(userError(error));
		} finally {
			setWorking(false);
		}
	};

	const chooseExportPath = async () => {
		try {
			const path = await save({
				defaultPath: `loppi-${localInput(dayStart).slice(0, 10)}.tar`,
				filters: [{ name: "loppi 归档", extensions: ["tar"] }],
			});
			if (path) setExportPath(path);
		} catch (error) {
			setDialogError(userError(error));
			onMessage(userError(error));
		}
	};

	return (
		<section className="page-container settings-page">
			<div className="page-heading" data-tauri-drag-region>
				<h1>设置</h1>
			</div>
			<h2>通用</h2>
			<div className="panel-card setting-panel">
				<LoadBoundary
					ready={entryMode !== null}
					error={entryError}
					retry={() => setEntryReload((v) => v + 1)}
					label="兔子的位置"
					rows={1}
				>
					<SettingRow
						title="兔子的位置"
						description="切换入口，保留当前倾听状态"
						action={
							<SegmentedOptions
								label="常驻入口模式"
								value={entryMode ?? "top"}
								options={[
									{ value: "top", label: "顶部" },
									{ value: "floating", label: "悬浮兔子" },
								]}
								disabled={switchingEntry}
								onChange={(mode) => void changeEntryMode(mode)}
							/>
						}
					/>
				</LoadBoundary>
				<LoadBoundary
					ready={loginStatus !== null}
					error={loadErrors.login}
					retry={() => void onContext()}
					label="登录时启动"
					rows={1}
				>
					<div className="setting-row">
						<div>
							<div className="setting-heading">
								<strong>登录时启动</strong>
								<span className="setting-status" aria-live="polite">
									{switchingLogin
										? "正在应用…"
										: loginStatus === 1
											? "已开启"
											: loginStatus === 2
												? "需要在系统设置中批准"
												: "未开启"}
								</span>
							</div>
							<p>登录电脑后自动启动 loppi</p>
						</div>
						<Switch
							label="登录时启动"
							checked={loginStatus === 1 || loginStatus === 2}
							disabled={switchingLogin || loginStatus === null}
							onChange={async () => {
								setSwitchingLogin(true);
								try {
									await invoke(
										loginStatus === 1 || loginStatus === 2
											? "unregister_login_item"
											: "register_login_item",
									);
									await onContext();
								} catch (error) {
									setDialogError(userError(error));
									onMessage(userError(error));
								} finally {
									setSwitchingLogin(false);
								}
							}}
						/>
					</div>
				</LoadBoundary>
			</div>
			<h2>录制</h2>
			<div className="panel-card setting-panel">
				<SourcePreparation
					state={state}
					privacy={privacy}
					error={loadErrors.privacy || loadErrors.state}
					onRefresh={async () => {
						await Promise.all([onContext(), onStatus()]);
					}}
				/>
			</div>
			<RecordingScheduleSettings
				state={state}
				refresh={onStatus}
				error={loadErrors.state}
			/>
			<h2>识别与整理</h2>
			<div className="panel-card setting-panel">
				<ModelPreparation
					models={models}
					error={loadErrors.models}
					onRefresh={onContext}
				/>
				<LoadBoundary
					ready={queuesReady}
					error={loadErrors.queues}
					retry={() => void onContext()}
					label="云端配置"
					rows={2}
				>
					{(["llm", "asr"] as const).map((provider) => (
						<CloudServiceSettings
							key={provider}
							provider={provider}
							configured={
								queues.find((queue) => queue.provider === provider)?.configured
							}
							refresh={onContext}
						/>
					))}
				</LoadBoundary>
			</div>
			<h2>存储与数据</h2>
			<div className="panel-card setting-panel">
				<AudioStorageSettings
					state={state}
					error={loadErrors.state}
					refresh={onStatus}
				/>
				<div className="setting-row">
					<div>
						<strong>导出记录</strong>
						<p>导出 {dateTitle(date)} 的文字和选定音频</p>
					</div>
					<Button
						type="button"
						tone="quiet"
						onClick={() => setExportOpen(true)}
					>
						导出
					</Button>
				</div>
				<div className="setting-row">
					<div>
						<strong>清理记录</strong>
						<p>默认清除原始音频</p>
					</div>
					<Button
						type="button"
						tone="quiet"
						onClick={() => {
							setCleanupMode("raw_audio");
							setCleanupPreview(null);
							setDialogError("");
							setCleanupOpen(true);
						}}
					>
						清理
					</Button>
				</div>
			</div>
			{(cleanupOpen || exportOpen) && (
				<Modal
					title={cleanupOpen ? "清理记录" : "导出记录"}
					busy={working}
					feedback={dialogError}
					onClose={closeDataDialog}
				>
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
								<Button
									type="button"
									tone="quiet"
									ref={previewBack}
									disabled={working}
									onClick={() => setCleanupPreview(null)}
								>
									返回
								</Button>
								<Button
									type="button"
									tone="danger"
									disabled={working}
									onClick={() => void cleanup()}
								>
									{cleanupPreview.mode === "all_data"
										? "继续清除全部数据"
										: "清除原始音频"}
								</Button>
							</div>
						</>
					) : (
						<>
							<label>
								范围
								<select
									disabled={working || previewing}
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
									{new Date(selection.item.started_ms).toLocaleString("zh-CN")}{" "}
									— {new Date(selection.item.ended_ms).toLocaleString("zh-CN")}
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
											disabled={working || previewing}
											type="datetime-local"
											value={from}
											onChange={(event) => setFrom(event.target.value)}
										/>
									</label>
									<label>
										结束
										<input
											disabled={working || previewing}
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
										disabled={working || previewing}
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
											className={cleanupMode === "raw_audio" ? "selected" : ""}
										>
											<input
												disabled={working || previewing}
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
												disabled={working || previewing}
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
										<Button
											type="button"
											tone="quiet"
											disabled={working}
											onClick={closeDataDialog}
										>
											取消
										</Button>
										<Button
											type="button"
											tone="primary"
											disabled={working || previewing}
											onClick={() => void preview()}
										>
											{previewing ? "正在检查…" : "查看影响"}
										</Button>
									</div>
								</>
							) : (
								<>
									<label>
										保存位置
										<div className="export-picker">
											<input
												disabled={working || previewing}
												value={exportPath}
												readOnly
												placeholder="选择保存位置"
											/>
											<Button
												type="button"
												tone="quiet"
												onClick={() => void chooseExportPath()}
											>
												选择…
											</Button>
										</div>
									</label>
									<label className="check-line">
										<input
											disabled={working || previewing}
											type="checkbox"
											checked={exportAudio}
											onChange={(event) => setExportAudio(event.target.checked)}
										/>
										包含原始音频
									</label>
									<label className="check-line">
										<input
											disabled={working || previewing}
											type="checkbox"
											checked={exportScreenshots}
											onChange={(event) =>
												setExportScreenshots(event.target.checked)
											}
										/>
										包含截图
									</label>
									<div className="modal-actions">
										<Button
											type="button"
											tone="quiet"
											disabled={working}
											onClick={() => setExportOpen(false)}
										>
											取消
										</Button>
										<Button
											type="button"
											tone="primary"
											disabled={working || !exportPath.trim()}
											onClick={() => void exportHistory()}
										>
											导出
										</Button>
									</div>
								</>
							)}
						</>
					)}
				</Modal>
			)}
		</section>
	);
}
