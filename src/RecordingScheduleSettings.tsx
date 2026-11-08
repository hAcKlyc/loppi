import { useRef, useState } from "react";
import type { ServiceState } from "./api";
import { Button, Modal, Switch } from "./Controls";
import { LoadBoundary } from "./LoadingState";
import {
	ALL_SCHEDULE_WEEKDAYS,
	nextScheduleAction,
	SCHEDULE_WEEKDAYS,
	scheduleMinute,
	scheduleSummary,
	scheduleTime,
} from "./recording-schedule";
import { useSettingMutation } from "./useSettingMutation";

type Draft = {
	start: string;
	stop: string;
	weekdays: number;
	revision?: number;
};

export function RecordingScheduleSettings({
	state,
	refresh,
	error: loadError,
}: {
	state: ServiceState | null;
	refresh: () => Promise<void>;
	error?: string;
}) {
	const [draft, setDraft] = useState<Draft | null>(null);
	const firstWeekday = useRef<HTMLButtonElement>(null);
	const mutation = useSettingMutation(refresh);
	const saved = state?.recording_schedule;
	const locked = mutation.busy || mutation.uncertain;
	const close = () => {
		setDraft(null);
		mutation.setError("");
	};
	const toggle = () => {
		if (!state || !saved) return;
		void mutation.submit({
			type: "set_recording_schedule",
			enabled: !saved.enabled,
			start_minute: saved.start_minute,
			stop_minute: saved.stop_minute,
			expected_revision: state.revision,
		});
	};
	const save = async () => {
		if (!draft?.weekdays || !state || !saved) return;
		const start = scheduleMinute(draft.start),
			stop = scheduleMinute(draft.stop);
		if (start === null || stop === null || start === stop) {
			mutation.setError("开始和停止时间不能相同，请选择有效时间");
			return;
		}
		const result = await mutation.submit({
			type: "set_recording_schedule",
			enabled: saved.enabled,
			start_minute: start,
			stop_minute: stop,
			weekday_mask: draft.weekdays,
			expected_revision: draft.revision ?? state.revision,
		});
		if (result === "saved") {
			setDraft(null);
		} else if (result === "conflict") {
			// An explicit retry after rejection uses refreshed state. Unknown results
			// always reuse the original request and revision inside the mutation hook.
			setDraft({ ...draft, revision: undefined });
		}
	};
	const start = draft && scheduleMinute(draft.start);
	const stop = draft && scheduleMinute(draft.stop);
	const overnight = start !== null && stop !== null && stop < start;
	return (
		<div className="panel-card setting-panel recording-schedule-panel">
			<LoadBoundary
				ready={!!saved}
				error={loadError}
				retry={() => void refresh()}
				label="定时录制"
				rows={2}
			>
				{saved && (
					<>
						<div className="setting-row">
							<div>
								<div className="setting-heading">
									<strong>定时录制</strong>
									<span className="setting-status" role="status">
										{mutation.busy && !draft
											? "正在应用…"
											: mutation.uncertain && !draft
												? "操作结果待确认"
												: nextScheduleAction(saved)}
									</span>
								</div>
								<p>到点开始或停止，仍可随时手动控制</p>
							</div>
							<Switch
								label="定时录制"
								checked={saved.enabled}
								disabled={locked || !!draft}
								onChange={toggle}
							/>
						</div>
						<div className="setting-row">
							<div>
								<strong>{scheduleSummary(saved)}</strong>
								<p>
									跟随电脑本地时间，从下一个时间点执行，不改变当前录制状态。
								</p>
							</div>
							<Button
								type="button"
								tone="quiet"
								disabled={locked}
								onClick={() => {
									mutation.setError("");
									setDraft({
										start: scheduleTime(saved.start_minute),
										stop: scheduleTime(saved.stop_minute),
										weekdays: saved.weekday_mask ?? ALL_SCHEDULE_WEEKDAYS,
										revision: state?.revision,
									});
								}}
							>
								编辑计划
							</Button>
						</div>
					</>
				)}
			</LoadBoundary>
			{!draft && mutation.error && (
				<p role="alert" className="schedule-error">
					{mutation.error}
				</p>
			)}
			{!draft && mutation.uncertain && (
				<Button
					type="button"
					tone="quiet"
					disabled={mutation.busy}
					onClick={() => void mutation.confirm()}
				>
					确认上次结果
				</Button>
			)}
			{draft && (
				<Modal
					title="编辑录制计划"
					busy={locked}
					onClose={close}
					initialFocusRef={firstWeekday}
				>
					<form
						onSubmit={(event) => {
							event.preventDefault();
							void save();
						}}
					>
						<fieldset className="schedule-weekdays" aria-label="重复日期">
							<span>重复</span>
							<div className="schedule-day-list">
								{SCHEDULE_WEEKDAYS.map((day, index) => {
									const bit = 1 << index;
									return (
										<button
											key={day}
											ref={index === 0 ? firstWeekday : undefined}
											className="schedule-day"
											type="button"
											aria-label={`周${day}`}
											aria-pressed={Boolean(draft.weekdays & bit)}
											disabled={locked}
											onClick={() => {
												setDraft({ ...draft, weekdays: draft.weekdays ^ bit });
												mutation.setError("");
											}}
										>
											{day}
										</button>
									);
								})}
							</div>
						</fieldset>
						{!draft.weekdays && (
							<p className="schedule-error" role="alert">
								至少选择一天
							</p>
						)}
						<div className="schedule-times">
							<label>
								开始
								<input
									type="time"
									required
									value={draft.start}
									disabled={locked}
									onChange={(event) => {
										setDraft({ ...draft, start: event.target.value });
										mutation.setError("");
									}}
								/>
							</label>
							<label>
								停止
								<input
									type="time"
									required
									value={draft.stop}
									disabled={locked}
									onChange={(event) => {
										setDraft({ ...draft, stop: event.target.value });
										mutation.setError("");
									}}
								/>
								{overnight && <span className="setting-status">次日</span>}
							</label>
						</div>
						<p className="schedule-note">
							{saved?.enabled
								? "保存后从下一个定时点执行。"
								: "定时录制已关闭，保存后保留关闭状态。"}
						</p>
						{mutation.error && (
							<p role="alert" className="schedule-error">
								{mutation.error}
							</p>
						)}
						<div className="modal-actions">
							<Button
								type="button"
								tone="quiet"
								disabled={locked}
								onClick={close}
							>
								取消
							</Button>
							<Button
								type="submit"
								tone="primary"
								disabled={mutation.busy || !draft.weekdays}
							>
								{mutation.busy
									? "正在应用…"
									: mutation.uncertain
										? "确认上次结果"
										: "保存计划"}
							</Button>
						</div>
					</form>
				</Modal>
			)}
		</div>
	);
}
