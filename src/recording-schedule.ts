import type { RecordingSchedule } from "./api";

export function scheduleTime(minute: number): string {
	return `${String(Math.floor(minute / 60)).padStart(2, "0")}:${String(minute % 60).padStart(2, "0")}`;
}
export function scheduleMinute(value: string): number | null {
	if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(value)) return null;
	const [hour, minute] = value.split(":").map(Number);
	return hour * 60 + minute;
}
export function nextScheduleAction(schedule?: RecordingSchedule): string {
	if (!schedule?.enabled) return "定时录制未启用";
	if (schedule.error) return "暂时无法确定下次时间，请检查系统时间";
	if (schedule.next_at_ms == null) return "正在读取下次时间";
	const at = new Date(schedule.next_at_ms);
	return `下一次 ${at.toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false })} ${schedule.next_action === "start" ? "开始" : "停止"}录制`;
}
