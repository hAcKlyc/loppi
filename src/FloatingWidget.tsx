import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { userError } from "./api";
import {
	commentCanSubmit,
	commentEnterSubmits,
	type EntryGeometry,
	type EntryKind,
	EntryRequests,
	type EntrySnapshot,
	type EntryView,
	entryAppearance,
	getEntrySnapshot,
	topRabbitMotion,
} from "./entry-api";
import { Rabbit } from "./Rabbit";
import "./App.css";
import "./entry.css";

const initial: EntrySnapshot = {
	sequence: 0,
	state: null,
	error: null,
	mode: "top",
	appearance: "waiting",
	detail: null,
	control: null,
	notice: null,
};
function Glyph({
	kind,
}: {
	kind: "screenshot" | "mark" | "comment" | "pause" | "resume" | "start";
}) {
	return (
		<svg
			width="22"
			height="22"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.7"
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
		>
			{kind === "screenshot" ? (
				<>
					<path d="M8 5 9.5 3h5L16 5h4v15H4V5Z" />
					<circle cx="12" cy="12" r="4" />
				</>
			) : kind === "mark" ? (
				<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z" />
			) : kind === "comment" ? (
				<>
					<path d="M4 4h16v13H9l-5 4Z" />
					<path d="M8 8h8M8 12h5" />
				</>
			) : kind === "pause" ? (
				<path d="M8 5v14M16 5v14" />
			) : (
				<path d="m8 4 11 8-11 8Z" />
			)}
		</svg>
	);
}
export function FloatingWidget() {
	const [snapshot, setSnapshot] = useState(initial);
	const snapshotRef = useRef(snapshot);
	snapshotRef.current = snapshot;
	const [view, setView] = useState<EntryView>("collapsed");
	const [panelReady, setPanelReady] = useState(false);
	const viewRef = useRef(view);
	viewRef.current = view;
	const [geometry, setGeometry] = useState<EntryGeometry>({
		width: 44,
		height: 32,
		band: 32,
		rabbit_x: 8,
		rabbit_y: 2,
		top_band: 32,
		top_shift: 296,
	});
	const [draft, setDraft] = useState("");
	const draftRef = useRef("");
	draftRef.current = draft;
	const anchor = useRef<number | null>(null);
	const textarea = useRef<HTMLTextAreaElement>(null);
	const composing = useRef(false);
	const requests = useRef(new EntryRequests());
	const active = useRef(new Set<EntryKind>());
	const [busy, setBusy] = useState<Partial<Record<EntryKind, boolean>>>({});
	const [uncertain, setUncertain] = useState<
		Partial<Record<EntryKind, boolean>>
	>({});
	const [feedback, setFeedback] = useState("");
	const feedbackTimer = useRef<number | undefined>(undefined);
	const [action, setAction] = useState<{
		name: "curious" | "nod";
		id: number;
	} | null>(null);
	const [nativeVisible, setNativeVisible] = useState(true);
	const actionSerial = useRef(0);
	const pointerInside = useRef(false);
	const focusInside = useRef(false);
	const keyboardExpand = useRef(false);
	const commentFocusIntent = useRef<number | "new" | null>(null);
	const inhibit = useRef(false);
	const dragging = useRef(false);
	const pointer = useRef<{ x: number; y: number } | null>(null);
	const suppressClick = useRef(false);
	const collapseTimer = useRef<number | undefined>(undefined);
	const layoutQueue = useRef(Promise.resolve());
	const generation = useRef(0);
	const mounted = useRef(false);
	const [ready, setReady] = useState(false);
	const restoreFocus = useRef(true);
	const lastMode = useRef(snapshot.mode);
	const notice = useRef(0);
	const focusComment = useCallback((intent: number) => {
		if (
			mounted.current &&
			viewRef.current === "comment" &&
			snapshotRef.current.mode === "top" &&
			commentFocusIntent.current === intent &&
			document.activeElement !== textarea.current
		)
			textarea.current?.focus();
	}, []);

	const showFeedback = useCallback((text: string) => {
		window.clearTimeout(feedbackTimer.current);
		setFeedback(text);
		feedbackTimer.current = window.setTimeout(() => setFeedback(""), 3200);
	}, []);
	const respond = useCallback(
		(name: "curious" | "nod") =>
			setAction({ name, id: ++actionSerial.current }),
		[],
	);
	const collapse = useCallback((restore = true, suppress = false) => {
		window.clearTimeout(collapseTimer.current);
		restoreFocus.current = restore;
		if (suppress && viewRef.current !== "collapsed") inhibit.current = true;
		generation.current++;
		if (
			!draftRef.current.trim() &&
			!requests.current.isBusy("comment") &&
			!requests.current.isUncertain("comment")
		)
			anchor.current = null;
		const focused = document.activeElement;
		if (focused instanceof HTMLElement) focused.blur();
		focusInside.current = false;
		keyboardExpand.current = false;
		commentFocusIntent.current = null;
		viewRef.current = "collapsed";
		setPanelReady(false);
		setView("collapsed");
	}, []);
	const openPanel = useCallback(
		(explicit = false) => {
			window.clearTimeout(collapseTimer.current);
			if (dragging.current || viewRef.current !== "collapsed") return;
			if (explicit) inhibit.current = false;
			if (inhibit.current) return;
			keyboardExpand.current = focusInside.current;
			viewRef.current = "panel";
			setPanelReady(false);
			setView("panel");
			respond("curious");
		},
		[respond],
	);
	const leave = useCallback(() => {
		pointerInside.current = false;

		window.clearTimeout(collapseTimer.current);
		collapseTimer.current = window.setTimeout(() => {
			if (
				!pointerInside.current &&
				!focusInside.current &&
				!keyboardExpand.current &&
				viewRef.current === "panel" &&
				!dragging.current
			)
				collapse(false);
		}, 200);
	}, [collapse]);
	useEffect(() => {
		mounted.current = true;
		document.body.classList.add("floating-body");
		let disposed = false;
		const accept = (next: EntrySnapshot) => {
			if (disposed) return;
			if (next.sequence < snapshotRef.current.sequence) return;
			if (lastMode.current !== next.mode) {
				lastMode.current = next.mode;
				collapse(true, true);
			}
			snapshotRef.current = next;
			setSnapshot(next);
			setReady(true);
			if (next.notice && next.notice.id !== notice.current) {
				notice.current = next.notice.id;
				showFeedback(next.notice.message);
			}
		};
		const subscriptions = [
			listen<EntrySnapshot>("hear-entry-snapshot", (event) =>
				accept(event.payload),
			),
			listen<{
				kind: number;
				value: number;
				generation: number;
				geometry?: EntryGeometry;
			}>("hear-entry-native", ({ payload }) => {
				if (disposed) return;
				if (payload.kind === 1) {
					if (payload.value) {
						pointerInside.current = true;
						openPanel();
					} else leave();
				} else if (payload.kind === 2) collapse(false, true);
				else if (payload.kind === 3) {
					collapse(false, true);
					if (payload.geometry) setGeometry(payload.geometry);
				} else if (payload.kind === 7) {
					inhibit.current = false;
				} else if (payload.kind === 5) {
					dragging.current = false;
					pointer.current = null;
					void invoke("entry_drag", { dragging: false }).catch((error) =>
						showFeedback(userError(error)),
					);
					if (payload.geometry) setGeometry(payload.geometry);
				} else if (payload.kind === 6) setNativeVisible(!!payload.value);
				else if (
					payload.kind === 8 &&
					payload.generation === generation.current &&
					viewRef.current === "panel" &&
					snapshotRef.current.mode === "top"
				) {
					if (payload.geometry) setGeometry(payload.geometry);
					setPanelReady(true);
				} else if (payload.kind === 9 && payload.value)
					focusComment(payload.generation);
			}),
		];
		void Promise.all(subscriptions)
			.then(() => getEntrySnapshot())
			.then(accept)
			.catch((error) => {
				if (!disposed) showFeedback(userError(error));
			});
		return () => {
			disposed = true;
			mounted.current = false;
			generation.current++;
			for (const subscription of subscriptions)
				void subscription.then((unlisten) => unlisten());
			window.clearTimeout(collapseTimer.current);
			window.clearTimeout(feedbackTimer.current);
			document.body.classList.remove("floating-body");
		};
	}, [collapse, leave, openPanel, showFeedback, focusComment]);
	const hasFeedback = !!feedback;
	const hasDetail = !!snapshot.detail;
	const mode = snapshot.mode;
	useEffect(() => {
		if (!ready) return;
		const token = ++generation.current;
		const desired = view;
		if (desired === "comment" && commentFocusIntent.current === "new")
			commentFocusIntent.current = token;
		const focusIntent =
			typeof commentFocusIntent.current === "number"
				? commentFocusIntent.current
				: null;
		layoutQueue.current = layoutQueue.current
			.then(async () => {
				if (
					!mounted.current ||
					token !== generation.current ||
					mode !== snapshotRef.current.mode
				)
					return;
				if (desired !== "comment")
					await invoke("entry_focus", {
						acquire: false,
						restore: restoreFocus.current,
						generation: token,
					});
				const next = await invoke<EntryGeometry | null>("entry_layout", {
					view: desired,
					generation: token,
					feedback: hasFeedback,
					detail: hasDetail,
					suppress: inhibit.current,
				});
				if (
					!mounted.current ||
					token !== generation.current ||
					mode !== snapshotRef.current.mode
				)
					return;
				if (!next) return;
				setGeometry(next);
				if (desired === "comment" && focusIntent !== null) {
					const acquired = await invoke<boolean>("entry_focus", {
						acquire: true,
						restore: false,
						generation: focusIntent,
					});
					if (!acquired) return;
					if (mounted.current && token === generation.current)
						focusComment(focusIntent);
					else if (
						!mounted.current ||
						viewRef.current !== "comment" ||
						mode !== snapshotRef.current.mode ||
						commentFocusIntent.current !== focusIntent
					)
						await invoke("entry_focus", {
							acquire: false,
							restore: false,
							generation: token,
						});
				}
			})
			.catch((error) => {
				if (mounted.current) showFeedback(userError(error));
			});
	}, [ready, view, mode, hasDetail, hasFeedback, showFeedback, focusComment]);
	const perform = async (kind: EntryKind) => {
		if (active.current.has(kind) || requests.current.isBusy(kind)) return;
		if (
			kind === "comment" &&
			!requests.current.isUncertain(kind) &&
			!commentCanSubmit(draftRef.current, composing.current)
		)
			return;
		active.current.add(kind);
		const text = kind === "comment" ? draftRef.current : null;
		const savedAnchor =
			kind === "comment" ? (anchor.current ?? Date.now()) : Date.now();
		if (snapshotRef.current.mode === "top" && kind !== "control")
			collapse(true, true);
		setBusy((current) => ({ ...current, [kind]: true }));
		try {
			if (kind === "screenshot" && !requests.current.isUncertain(kind)) {
				const privacy = await invoke<{ screen_and_system_audio: number }>(
					"privacy_status",
				);
				if (privacy.screen_and_system_audio !== 1) {
					const granted = await invoke<number>("request_screenshot_permission");
					if (granted !== 1) {
						showFeedback("请在系统设置中允许截屏后重试");
						return;
					}
				}
			}
			const result = await requests.current.send(kind, savedAnchor, text);
			setUncertain((current) => ({
				...current,
				[kind]: result.outcome === "uncertain",
			}));
			if (result.outcome === "confirmed") {
				if (kind === "comment") {
					setDraft("");
					draftRef.current = "";
					anchor.current = null;
				}
				if (kind !== "control") {
					respond("nod");
					showFeedback(
						kind === "mark"
							? "已标记重点"
							: kind === "screenshot"
								? "截屏已保存"
								: "评论已保存",
					);
				}
			} else
				showFeedback(
					result.outcome === "uncertain"
						? "结果尚未确认，再次操作可确认原记录"
						: userError(result.message ?? "保存未完成，请重试"),
				);
		} catch (error) {
			setUncertain((current) => ({
				...current,
				[kind]: requests.current.isUncertain(kind),
			}));
			showFeedback(userError(error));
		} finally {
			active.current.delete(kind);
			setBusy((current) => ({ ...current, [kind]: false }));
		}
	};
	const openComment = async () => {
		if (snapshotRef.current.mode === "floating") {
			try {
				await invoke("show_main");
				await emit("hear-open-comment", { anchor_ms: Date.now() });
			} catch (error) {
				showFeedback(userError(error));
			}
		} else {
			anchor.current ??= Date.now();
			commentFocusIntent.current = "new";
			viewRef.current = "comment";
			setView("comment");
		}
	};
	const openSettings = async () => {
		try {
			await invoke("show_main");
			await emit("hear-open-settings");
		} catch (error) {
			showFeedback(userError(error));
		}
	};
	const control = snapshot.control;
	const controlLabel =
		control === "pause" ? "暂停" : control === "start" ? "开始" : "继续";
	const isTop = snapshot.mode === "top";
	const expanded = view !== "collapsed";
	const rabbitSize = 52;
	const pendingComment = !!busy.comment || !!uncertain.comment;
	const rabbit = (
		<Rabbit
			state={entryAppearance(snapshot)}
			action={action}
			visible={nativeVisible}
		/>
	);
	return (
		<div
			role="toolbar"
			aria-label="hear 常驻入口"
			className={`entry-root ${isTop ? "entry-top" : "entry-floating"}`}
			style={
				{
					"--entry-band": `${geometry.band}px`,
					...topRabbitMotion(
						geometry.top_band,
						geometry.top_shift,
						hasFeedback,
					),
				} as React.CSSProperties
			}
			onFocusCapture={() => {
				focusInside.current = true;
				openPanel(true);
			}}
			onBlurCapture={(event) => {
				if (!event.currentTarget.contains(event.relatedTarget)) {
					focusInside.current = false;
					if (!pointerInside.current) leave();
				}
			}}
			onKeyDown={(event) => {
				if (
					event.key === "Escape" &&
					!composing.current &&
					!event.nativeEvent.isComposing
				) {
					event.preventDefault();
					collapse(true, true);
				}
			}}
		>
			{isTop ? (
				<div
					className={`notch-surface ${view === "comment" ? "comment-surface" : ""}`}
					data-ready={panelReady}
					data-view={view}
				>
					{view === "collapsed" ? null : view === "comment" ? (
						<div className="notch-composer">
							<textarea
								ref={textarea}
								aria-label="留下评论"
								placeholder="留下一句话…"
								value={draft}
								readOnly={pendingComment}
								onChange={(event) => {
									setDraft(event.target.value);
									draftRef.current = event.target.value;
								}}
								onCompositionStart={() => {
									composing.current = true;
								}}
								onCompositionEnd={() => {
									composing.current = false;
								}}
								onKeyDown={(event) => {
									if (
										commentEnterSubmits(
											{
												key: event.key,
												shiftKey: event.shiftKey,
												keyCode: event.keyCode,
												isComposing: event.nativeEvent.isComposing,
											},
											composing.current,
										)
									) {
										event.preventDefault();
										void perform("comment");
									}
								}}
							/>
							<div className="comment-footer">
								<span>
									{uncertain.comment
										? "原评论结果待确认"
										: "回车保存 · ⇧ 回车换行"}
								</span>
								<div className="comment-footer-actions">
									<button
										type="button"
										className="comment-cancel"
										onClick={() => collapse(true, true)}
									>
										取消
									</button>
									<button
										type="button"
										disabled={
											!!busy.comment ||
											(!uncertain.comment && !commentCanSubmit(draft, false))
										}
										onClick={() => void perform("comment")}
									>
										{uncertain.comment ? "确认结果" : "保存"}
									</button>
								</div>
							</div>
						</div>
					) : (
						<>
							<div className="notch-rabbit-space" aria-hidden="true" />
							<div
								className="notch-actions"
								role="toolbar"
								aria-label="快速操作"
							>
								<button
									type="button"
									disabled={!!busy.screenshot}
									ref={(element) => {
										if (
											element &&
											panelReady &&
											isTop &&
											keyboardExpand.current &&
											(!control || busy.control)
										) {
											keyboardExpand.current = false;
											element.focus();
										}
									}}
									onClick={() => void perform("screenshot")}
									title={uncertain.screenshot ? "确认原截屏结果" : undefined}
								>
									<Glyph kind="screenshot" />
									<span>截图</span>
								</button>
								<button
									type="button"
									disabled={!!busy.mark}
									onClick={() => void perform("mark")}
									title={uncertain.mark ? "确认原重点结果" : undefined}
								>
									<Glyph kind="mark" />
									<span>重点</span>
								</button>
								<button type="button" onClick={() => void openComment()}>
									<Glyph kind="comment" />
									<span>评论</span>
								</button>
							</div>
							{snapshot.detail && (
								<button
									type="button"
									className="entry-detail"
									onClick={() => void openSettings()}
									title="查看设置"
								>
									{snapshot.detail}
									<span>↗</span>
								</button>
							)}
						</>
					)}
				</div>
			) : (
				<>
					{!isTop && view === "panel" && (
						<div className="rabbit-menu" role="toolbar" aria-label="快速操作">
							<button
								type="button"
								disabled={!control || !!busy.control}
								onClick={() => void perform("control")}
							>
								<Glyph kind={control ?? "resume"} />
								<span>{controlLabel}</span>
							</button>
							<button type="button" onClick={() => void openComment()}>
								<Glyph kind="comment" />
								<span>评论</span>
							</button>
							<button
								type="button"
								disabled={!!busy.screenshot}
								onClick={() => void perform("screenshot")}
							>
								<Glyph kind="screenshot" />
								<span>截屏</span>
							</button>
						</div>
					)}
					<button
						type="button"
						className="entry-rabbit"
						style={{
							width: rabbitSize,
							height: rabbitSize,
						}}
						aria-label={isTop ? "hear 快速操作" : "标记重点；拖动以移动兔子"}
						title={!isTop && snapshot.detail ? snapshot.detail : undefined}
						onMouseEnter={() => {
							pointerInside.current = true;
							openPanel();
						}}
						onPointerDown={(event) => {
							if (!isTop) {
								pointer.current = { x: event.screenX, y: event.screenY };
								suppressClick.current = false;
							}
						}}
						onPointerMove={(event) => {
							if (
								isTop ||
								!pointer.current ||
								dragging.current ||
								Math.hypot(
									event.screenX - pointer.current.x,
									event.screenY - pointer.current.y,
								) < 7
							)
								return;
							dragging.current = true;
							suppressClick.current = true;
							pointer.current = null;
							void invoke("entry_drag", { dragging: true })
								.then(() => getCurrentWindow().startDragging())
								.catch((error) => showFeedback(userError(error)));
						}}
						onPointerUp={() => {
							pointer.current = null;
						}}
						onClick={() => {
							if (isTop) openPanel(true);
							else if (suppressClick.current) suppressClick.current = false;
							else void perform("mark");
						}}
					>
						{rabbit}
					</button>
				</>
			)}
			{isTop && (
				<button
					key="top-rabbit"
					type="button"
					className="entry-rabbit notch-rabbit-control"
					data-view={view}
					data-ready={panelReady}
					aria-label={expanded ? controlLabel : "hear 快速操作"}
					tabIndex={view === "comment" ? -1 : 0}
					disabled={expanded && (!control || !!busy.control)}
					aria-disabled={
						expanded && (!panelReady || !control || !!busy.control)
					}
					ref={(element) => {
						if (
							element &&
							panelReady &&
							keyboardExpand.current &&
							control &&
							!busy.control
						) {
							keyboardExpand.current = false;
							element.focus();
						}
					}}
					onMouseEnter={() => {
						pointerInside.current = true;
						openPanel();
					}}
					onClick={() => {
						if (viewRef.current === "collapsed") openPanel(true);
						else if (viewRef.current === "panel" && panelReady)
							void perform("control");
					}}
				>
					{rabbit}
					<span className="rabbit-control-overlay">
						<Glyph kind={control ?? "resume"} />
					</span>
				</button>
			)}
			{feedback && (
				<div
					className={`entry-feedback ${!isTop && expanded ? "menu-feedback" : ""}`}
					role="status"
				>
					{feedback}
				</div>
			)}
			{!isTop && snapshot.detail && view === "panel" && (
				<button
					type="button"
					className="floating-detail"
					onClick={() => void openSettings()}
					aria-label={snapshot.detail}
				>
					!
				</button>
			)}
		</div>
	);
}
