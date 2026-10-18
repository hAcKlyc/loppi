import { useEffect, useRef, useState } from "react";
import { core } from "./api";
import { LoadingSkeleton } from "./LoadingState";
import { monthBoundaries } from "./timeline-view";

type Props = {
	date: Date;
	days: number[] | null;
	onDate: (date: Date) => void;
	onClose: () => void;
};
export function RecordCalendar({ date, days, onDate, onClose }: Props) {
	const [month, setMonth] = useState(
		() => new Date(date.getFullYear(), date.getMonth(), 1),
	);
	const [result, setResult] = useState<{
		month: number;
		days: number[] | null;
		error?: boolean;
	} | null>(null);
	const root = useRef<HTMLDivElement>(null);
	const start = month.getTime();
	const loading = result?.month !== start;
	const available = result?.month === start ? result.days : null;
	const currentMonth = new Date(
		date.getFullYear(),
		date.getMonth(),
		1,
	).getTime();
	useEffect(() => {
		let cancelled = false;
		if (start === currentMonth && days !== null) {
			setResult({ month: start, days });
			return;
		}
		void core<{ type: "calendar_days"; id: string; days: number[] }>({
			type: "calendar_days",
			boundaries: monthBoundaries(new Date(start)),
		})
			.then((response) => {
				if (!cancelled) setResult({ month: start, days: response.days });
			})
			.catch(() => {
				if (!cancelled)
					setResult((current) => ({
						month: start,
						days: current?.month === start ? current.days : null,
						error: true,
					}));
			});
		return () => {
			cancelled = true;
		};
	}, [start, currentMonth, days]);
	useEffect(() => {
		root.current
			?.querySelector<HTMLButtonElement>(".calendar-month-heading button")
			?.focus();
		const close = (event: PointerEvent) => {
			if (
				event.target instanceof Node &&
				!root.current?.parentElement?.contains(event.target)
			)
				onClose();
		};
		document.addEventListener("pointerdown", close);
		return () => document.removeEventListener("pointerdown", close);
	}, [onClose]);
	const today = new Date();
	const todayStart = new Date(
		today.getFullYear(),
		today.getMonth(),
		today.getDate(),
	).getTime();
	const dates = monthBoundaries(month).slice(0, -1);
	const offset = (month.getDay() + 6) % 7;
	return (
		<div
			ref={root}
			className="calendar-popover"
			role="dialog"
			aria-label="选择有记录的日期"
			onKeyDown={(event) => {
				if (event.key === "Escape") {
					event.stopPropagation();
					onClose();
				}
			}}
		>
			<div className="calendar-month-heading">
				<button
					type="button"
					aria-label="上个月"
					onClick={() =>
						setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))
					}
				>
					‹
				</button>
				<strong aria-live="polite">
					{month.getFullYear()}年{month.getMonth() + 1}月
				</strong>
				<button
					className="calendar-month-next"
					type="button"
					aria-label="下个月"
					disabled={
						start >=
						new Date(today.getFullYear(), today.getMonth(), 1).getTime()
					}
					onClick={() =>
						setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))
					}
				>
					›
				</button>
			</div>
			<div className="calendar-weekdays" aria-hidden="true">
				{["一", "二", "三", "四", "五", "六", "日"].map((day) => (
					<span key={day}>{day}</span>
				))}
			</div>
			{loading ? (
				<LoadingSkeleton label="正在加载记录日期" rows={3} />
			) : (
				<div className="calendar-grid">
					{dates.map((day, index) => (
						<button
							key={day}
							type="button"
							style={index === 0 ? { gridColumnStart: offset + 1 } : undefined}
							className={
								day ===
								new Date(
									date.getFullYear(),
									date.getMonth(),
									date.getDate(),
								).getTime()
									? "selected"
									: ""
							}
							aria-label={`${month.getFullYear()}年${month.getMonth() + 1}月${index + 1}日`}
							aria-current={day === todayStart ? "date" : undefined}
							aria-pressed={
								day ===
								new Date(
									date.getFullYear(),
									date.getMonth(),
									date.getDate(),
								).getTime()
							}
							disabled={
								loading || day > todayStart || !available?.includes(day)
							}
							onClick={() => {
								onDate(new Date(day));
								onClose();
							}}
						>
							{index + 1}
						</button>
					))}
				</div>
			)}
			{loading ? null : result?.error ? (
				<p role="status">日期加载失败，请重新打开日历</p>
			) : available?.length === 0 ? (
				<p>本月暂无记录</p>
			) : null}
		</div>
	);
}
