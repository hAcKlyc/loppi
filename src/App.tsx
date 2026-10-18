import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import {
	type CloudQueue,
	type CloudTask,
	core,
	dayBounds,
	isEventStale,
	type Moment,
	type Person,
	type ReviewItem,
	type ServiceState,
	type Sources,
	type TimeGap,
	type TimelineItem,
	time,
	userError,
} from "./api";
import appIcon from "./assets/rabbit/app-icon-square.png";
import { CaptureStatus, useCaptureControl } from "./CaptureStatus";
import { DetailDrawer } from "./DetailDrawer";
import { type EntrySnapshot, getEntrySnapshot } from "./entry-api";
import { PeopleView } from "./PeopleView";
import { TimelineView } from "./TimelineView";
import { monthBoundaries } from "./timeline-view";
import { QueueView, SettingsView } from "./WorkspaceViews";
import "./App.css";

type Page = "timeline" | "queue" | "people" | "settings";
type EventForm = { mode: "create" | "edit"; item?: TimelineItem };
type EventTool = { item: TimelineItem; mode: "split" | "merge" | "delete" };
type DataAction = { kind: "export" | "cleanup"; item: TimelineItem };
type PrivacyStatus = {
	microphone: number;
	screen_and_system_audio: number;
	tap_required: boolean;
	tap_ready: boolean;
};

