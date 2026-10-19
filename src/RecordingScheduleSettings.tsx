import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import {
	type CoreRequest,
	type CoreResponse,
	type ServiceState,
	userError,
} from "./api";
import { Switch } from "./Controls";
import { LoadingSkeleton } from "./LoadingState";
import {
	nextScheduleAction,
	scheduleMinute,
	scheduleTime,
} from "./recording-schedule";

type Draft = {
	enabled: boolean;
	start: string;
	stop: string;
	revision?: number;
};
export function RecordingScheduleSettings({
	state,
	refresh,
}: {
	state: ServiceState | null;
	refresh: () => Promise<void>;
}) {
	const [draft, setDraft] = useState<Draft | null>(null);
	const [sending, setSending] = useState(false);
	const [error, setError] = useState("");
	const pending = useRef<{ id: string; request: CoreRequest } | null>(null);
	const saved = state?.recording_schedule;
	const value = draft ?? {
		enabled: saved?.enabled ?? false,
		start: scheduleTime(saved?.start_minute ?? 480),
		stop: scheduleTime(saved?.stop_minute ?? 1380),
		revision: state?.revision,
	};
	const edit = (patch: Partial<Draft>) => {
		setDraft({ ...value, ...patch });
		setError("");
	};
	const save = async () => {
		if (!state || sending) return;
		const start = scheduleMinute(value.start),
			stop = scheduleMinute(value.stop);
		if (start === null || stop === null || start === stop) {
			setError("开始和停止时间不能相同，请选择有效时间");
			return;
		}
		pending.current ??= {
			id: crypto.randomUUID(),
			request: {
				type: "set_recording_schedule",
				enabled: value.enabled,
				start_minute: start,
				stop_minute: stop,
				expected_revision: value.revision ?? state.revision,
			},
		};
		setSending(true);
		setError("");
		try {
			const result = await invoke<CoreResponse>("core_request", {
				request: pending.current.request,
				requestId: pending.current.id,
			});
			pending.current = null;
			if (result.type === "error") {
				setError(userError(`${result.code}: ${result.message}`));
				if (result.code === "revision_conflict")
					setDraft({ ...value, revision: undefined });
			} else {
				setDraft(null);
			}
			await refresh().catch(() =>
				setError("设置结果已确认，状态刷新失败，请刷新后查看"),
			);
		} catch {
			setError("保存结果待确认，请重试确认同一次保存");
		} finally {
			setSending(false);
		}
	};
	if (!saved) {
		return (
			<div className="panel-card setting-panel recording-schedule-panel">
				<LoadingSkeleton label="正在读取定时录制设置" rows={2} compact />
			</div>
		);
	}
	const locked = sending || pending.current !== null;
	return (
		<div className="panel-card setting-panel recording-schedule-panel">
			<div className="setting-row">
				<div>
					<strong>定时录制</strong>
					<p>每天到点停止或开始，仍可随时手动控制</p>
				</div>
				<Switch
					label="定时录制"
					checked={value.enabled}
					disabled={locked}
					onChange={() => edit({ enabled: !value.enabled })}
				/>
			</div>
			<div className="schedule-times">
				<label>
					每天停止录制
					<input
						type="time"
						value={value.stop}
						disabled={locked}
						onChange={(e) => edit({ stop: e.target.value })}
					/>
				</label>
				<label>
					每天开始录制
					<input
						type="time"
						value={value.start}
						disabled={locked}
						onChange={(e) => edit({ start: e.target.value })}
					/>
				</label>
			</div>
			<p className="schedule-note">
				跟随电脑本地时间。保存后从下一个时间点开始执行，不改变当前录制状态。
			</p>
			<p className="schedule-note">{nextScheduleAction(saved)}</p>
			{error && (
				<p role="alert" className="schedule-error">
					{error}
				</p>
			)}
			{draft && (
				<div className="setting-actions">
					<button
						type="button"
						className="quiet-button"
						disabled={sending || pending.current !== null}
						onClick={() => {
							setDraft(null);
							setError("");
						}}
					>
						取消
					</button>
					<button
						type="button"
						disabled={sending || !saved}
						onClick={() => void save()}
					>
						{sending ? "保存中…" : pending.current ? "重试确认" : "保存"}
					</button>
				</div>
			)}
		</div>
	);
}
