import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { AgentAccess } from "./AgentAccess";
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
import { Button, Field, Modal } from "./Controls";
import { DetailDrawer } from "./DetailDrawer";
import { type EntrySnapshot, getEntrySnapshot } from "./entry-api";
import { PeopleView } from "./PeopleView";
import { TimelineView } from "./TimelineView";
import { monthBoundaries } from "./timeline-view";
import { QueueView, SettingsView } from "./WorkspaceViews";
import "./App.css";
import { OnboardingView } from "./OnboardingView";
import type { ModelState, PrivacyStatus } from "./setup";

type Page =
	| "timeline"
	| "queue"
	| "people"
	| "settings"
	| "agents"
	| "onboarding";
type EventForm = { mode: "create" | "edit"; item?: TimelineItem };
type EventTool = { item: TimelineItem; mode: "split" | "merge" | "delete" };
type DataAction = { kind: "export" | "cleanup"; item: TimelineItem };

function localInput(timestamp: number): string {
	const d = new Date(timestamp);
	return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}T${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function errorText(error: unknown): string {
	return userError(error);
}

function App() {
	const [page, setPage] = useState<Page>("timeline");
	const [setupCompleted, setSetupCompleted] = useState<boolean | null>(null);
	const [setupError, setSetupError] = useState("");
	const setupLoading = useRef(false);
	const loadSetup = useCallback(async (recover = false) => {
		if (setupLoading.current) return;
		setupLoading.current = true;
		setSetupError("");
		try {
			const result = await invoke<{ completed: boolean }>("onboarding_status", {
				recover,
			});
			setSetupCompleted(result.completed);
			if (!result.completed) setPage("onboarding");
		} catch (e) {
			setSetupError(errorText(e));
		} finally {
			setupLoading.current = false;
		}
	}, []);
	useEffect(() => {
		void loadSetup();
	}, [loadSetup]);
	const [date, setDate] = useState(() => new Date());
	const [items, setItems] = useState<TimelineItem[]>([]);
	const [recordedDays, setRecordedDays] = useState<number[] | null>(null);
	const [gaps, setGaps] = useState<TimeGap[]>([]);
	const [dayMoments, setDayMoments] = useState<Moment[]>([]);
	const [loading, setLoading] = useState(true);
	const [timelineReady, setTimelineReady] = useState(false);
	const [timelineError, setTimelineError] = useState("");
	const [statusError, setStatusError] = useState("");
	const [snapshot, setSnapshot] = useState<EntrySnapshot | null>(null);
	const state = snapshot?.state ?? null;
	const [privacy, setPrivacy] = useState<PrivacyStatus | null>(null);
	const [loginStatus, setLoginStatus] = useState<number | null>(null);
	const [queues, setQueues] = useState<CloudQueue[]>([]);
	const [tasks, setTasks] = useState<CloudTask[]>([]);
	const [reviews, setReviews] = useState<ReviewItem[]>([]);
	const [people, setPeople] = useState<Person[]>([]);
	const [contextReady, setContextReady] = useState({
		queues: false,
		people: false,
	});
	const [contextErrors, setContextErrors] = useState({
		queues: "",
		people: "",
		models: "",
		privacy: "",
		login: "",
	});
	const [tasksReady, setTasksReady] = useState(false);
	const [reviewsReady, setReviewsReady] = useState(false);
	const [tasksError, setTasksError] = useState("");
	const [reviewsError, setReviewsError] = useState("");
	const tasksRequest = useRef(0);
	const [models, setModels] = useState<ModelState | null>(null);
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
	const [dialogError, setDialogError] = useState("");
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
		setStatusError("");
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
			setStatusError(errorText(cause));
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
		setTimelineReady(false);
		setTimelineError("");
		setDate(next);
	}, []);

	const readTimeline = useCallback(
		async (showLoading = false) => {
			// An old focus/poll callback must not claim a newer navigation request.
			if (dayBounds(date)[0] !== selectedDay.current) return;
			const request = ++timelineRequest.current;
			setTimelineError("");
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
					setTimelineReady(true);
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
				if (request === timelineRequest.current)
					setTimelineError(errorText(cause));
			} finally {
				if (request === timelineRequest.current) setLoading(false);
			}
		},
		[date, selectDate],
	);

	const timelineInFlight = useRef<{
		day: number;
		dirty: boolean;
		promise: Promise<void>;
	} | null>(null);
	const refreshTimeline = useCallback(
		(showLoading = false, invalidate = false): Promise<void> => {
			const day = dayBounds(date)[0];
			if (day !== selectedDay.current) return Promise.resolve();
			// Mutations invalidate the response immediately, then request one fresh
			// read after the in-flight snapshot. Focus/poll callers simply join it.
			if (invalidate) ++timelineRequest.current;
			const active = timelineInFlight.current;
			if (active?.day === day) {
				active.dirty ||= invalidate;
				return active.promise;
			}
			const flight = { day, dirty: false, promise: Promise.resolve() };
			timelineInFlight.current = flight;
			flight.promise = (async () => {
				try {
					do {
						flight.dirty = false;
						await readTimeline(showLoading);
					} while (flight.dirty && selectedDay.current === day);
				} finally {
					if (timelineInFlight.current === flight)
						timelineInFlight.current = null;
				}
			})();
			return flight.promise;
		},
		[date, readTimeline],
	);

	const contextInFlight = useRef<Promise<void> | null>(null);
	const refreshContext = useCallback(() => {
		if (contextInFlight.current) return contextInFlight.current;
		const request = (async () => {
			setContextErrors({
				queues: "",
				people: "",
				models: "",
				privacy: "",
				login: "",
			});
			const failed = (key: keyof typeof contextErrors, cause: unknown) =>
				setContextErrors((current) => ({
					...current,
					[key]: errorText(cause),
				}));
			await Promise.all([
				core<{ type: "cloud_status"; id: string; queues: CloudQueue[] }>({
					type: "cloud_status",
				})
					.then((result) => {
						setQueues(result.queues);
						setContextReady((current) => ({ ...current, queues: true }));
					})
					.catch((cause) => failed("queues", cause)),
				core<{ type: "people"; id: string; people: Person[] }>({
					type: "people",
				})
					.then((result) => {
						setPeople(result.people);
						setContextReady((current) => ({ ...current, people: true }));
					})
					.catch((cause) => failed("people", cause)),
				core<{
					type: "models";
					id: string;
					state: ModelState;
				}>({ type: "models" })
					.then((result) => setModels(result.state))
					.catch((cause) => failed("models", cause)),
				invoke<PrivacyStatus>("privacy_status")
					.then(setPrivacy)
					.catch((cause) => failed("privacy", cause)),
				invoke<number>("login_item_status")
					.then(setLoginStatus)
					.catch((cause) => failed("login", cause)),
			]);
		})();
		contextInFlight.current = request;
		void request.finally(() => {
			if (contextInFlight.current === request) contextInFlight.current = null;
		});
		return request;
	}, []);

	const refreshTasks = useCallback(async () => {
		const token = ++tasksRequest.current;
		setTasksError("");
		setReviewsError("");
		await Promise.all([
			Promise.all(
				["llm", "asr"].map((provider) =>
					core<{ type: "cloud_tasks"; id: string; tasks: CloudTask[] }>({
						type: "cloud_tasks",
						provider,
						limit: 50,
					}),
				),
			)
				.then((results) => {
					if (token !== tasksRequest.current) return;
					setTasks(
						results
							.flatMap((result) => result.tasks)
							.sort((a, b) => {
								const rank = (state: string) =>
									state === "failed"
										? 0
										: ["pending", "processing", "submitted"].includes(state)
											? 1
											: 2;
								return (
									rank(a.state) - rank(b.state) || b.started_ms - a.started_ms
								);
							}),
					);
					setTasksReady(true);
				})
				.catch((cause) => {
					if (token === tasksRequest.current) setTasksError(errorText(cause));
				}),
			core<{ type: "review_items"; id: string; items: ReviewItem[] }>({
				type: "review_items",
			})
				.then((result) => {
					if (token === tasksRequest.current) {
						setReviews(result.items);
						setReviewsReady(true);
					}
				})
				.catch((cause) => {
					if (token === tasksRequest.current) setReviewsError(errorText(cause));
				}),
		]);
	}, []);

	useEffect(() => {
		if (!isTauri()) return;
		const subscription = listen("hear-open-settings", () =>
			setPage(setupCompleted === false ? "onboarding" : "settings"),
		);
		return () => {
			void subscription.then((unlisten) => unlisten());
		};
	}, [setupCompleted]);
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
		void refreshTimeline(false, true);
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
				void refreshTimeline(false, true);
			}),
			listen<{ anchor_ms: number }>("hear-open-comment", (event) => {
				setCommentAnchor(event.payload.anchor_ms);
				setCommentDraft("");
				setDialogError("");
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
		setDialogError("");
		setBusy(true);
		try {
			await core(request);
			message(success);
			await Promise.all([
				refreshStatus(),
				refreshTimeline(false, true),
				refreshContext(),
			]);
			return true;
		} catch (cause) {
			if (isEventStale(cause)) {
				setForm(null);
				setTool(null);
				setSelection(null);
				await refreshTimeline(false, true);
			}
			setDialogError(errorText(cause));
			message(errorText(cause));
			return false;
		} finally {
			setBusy(false);
		}
	};

	const openForm = (next: EventForm) => {
		const [day] = dayBounds(date);
		setDialogError("");
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
			setDialogError("请填写标题和有效的时间范围");
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
		setDialogError("");
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
		setDialogError("");
		setBusy(true);
		try {
			await core(
				kind === "screenshot"
					? { type: "capture_screenshot" }
					: { type: "add_moment", kind, anchor_ms: anchor, text: text ?? null },
			);
			setMomentVersion((value) => value + 1);
			await refreshTimeline(false, true);
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
			setDialogError(errorText(cause));
			message(errorText(cause));
		} finally {
			setBusy(false);
		}
	};

	const captureControl = useCaptureControl({
		snapshot,
		onRefresh: refreshStatus,
		onSettings: () =>
			setPage(setupCompleted === false ? "onboarding" : "settings"),
	});
	const refreshSetup = async () => {
		// Finish any older poll, then fetch observations begun after mutations.
		await refreshContext();
		await Promise.all([refreshStatus(), refreshContext()]);
	};
	if (setupCompleted === null)
		return (
			<main className="onboarding-loading">
				<h1>准备 Hear</h1>
				<p role="status">{setupError || "正在读取设置…"}</p>
				{setupError && (
					<>
						<button type="button" onClick={() => void loadSetup()}>
							重试
						</button>
						{setupError.includes("首次设置记录损坏") && (
							<>
								<button type="button" onClick={() => void loadSetup(true)}>
									恢复设置记录
								</button>
								<p>恢复只重建首次页面记录，不会重置录音、来源或云配置。</p>
							</>
						)}
					</>
				)}
			</main>
		);
	if (page === "onboarding")
		return (
			<OnboardingView
				state={state}
				privacy={privacy}
				models={models}
				queues={queues}
				queuesReady={contextReady.queues}
				errors={{
					models: contextErrors.models,
					privacy: contextErrors.privacy,
					queues: contextErrors.queues,
					state: statusError || snapshot?.error || "",
				}}
				onRefresh={refreshSetup}
				onComplete={() => {
					setSetupCompleted(true);
					setPage("timeline");
				}}
				onLater={() => setPage("timeline")}
			/>
		);
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
							["agents", "Agent 接入", "M8 8 4 12l4 4M16 8l4 4-4 4M14 4l-4 16"],
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
			<main className="main-area" tabIndex={-1} data-focus-fallback>
				{setupCompleted === false && (
					<div className="connection-banner">
						首次准备尚未完成
						<button type="button" onClick={() => setPage("onboarding")}>
							继续设置
						</button>
					</div>
				)}
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
							<CaptureStatus
								snapshot={snapshot}
								control={captureControl}
								loadError={statusError}
								onRetry={() => void refreshStatus()}
							/>
						}
						date={date}
						recordedDays={recordedDays}
						recordingState={snapshot?.error ? null : state}
						items={items}
						gaps={gaps}
						moments={dayMoments}
						loading={loading && !timelineReady}
						ready={timelineReady}
						loadError={timelineError}
						onRetry={() => void refreshTimeline(false, true)}
						hideCoarse
						onDate={selectDate}
						onSelect={(item, initialAt) => setSelection({ item, initialAt })}
						onCreate={() => openForm({ mode: "create" })}
					/>
				)}
				{page === "queue" && (
					<QueueView
						queuesReady={contextReady.queues}
						queuesError={contextErrors.queues}
						tasksReady={tasksReady}
						tasksError={tasksError}
						reviewsReady={reviewsReady}
						reviewsError={reviewsError}
						queues={queues}
						tasks={tasks}
						reviews={reviews}
						busy={busy}
						onRefresh={async () => {
							await Promise.all([refreshTasks(), refreshContext()]);
						}}
						onMutate={mutate}
					/>
				)}
				{page === "people" && (
					<PeopleView
						people={people}
						ready={contextReady.people}
						loadError={contextErrors.people}
						onRetry={() => void refreshContext()}
						busy={busy}
						onMutate={mutate}
					/>
				)}
				{page === "agents" && <AgentAccess />}
				{page === "settings" && (
					<SettingsView
						queuesReady={contextReady.queues}
						loadErrors={{
							...contextErrors,
							state: snapshot?.error || statusError,
						}}
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
						onTimeline={() => refreshTimeline(false, true)}
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
					peopleReady={contextReady.people}
					peopleError={contextErrors.people}
					onPeopleRetry={() => void refreshContext()}
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
						void refreshTimeline(false, true);
						void refreshContext();
					}}
					onMessage={message}
					refreshToken={momentVersion}
				/>
			)}
			{commentOpen && (
				<Modal
					title="评论"
					busy={busy}
					feedback={dialogError}
					onClose={() => setCommentOpen(false)}
				>
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
						<Button
							type="button"
							tone="quiet"
							disabled={busy}
							onClick={() => setCommentOpen(false)}
						>
							取消
						</Button>
						<Button
							type="button"
							tone="primary"
							disabled={busy || !commentDraft.trim()}
							onClick={() =>
								void addMoment("comment", commentAnchor, commentDraft.trim())
							}
						>
							发送
						</Button>
					</div>
				</Modal>
			)}
			{form && (
				<Modal
					title={form.mode === "create" ? "新建事件" : "编辑事件"}
					busy={busy}
					feedback={dialogError}
					onClose={() => setForm(null)}
				>
					<Field label="名称">
						{(attributes) => (
							<input
								{...attributes}
								disabled={busy}
								value={title}
								onChange={(event) => setTitle(event.target.value)}
								placeholder="例如，产品周会"
							/>
						)}
					</Field>
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
						<Button
							type="button"
							tone="quiet"
							disabled={busy}
							onClick={() => setForm(null)}
						>
							取消
						</Button>
						<Button
							type="button"
							tone="primary"
							disabled={busy}
							onClick={() => void saveEvent()}
						>
							保存
						</Button>
					</div>
				</Modal>
			)}
			{tool && (
				<Modal
					title="管理事件"
					busy={busy}
					feedback={dialogError}
					onClose={() => setTool(null)}
				>
					<div className="tool-tabs">
						{(["split", "merge", "delete"] as const).map((mode) => (
							<button
								key={mode}
								type="button"
								className={tool.mode === mode ? "active" : ""}
								disabled={busy}
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
						<Button
							type="button"
							tone="quiet"
							disabled={busy}
							onClick={() => setTool(null)}
						>
							返回
						</Button>
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
				</Modal>
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