function localInput(timestamp: number): string {
	const d = new Date(timestamp);
	return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}T${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function errorText(error: unknown): string {
	return userError(error);
}

function App() {
	const [page, setPage] = useState<Page>("timeline");
	const [date, setDate] = useState(() => new Date());
	const [hideCoarse, setHideCoarse] = useState(true);
	const [items, setItems] = useState<TimelineItem[]>([]);
	const [recordedDays, setRecordedDays] = useState<number[] | null>(null);
	const [gaps, setGaps] = useState<TimeGap[]>([]);
	const [dayMoments, setDayMoments] = useState<Moment[]>([]);
	const [loading, setLoading] = useState(true);
	const [snapshot, setSnapshot] = useState<EntrySnapshot | null>(null);
	const state = snapshot?.state ?? null;
	const [privacy, setPrivacy] = useState<PrivacyStatus | null>(null);
	const [loginStatus, setLoginStatus] = useState<number | null>(null);
	const [queues, setQueues] = useState<CloudQueue[]>([]);
	const [tasks, setTasks] = useState<CloudTask[]>([]);
	const [reviews, setReviews] = useState<ReviewItem[]>([]);
	const [people, setPeople] = useState<Person[]>([]);
	const [models, setModels] = useState<{
		phase: string;
		speaker_phase: string;
		progress_percent: number;
	} | null>(null);
	const [selection, setSelection] = useState<{
		item: TimelineItem;
		initialAt?: number;
	} | null>(null);
	const selected = selection?.item ?? null;
	const [form, setForm] = useState<EventForm | null>(null);
	const [tool, setTool] = useState<EventTool | null>(null);
	const [splitAt, setSplitAt] = useState("");
	const [leftTitle, setLeftTitle] = useState("");
	const [rightTitle, setRightTitle] = useState("");
	const [mergeTarget, setMergeTarget] = useState("");
	const [mergeTitle, setMergeTitle] = useState("");
	const [title, setTitle] = useState("");
	const [from, setFrom] = useState("");
	const [to, setTo] = useState("");
	const [sources, setSources] = useState<Sources>("both");
	const [toast, setToast] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [commentOpen, setCommentOpen] = useState(false);
	const [dataAction, setDataAction] = useState<DataAction | null>(null);
	const [commentDraft, setCommentDraft] = useState("");
	const [commentAnchor, setCommentAnchor] = useState(0);
	const [momentVersion, setMomentVersion] = useState(0);
	const timelineRequest = useRef(0);
	const selectedDay = useRef(dayBounds(date)[0]);
	const initialDatePending = useRef(true);
	const manualSelection = useRef<string | null>(null);

	const message = (text: string) => {
		setToast(text);
		window.setTimeout(
			() => setToast((current) => (current === text ? null : current)),
			4500,
		);
	};

	const acceptSnapshot = useCallback((next: EntrySnapshot) => {
		setSnapshot((current) =>
			!current || next.sequence >= current.sequence ? next : current,
		);
	}, []);
	const refreshStatus = useCallback(async () => {
		try {
			if (isTauri()) {
				acceptSnapshot(await getEntrySnapshot(true));
			} else {
				await core<{
					type: "status";
					id: string;
					state: ServiceState;
				}>({ type: "status" });
			}
			setError(null);
		} catch (cause) {
			setError(errorText(cause));
			// A local bridge failure is not a new host observation. Preserve the
			// shared snapshot so main and entry cannot invent different states.
		}
	}, [acceptSnapshot]);

	const selectDate = useCallback((next: Date) => {
		initialDatePending.current = false;
		const nextDay = dayBounds(next)[0];
		if (nextDay === selectedDay.current) return;
		const previous = new Date(selectedDay.current);
		selectedDay.current = nextDay;
		++timelineRequest.current;
		// Month navigation survives a day change; it is not day-content state.
		if (
			previous.getFullYear() !== next.getFullYear() ||
			previous.getMonth() !== next.getMonth()
		) {
			setRecordedDays(null);
		}
		setItems([]);
		setGaps([]);
		setDayMoments([]);
		setSelection(null);
		manualSelection.current = null;
		setLoading(true);
		setDate(next);
	}, []);

	const refreshTimeline = useCallback(
		async (showLoading = false) => {
			// An old focus/poll callback must not claim a newer navigation request.
			if (dayBounds(date)[0] !== selectedDay.current) return;
			const request = ++timelineRequest.current;
			if (showLoading) setLoading(true);
			try {
				const [from_ms, to_ms] = dayBounds(date);
				const calendar = core<{
					type: "calendar_days";
					id: string;
					days: number[];
				}>({ type: "calendar_days", boundaries: monthBoundaries(date) }).catch(
					() => null,
				);
				const response = await core<{
					type: "timeline";
					id: string;
					items: TimelineItem[];
					gaps: TimeGap[];
				}>({ type: "timeline", from_ms, to_ms });
				const available = await calendar;
				const all: Moment[] = [];
				while (true) {
					const page = await core<{
						type: "moments";
						id: string;
						moments: Moment[];
					}>({
						type: "moments",
						from_ms,
						to_ms,
						offset: all.length,
						limit: 200,
					});
					all.push(...page.moments);
					if (page.moments.length < 200) break;
				}
				if (request === timelineRequest.current) {
					if (available) setRecordedDays(available.days);
					if (initialDatePending.current) {
						initialDatePending.current = false;
						const latest = available?.days[available.days.length - 1];
						if (
							response.items.length === 0 &&
							latest !== undefined &&
							latest !== from_ms
						) {
							selectDate(new Date(latest));
							return;
						}
					}
					setItems(response.items);
					setGaps(response.gaps);
					setDayMoments(all);
					setSelection((currentSelection) => {
						const current = currentSelection?.item;
						const preserveEntry = (item: TimelineItem | null) =>
							item ? { item, initialAt: currentSelection?.initialAt } : null;
						if (manualSelection.current) {
							const preferred = response.items.find(
								(next) => next.manual_range_id === manualSelection.current,
							);
							manualSelection.current = null;
							if (preferred) return { item: preferred };
						}
						if (!current) return null;
						if (current.id) {
							return preserveEntry(
								response.items.find((next) => next.id === current.id) ?? null,
							);
						}
						if (current.manual_range_id) {
							return preserveEntry(
								response.items.find(
									(next) =>
										next.manual_range_id === current.manual_range_id &&
										next.started_ms <=
											(current.started_ms + current.ended_ms) / 2 &&
										next.ended_ms > (current.started_ms + current.ended_ms) / 2,
								) ?? null,
							);
						}
						const remaining = response.items.find(
							(next) =>
								!next.id &&
								next.sources === current.sources &&
								next.started_ms <= current.started_ms &&
								next.ended_ms >= current.ended_ms,
						);
						if (remaining) return preserveEntry(remaining);
						const midpoint = (current.started_ms + current.ended_ms) / 2;
						return preserveEntry(
							response.items.find(
								(next) =>
									next.started_ms <= midpoint &&
									next.ended_ms > midpoint &&
									next.sources === current.sources,
							) ?? null,
						);
					});
					setError(null);
				}
			} catch (cause) {
				if (request === timelineRequest.current) {
					if (showLoading) {
						setItems([]);
						setGaps([]);
						setDayMoments([]);
					}
					setError(errorText(cause));
				}
			} finally {
				if (request === timelineRequest.current) setLoading(false);
			}
		},
		[date, selectDate],
	);

	const contextInFlight = useRef<Promise<void> | null>(null);
	const refreshContext = useCallback(() => {
		if (contextInFlight.current) return contextInFlight.current;
		const request = (async () => {
			const results = await Promise.allSettled([
				core<{ type: "cloud_status"; id: string; queues: CloudQueue[] }>({
					type: "cloud_status",
				}),
				core<{ type: "people"; id: string; people: Person[] }>({
					type: "people",
				}),
				core<{
					type: "models";
					id: string;
					state: {
						phase: string;
						speaker_phase: string;
						progress_percent: number;
					};
				}>({ type: "models" }),
				invoke<PrivacyStatus>("privacy_status"),
				invoke<number>("login_item_status"),
			]);
			if (results[0].status === "fulfilled") setQueues(results[0].value.queues);
			if (results[1].status === "fulfilled") setPeople(results[1].value.people);
			if (results[2].status === "fulfilled") setModels(results[2].value.state);
			if (results[3].status === "fulfilled") setPrivacy(results[3].value);
			if (results[4].status === "fulfilled") setLoginStatus(results[4].value);
		})();
		contextInFlight.current = request;
		void request.finally(() => {
			if (contextInFlight.current === request) contextInFlight.current = null;
		});
		return request;
	}, []);

	const refreshTasks = useCallback(async () => {
		const results = await Promise.allSettled([
			core<{ type: "cloud_tasks"; id: string; tasks: CloudTask[] }>({
				type: "cloud_tasks",
				provider: "llm",
				limit: 50,
			}),
			core<{ type: "cloud_tasks"; id: string; tasks: CloudTask[] }>({
				type: "cloud_tasks",
				provider: "asr",
				limit: 50,
			}),
			core<{ type: "review_items"; id: string; items: ReviewItem[] }>({
				type: "review_items",
			}),
		]);
		if (
			results[0].status === "fulfilled" &&
			results[1].status === "fulfilled"
		) {
			setTasks(
				[...results[0].value.tasks, ...results[1].value.tasks].sort((a, b) => {
					const rank = (state: string) =>
						state === "failed"
							? 0
							: ["pending", "processing", "submitted"].includes(state)
								? 1
								: 2;
					return rank(a.state) - rank(b.state) || b.started_ms - a.started_ms;
				}),
			);
		}
		if (results[2].status === "fulfilled") setReviews(results[2].value.items);
	}, []);

	useEffect(() => {
		if (!isTauri()) return;
		const subscription = listen("hear-open-settings", () =>
			setPage("settings"),
		);
		return () => {
			void subscription.then((unlisten) => unlisten());
		};
	}, []);
	useEffect(() => {
		if (!isTauri()) return;
		let disposed = false;
		const subscription = listen<EntrySnapshot>(
			"hear-entry-snapshot",
			(event) => {
				if (!disposed) acceptSnapshot(event.payload);
			},
		);
		void subscription
			.then(() => getEntrySnapshot())
			.then((next) => {
				if (!disposed) acceptSnapshot(next);
			})
			.catch((cause) => {
				if (!disposed) setError(errorText(cause));
			});
		return () => {
			disposed = true;
			void subscription.then((unlisten) => unlisten());
		};
	}, [acceptSnapshot]);
	useEffect(() => {
		void invoke("gui_ready");
		void refreshStatus();
		void refreshContext();
		const timer = window.setInterval(() => {
			if (document.visibilityState !== "visible" || !document.hasFocus())
				return;
			void refreshContext();
		}, 5000);
		return () => window.clearInterval(timer);
	}, [refreshStatus, refreshContext]);
	// A pause/resume or source restart invalidates the prior session's live
	// projection, including responses already in flight. Reuse the timeline
	// request sequence; the status snapshot alone cannot re-light an old card.
	const recordingObservation = JSON.stringify([
		snapshot?.error,
		state?.revision,
		state?.capture_state,
		state?.microphone_phase,
		state?.system_phase,
		state?.microphone_recording,
		state?.system_recording,
	]);
	useEffect(() => {
		void recordingObservation;
		setItems((current) =>
			current.map((item) => ({ ...item, recording_sources: [] })),
		);
		void refreshTimeline();
	}, [refreshTimeline, recordingObservation]);
	useEffect(() => {
		if (page === "queue") void refreshTasks();
	}, [page, refreshTasks]);
	useEffect(() => {
		if (page !== "timeline" && page !== "queue") return;
		const timer = window.setInterval(() => {
			if (document.visibilityState !== "visible" || !document.hasFocus())
				return;
			if (page === "timeline") {
				void refreshTimeline();
				setMomentVersion((value) => value + 1);
			} else void refreshTasks();
		}, 15_000);
		return () => window.clearInterval(timer);
	}, [page, refreshTimeline, refreshTasks]);
	useEffect(() => {
		let lastRefresh = 0;
		const refreshForeground = () => {
			if (Date.now() - lastRefresh < 250) return;
			lastRefresh = Date.now();
			void refreshStatus();
			void refreshContext();
			if (page === "timeline") void refreshTimeline();
			if (page === "queue") void refreshTasks();
			setMomentVersion((value) => value + 1);
		};
		const onVisibility = () => {
			if (document.visibilityState === "visible") refreshForeground();
		};
		window.addEventListener("focus", refreshForeground);
		document.addEventListener("visibilitychange", onVisibility);
		return () => {
			window.removeEventListener("focus", refreshForeground);
			document.removeEventListener("visibilitychange", onVisibility);
		};
	}, [page, refreshStatus, refreshContext, refreshTimeline, refreshTasks]);
	useEffect(() => {
		if (!isTauri()) return;
		let disposed = false;
		const unsubscribers: Array<() => void> = [];
		void Promise.all([
			listen("hear-moment-changed", () => {
				setMomentVersion((value) => value + 1);
				void refreshTimeline();
			}),
			listen<{ anchor_ms: number }>("hear-open-comment", (event) => {
				setCommentAnchor(event.payload.anchor_ms);
				setCommentDraft("");
				setCommentOpen(true);
			}),
		]).then((callbacks) => {
			if (disposed)
				callbacks.forEach((callback) => {
					callback();
				});
			else unsubscribers.push(...callbacks);
		});
		return () => {
			disposed = true;
			unsubscribers.forEach((callback) => {
				callback();
			});
		};
	}, [refreshTimeline]);

	const mutate = async (
		request: { type: string; [key: string]: unknown },
		success: string,
	) => {
		setBusy(true);
		try {
			await core(request);
			message(success);
			await Promise.all([refreshStatus(), refreshTimeline(), refreshContext()]);
			return true;
		} catch (cause) {
			if (isEventStale(cause)) {
				setForm(null);
				setTool(null);
				setSelection(null);
				await refreshTimeline();
			}
			message(errorText(cause));
			return false;
		} finally {
			setBusy(false);
		}
	};

	const openForm = (next: EventForm) => {
		const [day] = dayBounds(date);
		setForm(next);
		setTitle(next.item?.title ?? "");
		setFrom(localInput(next.item?.started_ms ?? day + 9 * 3_600_000));
		setTo(localInput(next.item?.ended_ms ?? day + 10 * 3_600_000));
		setSources(next.item?.sources ?? "both");
	};

	const saveEvent = async () => {
		const started_ms =
			form?.item && from === localInput(form.item.started_ms)
				? form.item.started_ms
				: new Date(from).getTime();
		const ended_ms =
			form?.item && to === localInput(form.item.ended_ms)
				? form.item.ended_ms
				: new Date(to).getTime();
		if (
			!title.trim() ||
			!Number.isFinite(started_ms) ||
			ended_ms <= started_ms
		) {
			message("请填写标题和有效的时间范围");
			return;
		}
		const request =
			form?.mode === "edit" && form.item?.id
				? {
						type: "edit_event",
						event_id: form.item.id,
						expected_revision: form.item.revision,
						started_ms,
						ended_ms,
						title: title.trim(),
						sources,
					}
				: {
						type: "create_event",
						started_ms,
						ended_ms,
						title: title.trim(),
						sources,
					};
		if (await mutate(request, "事件已保存")) {
			setSelection(null);
			setForm(null);
		}
	};

	const openTools = (item: TimelineItem) => {
		setTool({ item, mode: "split" });
		setSplitAt(localInput((item.started_ms + item.ended_ms) / 2));
		setLeftTitle(item.title || "前半段");
		setRightTitle(item.title || "后半段");
		setMergeTarget("");
		setMergeTitle(item.title || "合并事件");
	};

	const saveTool = async () => {
		if (!tool?.item.id || tool.item.revision == null) return;
		let request: { type: string; [key: string]: unknown };
		if (tool.mode === "split") {
			const at_ms = new Date(splitAt).getTime();
			if (
				!(at_ms > tool.item.started_ms && at_ms < tool.item.ended_ms) ||
				!leftTitle.trim() ||
				!rightTitle.trim()
			) {
				message("请选择事件内部的分割时刻并填写标题");
				return;
			}
			request = {
				type: "split_event",
				event_id: tool.item.id,
				expected_revision: tool.item.revision,
				at_ms,
				left_title: leftTitle.trim(),
				right_title: rightTitle.trim(),
			};
		} else if (tool.mode === "merge") {
			const target = items.find((item) => item.id === mergeTarget);
			if (!target?.id || target.revision == null || !mergeTitle.trim()) {
				message("请选择要合并的事件");
				return;
			}
			request = {
				type: "merge_events",
				left_id: tool.item.id,
				left_revision: tool.item.revision,
				right_id: target.id,
				right_revision: target.revision,
				title: mergeTitle.trim(),
			};
		} else {
			request = {
				type: "delete_event",
				event_id: tool.item.id,
				expected_revision: tool.item.revision,
			};
		}
		if (
			await mutate(
				request,
				tool.mode === "delete" ? "事件标注已取消" : "事件已更新",
			)
		) {
			setTool(null);
			setSelection(null);
		}
	};

	const addMoment = async (
		kind: "mark" | "comment" | "screenshot",
		anchor: number,
		text?: string,
	) => {
		setBusy(true);
		try {
			await core(
				kind === "screenshot"
					? { type: "capture_screenshot" }
					: { type: "add_moment", kind, anchor_ms: anchor, text: text ?? null },
			);
			setMomentVersion((value) => value + 1);
			await refreshTimeline();
			message(
				kind === "mark"
					? "已标记重点"
					: kind === "screenshot"
						? "截屏已保存"
						: "评论已保存",
			);
			setCommentOpen(false);
			setCommentDraft("");
		} catch (cause) {
			message(errorText(cause));
		} finally {
			setBusy(false);
		}
	};

	const captureControl = useCaptureControl({
		snapshot,
		onRefresh: refreshStatus,
		onSettings: () => setPage("settings"),
	});
	return (
		<div className="app-shell">
			<aside className="sidebar" data-tauri-drag-region>
				<div className="brand" data-tauri-drag-region>
					<img
						className="brand-symbol"
						src={appIcon}
						alt=""
						draggable={false}
						data-tauri-drag-region
					/>
					<strong>hear</strong>
				</div>
				<nav aria-label="主导航" data-tauri-drag-region>
					{(
						[
							[
								"timeline",
								"时间线",
								"M8 4v16M12 6h8M12 12h6M12 18h8M4 6h.01M4 12h.01M4 18h.01",
							],
							["queue", "处理队列", "M20 12a8 8 0 1 1-8-8M12 7v5l3 2M17 3v6h6"],
							[
								"people",
								"人物库",
								"M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0ZM4 21v-2a8 8 0 0 1 16 0v2",
							],
							[
								"settings",
								"设置",
								"M4 7h3m6 0h7M4 17h7m6 0h3M13 7a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM17 17a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
							],
						] as const
					).map(([id, label, icon]) => (
						<button
							key={id}
							type="button"
							className={`nav-button ${page === id ? "active" : ""}`}
							aria-label={label}
							aria-current={page === id ? "page" : undefined}
							title={label}
							onClick={() => {
								setPage(id);
								setDataAction(null);
								setSelection(null);
							}}
						>
							<span className="nav-icon" aria-hidden="true">
								<svg
									aria-hidden="true"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									strokeWidth="1.6"
									strokeLinecap="round"
									strokeLinejoin="round"
								>
									<path d={icon} />
								</svg>
							</span>
							<span>{label}</span>
							{id === "queue" && queues.some((queue) => queue.failed > 0) && (
								<i />
							)}
						</button>
					))}
				</nav>
			</aside>
			<main className="main-area">
				{error && (
					<div className="connection-banner">
						后台暂不可用 · {error}
						<button type="button" onClick={() => void refreshStatus()}>
							重试
						</button>
					</div>
				)}
				{state?.storage_warning && !state.storage_paused && (
					<div className="connection-banner storage-banner">
						音频空间即将不足 · 已用{" "}
						{(state.audio_used_bytes / 1_073_741_824).toFixed(2)} GB
						<button type="button" onClick={() => setPage("settings")}>
							查看
						</button>
					</div>
				)}
				{page === "timeline" && (
					<TimelineView
						captureStatus={
							<CaptureStatus snapshot={snapshot} control={captureControl} />
						}
						date={date}
						recordedDays={recordedDays}
						recordingState={snapshot?.error ? null : state}
						items={items}
						gaps={gaps}
						moments={dayMoments}
						loading={loading}
						hideCoarse={hideCoarse}
						onHideCoarse={setHideCoarse}
						onDate={selectDate}
						onSelect={(item, initialAt) => setSelection({ item, initialAt })}
						onCreate={() => openForm({ mode: "create" })}
					/>
				)}
				{page === "queue" && (
					<QueueView
						queues={queues}
						tasks={tasks}
						reviews={reviews}
						busy={busy}
						onRefresh={refreshTasks}
						onMutate={mutate}
					/>
				)}
				{page === "people" && (
					<PeopleView people={people} busy={busy} onMutate={mutate} />
				)}
				{page === "settings" && (
					<SettingsView
						date={date}
						state={state}
						queues={queues}
						selection={dataAction}
						privacy={privacy}
						loginStatus={loginStatus}
						models={models}
						busy={busy}
						onStatus={refreshStatus}
						onContext={refreshContext}
						onTimeline={refreshTimeline}
						onMutate={mutate}
						onMessage={message}
					/>
				)}
			</main>

			{selected && (
				<DetailDrawer
					key={`${selected.id ?? selected.manual_range_id ?? "range"}-${selected.started_ms}-${selected.sources}`}
					item={selected}
					initialAt={selection?.initialAt}
					people={people}
					onClose={() => setSelection(null)}
					onEdit={(item) => openForm({ mode: "edit", item })}
					onTools={openTools}
					onQueue={() => {
						setSelection(null);
						setPage("queue");
					}}
					onDataAction={(kind, item) => {
						setDataAction({ kind, item });
						setSelection(null);
						setPage("settings");
					}}
					onChanged={(rangeId) => {
						if (rangeId) manualSelection.current = rangeId;
						void refreshTimeline();
						void refreshContext();
					}}
					onMessage={message}
					refreshToken={momentVersion}
				/>
			)}
			{commentOpen && (
				<div className="modal-layer">
					<button
						type="button"
						className="modal-scrim"
						aria-label="关闭评论"
						onClick={() => setCommentOpen(false)}
					/>
					<div className="modal">
						<div className="modal-title">
							<h2>评论</h2>
							<button
								type="button"
								className="icon-button"
								onClick={() => setCommentOpen(false)}
							>
								×
							</button>
						</div>
						<p className="comment-context">
							{new Date(commentAnchor).toLocaleString("zh-CN")}
						</p>
						<textarea
							aria-label="评论内容"
							className="comment-input"
							value={commentDraft}
							onChange={(event) => setCommentDraft(event.target.value)}
							placeholder="写下这一刻的想法…"
						/>
						<div className="modal-actions">
							<button
								type="button"
								className="quiet-button"
								onClick={() => setCommentOpen(false)}
							>
								取消
							</button>
							<button
								type="button"
								className="primary-button"
								disabled={busy || !commentDraft.trim()}
								onClick={() =>
									void addMoment("comment", commentAnchor, commentDraft.trim())
								}
							>
								发送
							</button>
						</div>
					</div>
				</div>
			)}
			{form && (
				<div className="modal-layer">
					<button
						type="button"
						className="modal-scrim"
						aria-label="关闭"
						onClick={() => setForm(null)}
					/>
					<div className="modal">
						<div className="modal-title">
							<h2>{form.mode === "create" ? "新建事件" : "编辑事件"}</h2>
							<button
								type="button"
								className="icon-button"
								onClick={() => setForm(null)}
							>
								×
							</button>
						</div>
						<label>
							名称
							<input
								value={title}
								onChange={(event) => setTitle(event.target.value)}
								placeholder="例如，产品周会"
							/>
						</label>
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
						<label>
							声音来源
							<select
								value={sources}
								onChange={(event) => setSources(event.target.value as Sources)}
							>
								<option value="both">双路声音</option>
								<option value="microphone">麦克风</option>
								<option value="system">电脑声音</option>
							</select>
						</label>
						<div className="modal-actions">
							<button
								type="button"
								className="quiet-button"
								onClick={() => setForm(null)}
							>
								取消
							</button>
							<button
								type="button"
								className="primary-button"
								disabled={busy}
								onClick={() => void saveEvent()}
							>
								保存
							</button>
						</div>
					</div>
				</div>
			)}
			{tool && (
				<div className="modal-layer">
					<button
						type="button"
						className="modal-scrim"
						aria-label="关闭事件操作"
						onClick={() => setTool(null)}
					/>
					<div className="modal">
						<div className="modal-title">
							<h2>管理事件</h2>
							<button
								type="button"
								className="icon-button"
								onClick={() => setTool(null)}
							>
								×
							</button>
						</div>
						<div className="tool-tabs">
							{(["split", "merge", "delete"] as const).map((mode) => (
								<button
									key={mode}
									type="button"
									className={tool.mode === mode ? "active" : ""}
									onClick={() => setTool({ ...tool, mode })}
								>
									{mode === "split"
										? "拆分"
										: mode === "merge"
											? "合并"
											: "取消标注"}
								</button>
							))}
						</div>
						{tool.mode === "split" && (
							<>
								<label>
									分割时刻
									<input
										type="datetime-local"
										value={splitAt}
										onChange={(event) => setSplitAt(event.target.value)}
									/>
								</label>
								<div className="field-grid">
									<label>
										前半段名称
										<input
											value={leftTitle}
											onChange={(event) => setLeftTitle(event.target.value)}
										/>
									</label>
									<label>
										后半段名称
										<input
											value={rightTitle}
											onChange={(event) => setRightTitle(event.target.value)}
										/>
									</label>
								</div>
							</>
						)}
						{tool.mode === "merge" && (
							<>
								<label>
									合并到
									<select
										value={mergeTarget}
										onChange={(event) => setMergeTarget(event.target.value)}
									>
										<option value="">选择事件</option>
										{items
											.filter((item) => item.id && item.id !== tool.item.id)
											.map((item) => (
												<option key={item.id} value={item.id || ""}>
													{time(item.started_ms)} · {item.title}
												</option>
											))}
									</select>
								</label>
								<label>
									合并后名称
									<input
										value={mergeTitle}
										onChange={(event) => setMergeTitle(event.target.value)}
									/>
								</label>
							</>
						)}
						{tool.mode === "delete" && (
							<p>
								这会取消当前事件的命名和边界。声音、转录、截图与笔记会留在原来的时间位置。
							</p>
						)}
						<div className="modal-actions">
							<button
								type="button"
								className="quiet-button"
								onClick={() => setTool(null)}
							>
								返回
							</button>
							<button
								type="button"
								className={
									tool.mode === "delete" ? "danger-button" : "primary-button"
								}
								disabled={busy}
								onClick={() => void saveTool()}
							>
								{tool.mode === "delete" ? "取消标注" : "保存"}
							</button>
						</div>
					</div>
				</div>
			)}
			{toast && (
				<div className="toast" role="status">
					{toast}
				</div>
			)}
		</div>
	);
}

export default App;
