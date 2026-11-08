import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { core, userError } from "./api";
import { LoadingSkeleton } from "./LoadingState";
import {
	type UsageStatistics as Statistics,
	type UsageDay,
	type UsageMetric,
	usageAxis,
	usageBoundaries,
	usageNumber,
} from "./usage-statistics";
import "./usage-statistics.css";

const metrics: Record<UsageMetric, { label: string; color: string }> = {
	count: { label: "次数", color: "var(--color-chart-usage)" },
	input_tokens: { label: "输入 Token", color: "var(--color-chart-usage)" },
	output_tokens: { label: "输出 Token", color: "var(--color-chart-output)" },
	cached_tokens: { label: "缓存 Token", color: "var(--color-chart-cached)" },
	cost_nano: { label: "成本", color: "var(--color-chart-cost)" },
};
const asrMetrics: UsageMetric[] = ["count", "cost_nano"];
const llmMetrics: UsageMetric[] = [
	"input_tokens",
	"output_tokens",
	"cached_tokens",
	"cost_nano",
];
const labelDate = (at: number) =>
	new Date(at).toLocaleDateString("zh-CN", {
		month: "numeric",
		day: "numeric",
	});

export function UsageChart({
	kind,
	days,
}: {
	kind: "asr" | "llm";
	days: UsageDay[];
}) {
	const available = kind === "asr" ? asrMetrics : llmMetrics;
	const [hidden, setHidden] = useState<UsageMetric[]>([]);
	const [hover, setHover] = useState<number | null>(null);
	const plot = useRef<SVGSVGElement>(null);
	const [plotWidth, setPlotWidth] = useState(900);
	useLayoutEffect(() => {
		const element = plot.current;
		if (!element) return;
		const measure = () => {
			const width = element.getBoundingClientRect().width;
			if (width > 0) setPlotWidth(width);
		};
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(element);
		return () => observer.disconnect();
	}, []);
	const titleId = useId();
	const detailId = useId();
	const visible = available.filter((metric) => !hidden.includes(metric));
	const moneyVisible = visible.includes("cost_nano");
	const unitsVisible = visible.some((metric) => metric !== "cost_nano");
	const max = (money: boolean) =>
		Math.max(
			money ? 1000000 : 1,
			...days.flatMap((day) =>
				visible
					.filter((metric) => (metric === "cost_nano") === money)
					.map((metric) => day[kind][metric]),
			),
		) * 1.15;
	const maxUnits = Math.ceil(max(false) / 4) * 4;
	const maxMoney = max(true);
	// SVG units follow logical pixels, so labels retain their token font size.
	const plotSpan = Math.max(1, plotWidth - 150);
	const plotRight = 64 + plotSpan;
	const x = (i: number) => 64 + (i * plotSpan) / Math.max(1, days.length - 1);
	const y = (v: number, money: boolean) =>
		208 - (v / (money ? maxMoney : maxUnits)) * 160;
	const path = (metric: UsageMetric) => {
		let connected = false;
		return days
			.map((day, i) => {
				if (!day.covered) {
					connected = false;
					return "";
				}
				const command = `${connected ? "L" : "M"}${x(i)},${y(day[kind][metric], metric === "cost_nano")}`;
				connected = true;
				return command;
			})
			.join(" ");
	};
	const day = hover === null ? null : days[Math.min(hover, days.length - 1)];
	const incomplete = days.reduce(
		(sum, entry) => sum + entry[kind].incomplete,
		0,
	);
	const requests = days.reduce((sum, entry) => sum + entry[kind].requests, 0);
	const nothingSelected = visible.length === 0;
	return (
		<section className="usage-chart panel-card" aria-labelledby={titleId}>
			<div className="usage-chart-heading">
				<div>
					<h2 id={titleId}>
						{kind === "asr" ? "语音转录使用量" : "LLM 使用量"}
					</h2>
					<p>
						{kind === "asr"
							? "次数按独立受理请求统计 · 成本按音频时长计算"
							: "缓存 Token 包含在输入 Token 中"}
					</p>
				</div>
				<fieldset
					className="usage-legend"
					aria-label={`${kind === "asr" ? "语音转录" : "LLM"}指标`}
				>
					{available.map((metric) => (
						<button
							key={metric}
							type="button"
							aria-pressed={!hidden.includes(metric)}
							onClick={() =>
								setHidden((old) =>
									old.includes(metric)
										? old.filter((item) => item !== metric)
										: [...old, metric],
								)
							}
						>
							<span style={{ background: metrics[metric].color }} />
							{metrics[metric].label}
						</button>
					))}
				</fieldset>
			</div>
			<div className="usage-plot-wrap">
				<svg
					ref={plot}
					viewBox={`0 0 ${plotWidth} 252`}
					role="img"
					aria-labelledby={titleId}
					aria-describedby={detailId}
					onPointerMove={(event) => {
						const rect = event.currentTarget.getBoundingClientRect();
						const px = ((event.clientX - rect.left) * plotWidth) / rect.width;
						setHover(
							Math.max(
								0,
								Math.min(
									days.length - 1,
									Math.round(((px - 64) / plotSpan) * (days.length - 1)),
								),
							),
						);
					}}
					onPointerLeave={() => setHover(null)}
				>
					<title>
						{kind === "asr"
							? "每日语音转录次数与估算成本"
							: "每日 Token 与估算成本"}
						，使用左右方向键查看日期
					</title>
					{[0, 1, 2, 3, 4].map((step) => {
						const value = step / 4;
						return (
							<g key={step} className="usage-grid">
								<line
									x1="64"
									x2={plotRight}
									y1={208 - value * 160}
									y2={208 - value * 160}
								/>
								{unitsVisible && (
									<text x="54" y={212 - value * 160} textAnchor="end">
										{usageAxis(maxUnits * value)}
									</text>
								)}
								{moneyVisible && (
									<text x={plotRight + 12} y={212 - value * 160}>
										{usageAxis(maxMoney * value, true)}
									</text>
								)}
							</g>
						);
					})}
					{visible.map((metric) => (
						<path
							key={metric}
							d={path(metric)}
							fill="none"
							stroke={metrics[metric].color}
							strokeWidth="var(--stroke-chart)"
							strokeLinecap="round"
							strokeLinejoin="round"
						/>
					))}
					{days.filter((entry) => entry.covered).length === 1 &&
						days.map(
							(entry, i) =>
								entry.covered &&
								visible.map((metric) => (
									<circle
										key={`${entry.started_ms}-${metric}`}
										cx={x(i)}
										cy={y(entry[kind][metric], metric === "cost_nano")}
										r="3"
										fill={metrics[metric].color}
									/>
								)),
						)}
					{days.map(
						(entry, i) =>
							(i === 0 ||
								i === days.length - 1 ||
								i % Math.ceil((days.length - 1) / 5) === 0) && (
								<text
									key={entry.started_ms}
									x={x(i)}
									y="238"
									textAnchor="middle"
									className="usage-date"
								>
									{labelDate(entry.started_ms)}
								</text>
							),
					)}
					{day && hover !== null && !nothingSelected && (
						<g>
							<line
								x1={x(Math.min(hover, days.length - 1))}
								x2={x(Math.min(hover, days.length - 1))}
								y1="40"
								y2="208"
								className="usage-crosshair"
							/>
							{day.covered &&
								visible.map((metric) => (
									<circle
										key={metric}
										cx={x(Math.min(hover, days.length - 1))}
										cy={y(day[kind][metric], metric === "cost_nano")}
										r="4"
										fill={metrics[metric].color}
										stroke="var(--color-surface)"
										strokeWidth="var(--stroke-icon-emphasis)"
									/>
								))}
						</g>
					)}
				</svg>
				<input
					className="usage-day-slider"
					style={{ left: 64, width: plotSpan }}
					type="range"
					min={0}
					max={days.length - 1}
					value={Math.min(hover ?? days.length - 1, days.length - 1)}
					aria-label="查看每日统计，使用左右方向键切换日期"
					aria-describedby={detailId}
					onFocus={() => setHover(days.length - 1)}
					onBlur={() => setHover(null)}
					onChange={(event) => setHover(Number(event.target.value))}
				/>
				{nothingSelected ? (
					<div className="usage-chart-empty">选择指标查看趋势</div>
				) : (
					requests === 0 && (
						<div className="usage-chart-empty">这段时间暂无使用记录</div>
					)
				)}
				{day && !nothingSelected && (
					<div className="usage-tooltip" aria-hidden="true">
						<strong>
							{labelDate(day.started_ms)}
							{new Date(day.started_ms).toDateString() ===
							new Date().toDateString()
								? " · 截至当前"
								: ""}
						</strong>
						{!day.covered ? (
							<span>尚未开始统计</span>
						) : (
							<>
								{visible.map((metric) => (
									<span key={metric}>
										<i style={{ background: metrics[metric].color }} />
										{metrics[metric].label}
										<b>{usageNumber(day[kind][metric], metric)}</b>
									</span>
								))}
								{(day.partial || day[kind].incomplete > 0) && (
									<small>部分数据，显示已知用量与成本</small>
								)}
							</>
						)}
					</div>
				)}
			</div>
			<div id={detailId} className="usage-accessible" aria-live="polite">
				{day
					? `${labelDate(day.started_ms)}：${day.covered ? visible.map((metric) => `${metrics[metric].label} ${usageNumber(day[kind][metric], metric)}`).join("，") : "尚未开始统计"}${day.partial || day[kind].incomplete ? "，部分数据" : ""}`
					: "聚焦图表后可用左右方向键查看每日数值。"}
			</div>
			{incomplete > 0 && (
				<p className="usage-incomplete">
					{incomplete} 笔调用的用量或价格待确认，图中成本为已知部分。
				</p>
			)}
		</section>
	);
}

