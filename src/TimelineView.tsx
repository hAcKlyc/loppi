import { useState } from "react";
import {
	addDays,
	dateTitle,
	dayBounds,
	type Moment,
	manualAsrSummary,
	processingLabel,
	type TimeGap,
	type TimelineItem,
	time,
} from "./api";

type Props = {
	date: Date;
	items: TimelineItem[];
	gaps: TimeGap[];
	moments: Moment[];
	loading: boolean;
	hideCoarse: boolean;
	onHideCoarse: (hide: boolean) => void;
	onDate: (date: Date) => void;
	onSelect: (item: TimelineItem) => void;
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
		default:
			return "未记录";
	}
}

export function TimelineView({
	date,
	items,
	gaps,
	moments,
	loading,
	hideCoarse,
	onHideCoarse,
	onDate,
	onSelect,
	onCreate,
}: Props) {
	const [calendarOpen, setCalendarOpen] = useState(false);
	const today = new Date();
	const todayKey = dateInput(today);
	const selectedKey = dateInput(date);
	const dates =
		selectedKey === todayKey
			? [date, addDays(date, -1), addDays(date, -2)]
			: [addDays(date, 1), date, addDays(date, -1)];
	const [from, to] = dayBounds(date);
	const position = (ms: number) =>
		`${Math.max(0, Math.min(100, ((ms - from) / (to - from)) * 100))}%`;
	const sorted = [...items].sort((a, b) => b.started_ms - a.started_ms);
	const visible = sorted.filter(
		(item) =>
			!hideCoarse ||
			item.id !== null ||
			item.manual_range_id !== null ||
			moments.some(
				(moment) =>
					moment.anchor_ms >= item.started_ms &&
					moment.anchor_ms < item.ended_ms,
			),
	);
	const statusGaps = gaps.filter((gap) => gap.ended_ms > gap.started_ms);

	return (
		<div className="timeline-page">
			<div className="date-header">
				<div className="calendar-anchor">
					<button
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
						<div className="calendar-popover">
							<label htmlFor="jump-date">跳转日期</label>
							<input
								id="jump-date"
								type="date"
								max={todayKey}
								value={selectedKey}
								onChange={(event) => {
									const [year, month, day] = event.target.value
										.split("-")
										.map(Number);
									if (year && month && day)
										onDate(new Date(year, month - 1, day));
									setCalendarOpen(false);
								}}
							/>
						</div>
					)}
				</div>
				<nav className="date-track" aria-label="按日期浏览">
					{dates.map((day) => {
						const key = dateInput(day);
						const selected = key === selectedKey;
						return selected ? (
							<div key={key} className="date-current" aria-current="date">
								<strong>{dateTitle(day)}</strong>
								<span>
									{weekdays[day.getDay()]}
									{key === todayKey ? " · 今天" : ""}
								</span>
							</div>
						) : (
							<button
								key={key}
								type="button"
								className="date-neighbor"
								onClick={() => onDate(day)}
								disabled={key > todayKey}
							>
								{dateTitle(day)}
							</button>
						);
					})}
				</nav>
			</div>

			<section className="day-map" aria-label="这一天的事件分布">
				<div className="day-map-track">
					{statusGaps.map((gap) => (
						<span
							key={`${gap.source}-${gap.started_ms}-${gap.reason}`}
							className="day-map-gap"
							style={{
								left: position(gap.started_ms),
								width: `${Math.max(0.3, ((gap.ended_ms - gap.started_ms) / (to - from)) * 100)}%`,
							}}
							title={`${time(gap.started_ms)}—${time(gap.ended_ms)} · ${gapReason(gap.reason)} · ${gap.source === "microphone" ? "麦克风" : "电脑声音"}`}
						/>
					))}
					{moments.map((moment) => (
						<span
							key={moment.id}
							className={`day-map-moment ${moment.kind}`}
							style={{ left: position(moment.anchor_ms) }}
							title={`${time(moment.anchor_ms)} · ${moment.kind === "mark" ? "重点" : moment.kind === "comment" ? "评论" : "截屏"}`}
						/>
					))}
					{items
						.filter((item) => item.id || item.manual_range_id)
						.map((item) => (
							<button
								key={`${item.id ?? item.manual_range_id}-${item.started_ms}-${item.sources}`}
								type="button"
								className={`day-map-event ${item.manual_range_id ? "manual" : ""}`}
								title={`${time(item.started_ms)}—${time(item.ended_ms)} ${item.title ?? "未命名事件"}`}
								style={{
									left: position(item.started_ms),
									width: `${Math.max(1.1, ((Math.min(item.ended_ms, to) - Math.max(item.started_ms, from)) / (to - from)) * 100)}%`,
								}}
								onClick={() => onSelect(item)}
							/>
						))}
				</div>
				<div className="day-map-hours">
					<span>00:00</span>
					<span>06:00</span>
					<span>12:00</span>
					<span>18:00</span>
					<span>24:00</span>
				</div>
				{statusGaps.length > 0 && (
					<details className="day-map-gap-details">
						<summary>
							未记录时段 <em>{statusGaps.length}</em>
						</summary>
						<div className="day-map-gap-list">
							{statusGaps.map((gap) => (
								<span key={`${gap.source}-${gap.started_ms}-${gap.reason}`}>
									<i />
									{time(gap.started_ms)}—{time(gap.ended_ms)}{" "}
									{gapReason(gap.reason)} ·{" "}
									{gap.source === "microphone" ? "麦克风" : "电脑声音"}
								</span>
							))}
						</div>
					</details>
				)}
			</section>

			<div className="section-heading">
				<h2>日程</h2>
				<div className="section-actions">
					<button
						type="button"
						className={`quiet-button ${hideCoarse ? "active" : ""}`}
						aria-pressed={hideCoarse}
						onClick={() => onHideCoarse(!hideCoarse)}
					>
						{hideCoarse ? "显示粗转" : "隐藏粗转"}
					</button>
					<button type="button" className="quiet-button" onClick={onCreate}>
						＋ 新建事件
					</button>
				</div>
			</div>
			<div className="event-list">
				{loading ? (
					<div className="list-empty">正在加载…</div>
				) : visible.length === 0 ? (
					<div className="list-empty">
						{sorted.length > 0 ? "粗转内容已隐藏" : "这一天还没有记录"}
					</div>
				) : (
					visible.map((item) => {
						const count = moments.filter(
							(moment) =>
								moment.anchor_ms >= item.started_ms &&
								moment.anchor_ms < item.ended_ms,
						).length;
						const excerpt =
							hideCoarse && !item.id && !item.manual_range_id
								? ""
								: (item.precise_segments[0]?.text ??
									item.coarse_segments[0]?.text ??
									"");
						const manualStatus = manualAsrSummary(item);
						return (
							<div
								className="event-line"
								key={`${item.id ?? item.manual_range_id ?? "unorganized"}-${item.started_ms}-${item.ended_ms}-${item.sources}`}
							>
								<time>{time(item.started_ms)}</time>
								<button
									type="button"
									className={`event-card ${item.id || item.manual_range_id ? "named" : ""}`}
									onClick={() => onSelect(item)}
								>
									<div className="event-card-meta">
										<span>
											{time(item.started_ms)}
											{item.ended_ms - item.started_ms < 60_000 &&
											item.processing_state === "moment_only"
												? ""
												: ` — ${time(item.ended_ms)}`}
										</span>
										<span>{processingLabel(item.processing_state)}</span>
									</div>
									<h3>
										{item.title ||
											(item.manual_range_id
												? "未命名事件"
												: hideCoarse
													? "现场记录"
													: "未整理")}
									</h3>
									{excerpt && <p>{excerpt}</p>}
									<div className="event-card-foot">
										<span>
											{item.processing_state === "moment_only"
												? "时刻记录"
												: item.sources === "both"
													? "双路声音"
													: item.sources === "microphone"
														? "麦克风"
														: "电脑声音"}
										</span>
										{count > 0 && <span>{count} 条记录</span>}
										{manualStatus && (
											<span className="event-card-task">{manualStatus}</span>
										)}
									</div>
								</button>
							</div>
						);
					})
				)}
			</div>
		</div>
	);
}
