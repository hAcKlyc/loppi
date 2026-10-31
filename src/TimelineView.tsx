import {
	type ReactNode,
	useCallback,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import {
	dateTitle,
	dayBounds,
	type Moment,
	manualAsrSummary,
	processingLabel,
	type ServiceState,
	type Source,
	type TimeGap,
	type TimelineItem,
	time,
} from "./api";
import { Button } from "./Controls";
import { DayMapCursor } from "./DayMapCursor";
import { LoadBoundary } from "./LoadingState";
import { RecordCalendar } from "./RecordCalendar";
import {
	cardExcerpt,
	dayMapPoint,
	daySpan,
	headerDays,
	liveCard,
} from "./timeline-view";

type Props = {
	captureStatus: ReactNode;
	date: Date;
	recordedDays: number[] | null;
	recordingState: ServiceState | null;
	items: TimelineItem[];
	gaps: TimeGap[];
	moments: Moment[];
	loading: boolean;
	ready?: boolean;
	loadError?: string;
	onRetry?: () => void;
	hideCoarse: boolean;
	onDate: (date: Date) => void;
	onSelect: (item: TimelineItem, at?: number) => void;
	onCreate: () => void;
};

const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

function dateInput(date: Date): string {
	const year = date.getFullYear();
	const month = String(date.getMonth() + 1).padStart(2, "0");
	const day = String(date.getDate()).padStart(2, "0");
	return `${year}-${month}-${day}`;
}

function gapReason(reason: string): string {
	switch (reason) {
		case "manual_pause":
			return "已暂停";
		case "schedule_pause":
			return "定时暂停";
		case "storage_limit":
			return "空间不足";
		case "source_disabled":
			return "声音来源已关闭";
		case "source_failure":
			return "录音中断";
		case "interrupted_tail":
			return "录音中断";
		case "sleep_or_clock_gap":
			return "休眠或时钟变化";
		case "machine_sleep":
			return "电脑休眠";
		default:
			return "未记录";
	}
}

export function TimelineView({
	captureStatus,
	date,
	recordedDays,
	recordingState,
	items,
	gaps,
	moments,
	loading,
	ready = !loading,
	loadError,
	onRetry,
	hideCoarse,
	onDate,
	onSelect,
	onCreate,
}: Props) {
	const [calendarOpen, setCalendarOpen] = useState(false);
	const today = new Date();
	const todayKey = dateInput(today);
	const selectedKey = dateInput(date);
	const [from, to] = dayBounds(date);
	const dates = headerDays(
		recordedDays ?? (items.length ? [from] : []),
		from,
	).map((ms) => new Date(ms));
	const dateTrack = useRef<HTMLElement>(null);
	const datePositions = useRef<{
		target: string;
		direction: number;
		positions: Map<string, { left: number; title: DOMRect }>;
	} | null>(null);
	const chooseDate = (next: Date) => {
		// Read painted geometry before React changes the layout, including any
		// animation in flight. A second click continues from that exact frame.
		const positions = new Map<string, { left: number; title: DOMRect }>();
		for (const node of dateTrack.current?.querySelectorAll<HTMLElement>(
			"[data-date]",
		) ?? []) {
			const title = node.querySelector("strong");
			if (title)
				positions.set(node.dataset.date ?? "", {
					left: node.getBoundingClientRect().left,
					title: title.getBoundingClientRect(),
				});
		}
		datePositions.current = {
			target: dateInput(next),
			direction: dateInput(next) < selectedKey ? 1 : -1,
			positions,
		};
		onDate(next);
	};
	useLayoutEffect(() => {
		const previous = datePositions.current;
		datePositions.current = null;
		if (
			!previous ||
			previous.target !== selectedKey ||
			window.matchMedia("(prefers-reduced-motion: reduce)").matches
		)
			return;
		const animations: Animation[] = [];
		const motion = getComputedStyle(document.documentElement);
		const options = {
			duration: Number.parseFloat(motion.getPropertyValue("--motion-layout")),
			easing: motion.getPropertyValue("--ease-standard").trim(),
		};
		// Natural widths establish the destination once. Animate positions and
		// text geometry together instead of competing with flex/width transitions.
		for (const node of dateTrack.current?.querySelectorAll<HTMLElement>(
			"[data-date]",
		) ?? []) {
			const before = previous.positions.get(node.dataset.date ?? "");
			const title = node.querySelector("strong");
			if (!title) continue;
			const after = title.getBoundingClientRect();
			const dx = before
				? before.left - node.getBoundingClientRect().left
				: previous.direction *
					Number.parseFloat(motion.getPropertyValue("--space-9"));
			if (before) {
				const base = getComputedStyle(title).transform;
				const dy = before.title.top - after.top;
				animations.push(
					title.animate(
						[
							{
								transform: `translate(${before.title.left - after.left - dx}px, ${dy}px) ${base} scale(${before.title.width / after.width}, ${before.title.height / after.height})`,
							},
							{ transform: base },
						],
						options,
					),
				);
			}
			animations.push(
				node.animate(
					[
						{ transform: `translateX(${dx}px)`, opacity: before ? 1 : 0 },
						{ transform: "translateX(0)", opacity: 1 },
					],
					options,
				),
			);
		}
		return () => {
			for (const animation of animations) animation.cancel();
		};
	}, [selectedKey]);
	const activeItem = liveCard(items, recordingState, date, today.getTime());
	const calendarButton = useRef<HTMLButtonElement>(null);
	const closeCalendar = useCallback(() => {
		setCalendarOpen(false);
		calendarButton.current?.focus();
	}, []);
	const position = (ms: number) =>
		`${Math.max(0, Math.min(100, ((ms - from) / (to - from)) * 100))}%`;
	const sorted = [...items].sort((a, b) => b.started_ms - a.started_ms);
	const visible = sorted.filter(
		(item) =>
			!hideCoarse ||
			item === activeItem ||
			item.id !== null ||
			item.manual_range_id !== null ||
			(item.local_analysis_failures ?? 0) > 0 ||
			[
				"fit_failed",
				"fit_retrying",
				"fit_credentials_unavailable",
				"analysis_failed",
				"asr_failed",
				"partial_asr_failed",
			].includes(item.processing_state) ||
			moments.some(
				(moment) =>
					moment.anchor_ms >= item.started_ms &&
					moment.anchor_ms < item.ended_ms,
			),
	);
	const now = today.getTime();
	const statusGaps = gaps.flatMap((gap) => {
		const style = daySpan(gap.started_ms, gap.ended_ms, from, to, now);
		return style
			? [
					{
						...gap,
						started_ms: Math.max(from, gap.started_ms),
						ended_ms: Math.min(to, now, gap.ended_ms),
						style,
					},
				]
			: [];
	});
	const dayMoments = moments.filter(
		(moment) =>
			moment.anchor_ms >= from &&
			moment.anchor_ms < to &&
			moment.anchor_ms <= now,
	);
	const sources: { key: Source; label: string }[] = [
		{ key: "microphone", label: "麦克风" },
		{ key: "system", label: "电脑声音" },
	];

	return (
		<div className="timeline-page">
			<header className="timeline-header" data-tauri-drag-region>
				<div className="date-header" data-tauri-drag-region>
					<div className="calendar-anchor">
						<button
							ref={calendarButton}
							aria-expanded={calendarOpen}
							aria-haspopup="dialog"
							className="calendar-button"
							type="button"
							title="选择日期"
							aria-label="选择日期"
							onClick={() => setCalendarOpen(!calendarOpen)}
						>
							<svg viewBox="0 0 24 24" aria-hidden="true">
								<rect x="3" y="5" width="18" height="16" rx="2" />
								<path d="M7 3v4M17 3v4M3 10h18" />
							</svg>
						</button>
						{calendarOpen && (
							<RecordCalendar
								date={date}
								days={recordedDays}
								onDate={chooseDate}
								onClose={closeCalendar}
							/>
						)}
					</div>
					<nav
						ref={dateTrack}
						className="date-track"
						aria-label="按日期浏览"
						data-tauri-drag-region
					>
						{dates.map((day) => {
							const key = dateInput(day);
							const selected = key === selectedKey;
							return (
								<button
									key={key}
									data-date={key}
									type="button"
									className={`date-choice ${selected ? "date-current" : "date-neighbor"}`}
									aria-current={selected ? "date" : undefined}
									aria-label={`${dateTitle(day)}${selected ? "，已选中" : ""}`}
									onClick={() => {
										if (!selected) chooseDate(day);
									}}
									disabled={key > todayKey}
								>
									<strong>{dateTitle(day)}</strong>
									<span aria-hidden={!selected}>
										{weekdays[day.getDay()]}
										{key === todayKey ? " · 今天" : ""}
									</span>
								</button>
							);
						})}
					</nav>
				</div>
				{captureStatus}
			</header>

			<section className="day-map" aria-label="当天内容范围与未记录时段">
				<div className="day-map-heading">
					<strong>记录分布</strong>
					<div className="day-map-key">
						<div className="day-map-legend">
							<span>
								<i className="raw" />
								未整理
							</span>
							<span>
								<i className="organized" />
								事件 / 精转范围
							</span>
							<span>
								<i className="gap" />
								未记录
							</span>
						</div>
						{selectedKey === todayKey && (
							<span className="day-map-clock">现在 {time(now)}</span>
						)}
					</div>
				</div>
				<LoadBoundary
					ready={ready}
					error={loadError}
					retry={onRetry}
					label="记录分布"
					rows={2}
				>
					<div className="day-map-plot">
						{sources.map(({ key, label }) => (
							<div className="day-map-row" key={key}>
								<span className="day-map-source">{label}</span>
								<div className="day-map-track">
									{items
										.filter(
											(item) =>
												item.processing_state !== "moment_only" &&
												(item.sources === "both" || item.sources === key),
										)
										.map((item) => {
											const style = daySpan(
												item.started_ms,
												item.ended_ms,
												from,
												to,
												now,
											);
											if (!style) return null;
											const title = `${time(Math.max(item.started_ms, from))}—${time(Math.min(item.ended_ms, to, now))} · ${item.title || (item.manual_range_id ? "精转范围" : "未整理素材")} · ${label}`;
											return (
												<button
													key={`${item.id ?? item.manual_range_id ?? "raw"}-${item.started_ms}-${item.sources}`}
													type="button"
													className={`day-map-event ${item.id || item.manual_range_id ? "organized" : "raw"}`}
													title={title}
													aria-label={title}
													style={style}
													onClick={(event) => {
														event.currentTarget.focus();
														const bounds =
															event.currentTarget.parentElement?.getBoundingClientRect();
														const point =
															event.detail > 0 && bounds
																? dayMapPoint(
																		event.clientX,
																		bounds.left,
																		bounds.width,
																		from,
																		to,
																	)
																: null;
														onSelect(item, point?.at);
													}}
												/>
											);
										})}
									{statusGaps
										.filter((gap) => gap.source === key)
										.map((gap) => (
											<span
												key={`${gap.started_ms}-${gap.reason}`}
												className="day-map-gap"
												style={gap.style}
												title={`${time(gap.started_ms)}—${time(gap.ended_ms)} · ${gapReason(gap.reason)} · ${label}`}
											/>
										))}
								</div>
							</div>
						))}
						{dayMoments.length > 0 && (
							<div className="day-map-row">
								<span className="day-map-source">现场记录</span>
								<div className="day-map-moments">
									{dayMoments.map((moment) => (
										<span
											key={moment.id}
											className={`day-map-moment ${moment.kind}`}
											style={{ left: position(moment.anchor_ms) }}
											title={`${time(moment.anchor_ms)} · ${moment.kind === "mark" ? "重点" : moment.kind === "comment" ? "笔记" : "截图"}`}
										/>
									))}
								</div>
							</div>
						)}
						{selectedKey === todayKey && (
							<div className="day-map-now-track">
								<span className="day-map-now" style={{ left: position(now) }} />
							</div>
						)}
						<DayMapCursor from={from} to={to} />
					</div>
					<div className="day-map-hours">
						<span>00:00</span>
						<span>06:00</span>
						<span>12:00</span>
						<span>18:00</span>
						<span>24:00</span>
					</div>
				</LoadBoundary>
			</section>

			<div className="section-heading">
				<h2>
					当天记录{" "}
					{ready && <span className="record-count">{visible.length}</span>}
				</h2>
				<div className="section-actions">
					<Button
						type="button"
						tone="secondary"
						className="create-event"
						onClick={onCreate}
					>
						＋ 新建事件
					</Button>
				</div>
			</div>
			<div className="event-list">
				<LoadBoundary
					ready={ready}
					error={loadError}
					retry={onRetry}
					label="当天记录"
				>
					{visible.length === 0 ? (
						<div className="list-empty">
							{sorted.length > 0 ? "粗转内容已隐藏" : "这一天还没有记录"}
						</div>
					) : (
						visible.map((item) => {
							const itemMoments = moments.filter(
								(moment) =>
									moment.anchor_ms >= item.started_ms &&
									moment.anchor_ms < item.ended_ms,
							);
							const count = itemMoments.length;
							const live = item === activeItem;
							const excerpt = cardExcerpt(
								item,
								itemMoments,
								hideCoarse && !live,
							);
							const manualStatus = manualAsrSummary(item);
							const fitIssue =
								item.fit_state &&
								[
									"fit_failed",
									"fit_credentials_unavailable",
									"fit_retrying",
								].includes(item.fit_state)
									? item.fit_state
									: null;
							const processingState = item.processing_state;
							const localIssue = (item.local_analysis_failures ?? 0) > 0;
							const processingIssue = [
								"fit_failed",
								"fit_credentials_unavailable",
								"fit_retrying",
								"asr_failed",
								"partial_asr_failed",
								"analysis_failed",
							].includes(processingState);
							const sourceLabel =
								excerpt.label ||
								(item.processing_state === "moment_only"
									? "时刻记录"
									: item.sources === "both"
										? "双路声音"
										: item.sources === "microphone"
											? "麦克风"
											: "电脑声音");
							return (
								<div
									className="event-line"
									key={`${item.id ?? item.manual_range_id ?? "unorganized"}-${item.started_ms}-${item.ended_ms}-${item.sources}`}
								>
									<time>{time(item.started_ms)}</time>
									<button
										type="button"
										className={`event-card ${item.id || item.manual_range_id ? "named" : ""} ${live ? "is-recording" : ""}`}
										onClick={(event) => {
											event.currentTarget.focus({ preventScroll: true });
											onSelect(item);
										}}
									>
										<div className="event-card-meta">
											<span>
												{time(item.started_ms)}
												{item.ended_ms - item.started_ms < 60_000 &&
												item.processing_state === "moment_only"
													? ""
													: ` — ${live ? "至今" : time(item.ended_ms)}`}
											</span>
											<span className="event-card-source">{sourceLabel}</span>
											{count > 0 && <span>{count} 条现场记录</span>}
											{manualStatus && (
												<span className="event-card-task" title={manualStatus}>
													{manualStatus}
												</span>
											)}
											{localIssue && (
												<span className="event-card-state is-failed">
													部分本地分析失败 · {item.local_analysis_failures} 项
												</span>
											)}
											{fitIssue && fitIssue !== processingState && (
												<span className="event-card-state is-failed">
													{fitIssue === "fit_failed"
														? "部分整理失败"
														: processingLabel(fitIssue)}
												</span>
											)}
											{(!live || processingIssue) && (
												<span
													className={`event-card-state ${processingIssue ? "is-failed" : ""}`}
													title={
														processingState === "fit_credentials_unavailable"
															? "请重新打开 loppi；若仍失败，请在设置中重新保存云端密钥"
															: undefined
													}
												>
													{processingLabel(processingState, item.transcription)}
												</span>
											)}
											{live && (
												<span className="event-card-state recording-label">
													正在记录
												</span>
											)}
										</div>
										<h3>
											{item.title ||
												(live
													? "实时记录内容"
													: item.manual_range_id
														? "未命名事件"
														: (hideCoarse && !processingIssue && !localIssue) ||
																item.processing_state === "moment_only"
															? "现场记录"
															: "未整理素材")}
										</h3>
										<p
											className={excerpt.label ? "" : "event-card-placeholder"}
										>
											{excerpt.text}
										</p>
									</button>
								</div>
							);
						})
					)}
				</LoadBoundary>
			</div>
		</div>
	);
}
