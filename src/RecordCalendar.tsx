import { useEffect, useRef, useState } from "react";
import { core } from "./api";
import { LoadingSkeleton } from "./LoadingState";
import { monthBoundaries } from "./timeline-view";

type Props = {
	date: Date;
	days: number[] | null;
	onDate: (date: Date) => void;
	onClose: (restoreFocus?: boolean) => void;
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
	const previousMonth = useRef<HTMLButtonElement>(null);
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
		const anchor = root.current?.parentElement;
		if (!anchor) return;
		previousMonth.current?.focus({ preventScroll: true });
		const onPointerDown = (event: PointerEvent) => {
			if (event.target instanceof Node && !anchor.contains(event.target))
				onClose(false);
		};
		const onFocusOut = (event: FocusEvent) => {
			if (
				event.relatedTarget instanceof Node &&
				!anchor.contains(event.relatedTarget)
			)
				onClose(false);
		};
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				event.stopPropagation();
				onClose();
			}
		};
		document.addEventListener("pointerdown", onPointerDown);
		anchor.addEventListener("focusout", onFocusOut);
		anchor.addEventListener("keydown", onKeyDown);
		return () => {
			document.removeEventListener("pointerdown", onPointerDown);
			anchor.removeEventListener("focusout", onFocusOut);
			anchor.removeEventListener("keydown", onKeyDown);
		};
	}, [onClose]);
	const today = new Date();
	const todayStart = new Date(
		today.getFullYear(),
		today.getMonth(),
		today.getDate(),
	).getTime();
	const todayMonth = new Date(
		today.getFullYear(),
		today.getMonth(),
		1,
	).getTime();
	const moveMonth = (offset: -1 | 1, button: HTMLButtonElement) => {
		const next = new Date(month.getFullYear(), month.getMonth() + offset, 1);
		// The next-month button becomes disabled on reaching the current month.
		(next.getTime() >= todayMonth ? previousMonth.current : button)?.focus({
			preventScroll: true,
		});
		setMonth(next);
	};
	const dates = monthBoundaries(month).slice(0, -1);
	const offset = (month.getDay() + 6) % 7;
	return (
		<div
			ref={root}
			className="calendar-popover"
			role="dialog"
			aria-label="选择有记录的日期"
			tabIndex={-1}
		>
			<div className="calendar-month-heading">
				<button
					ref={previousMonth}
					type="button"
					aria-label="上个月"
					onClick={(event) => moveMonth(-1, event.currentTarget)}
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
					disabled={start >= todayMonth}
					onClick={(event) => moveMonth(1, event.currentTarget)}
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