export function UsageStatisticsView({ refresh }: { refresh: number }) {
	const [range, setRange] = useState<7 | 30 | 90>(30);
	const [statistics, setStatistics] = useState<Statistics | null>(null);
	const [error, setError] = useState("");
	const [loadedRange, setLoadedRange] = useState<number | null>(null);
	const requestGeneration = useRef<object | null>(null);
	useEffect(() => {
		const generation = { range, refresh };
		requestGeneration.current = generation;
		let loading = false;
		setError("");
		const load = async () => {
			if (loading) return;
			loading = true;
			try {
				const response = await core<{
					type: "usage_statistics";
					id: string;
					statistics: Statistics;
				}>({ type: "usage_statistics", boundaries: usageBoundaries(range) });
				if (requestGeneration.current === generation) {
					setStatistics(response.statistics);
					setLoadedRange(range);
					setError("");
				}
			} catch (failure) {
				if (requestGeneration.current === generation)
					setError(userError(failure));
			} finally {
				loading = false;
			}
		};
		void load();
		const timer = setInterval(() => void load(), 30000);
		return () => {
			requestGeneration.current = null;
			clearInterval(timer);
		};
	}, [range, refresh]);
	return (
		<div className="usage-statistics">
			<div className="usage-toolbar">
				<fieldset className="usage-range" aria-label="统计时间范围">
					{([7, 30, 90] as const).map((days) => (
						<button
							type="button"
							key={days}
							aria-pressed={range === days}
							onClick={() => setRange(days)}
						>
							近 {days} 天
						</button>
					))}
				</fieldset>
				<span>按日统计 · 本地时间</span>
			</div>
			{error && (
				<p className="usage-incomplete" role="alert">
					统计暂时无法更新：{error}，可点击右上角刷新。
				</p>
			)}
			{statistics && loadedRange !== range && !error && (
				<LoadingSkeleton label="正在加载统计范围" rows={6} />
			)}
			{statistics ? (
				<div className="usage-charts" hidden={loadedRange !== range}>
					<UsageChart kind="asr" days={statistics.days} />
					<UsageChart kind="llm" days={statistics.days} />
					<p className="usage-footnote">
						从 {new Date(statistics.started_ms).toLocaleString("zh-CN")}{" "}
						开始统计，之前的用量不补算。成本按调用时记录的人民币单价估算，未抵扣套餐或赠送额度。
					</p>
				</div>
			) : (
				!error && <LoadingSkeleton label="正在加载统计数据" rows={6} />
			)}
		</div>
	);
}
