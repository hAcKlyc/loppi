export type UsageSeries = {
	requests: number;
	count: number;
	input_tokens: number;
	output_tokens: number;
	cached_tokens: number;
	cost_nano: number;
	incomplete: number;
};
export type UsageDay = {
	started_ms: number;
	covered: boolean;
	partial: boolean;
	asr: UsageSeries;
	llm: UsageSeries;
};
export type UsageStatistics = {
	started_ms: number;
	currency: string;
	days: UsageDay[];
};
export type UsageMetric =
	| "count"
	| "input_tokens"
	| "output_tokens"
	| "cached_tokens"
	| "cost_nano";

// Calendar arithmetic preserves local midnight across DST; never subtract N*24h.
export function usageBoundaries(days: 7 | 30 | 90, now = new Date()): number[] {
	const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
	start.setDate(start.getDate() - days + 1);
	return Array.from({ length: days + 1 }, (_, offset) => {
		const date = new Date(start);
		date.setDate(date.getDate() + offset);
		return date.getTime();
	});
}
export function usageNumber(value: number, metric: UsageMetric): string {
	if (metric === "cost_nano") {
		if (value > 0 && value < 10000) return "<¥0.00001";
		return `¥${(value / 1e9).toLocaleString("zh-CN", { maximumFractionDigits: 5, minimumFractionDigits: 2 })}`;
	}
	return value.toLocaleString("zh-CN");
}
export function usageAxis(value: number, money = false): string {
	if (money) return usageNumber(value, "cost_nano");
	if (value >= 1e6) return `${+(value / 1e6).toFixed(1)}M`;
	if (value >= 1000) return `${+(value / 1000).toFixed(1)}k`;
	return `${Math.round(value)}`;
}
