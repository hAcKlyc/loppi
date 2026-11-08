import type { RecordingSchedule } from "./api";

export const SCHEDULE_WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];
export const ALL_SCHEDULE_WEEKDAYS = 0x7f;

export function scheduleTime(minute: number): string {
	return `${String(Math.floor(minute / 60)).padStart(2, "0")}:${String(minute % 60).padStart(2, "0")}`;
}
export function scheduleMinute(value: string): number | null {
	if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(value)) return null;
	const [hour, minute] = value.split(":").map(Number);
	return hour * 60 + minute;
}
export function scheduleSummary(schedule: RecordingSchedule): string {
	const mask = schedule.weekday_mask ?? ALL_SCHEDULE_WEEKDAYS;
	const days =
		mask === ALL_SCHEDULE_WEEKDAYS
			? "每天"
			: mask === 0x1f
				? "周一至周五"
				: mask === 0x60
					? "周末"
					: `周${SCHEDULE_WEEKDAYS.filter((_, index) => mask & (1 << index)).join("、")}`;
	const stop = schedule.stop_minute < schedule.start_minute ? "次日 " : "";
	return `${days} · ${scheduleTime(schedule.start_minute)}–${stop}${scheduleTime(schedule.stop_minute)}`;
}
export function nextScheduleAction(schedule?: RecordingSchedule): string {
	if (!schedule?.enabled) return "定时录制未启用";
	if (schedule.error) return "暂时无法确定下次时间，请检查系统时间";
	if (schedule.next_at_ms == null) return "正在读取下次时间";
	const at = new Date(schedule.next_at_ms);
	return `下一次 ${at.toLocaleString("zh-CN", { weekday: "short", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false })} ${schedule.next_action === "start" ? "开始" : "停止"}录制`;
}
