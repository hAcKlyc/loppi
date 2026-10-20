import { useState } from "react";
import type { ServiceState } from "./api";
import { Modal, Switch } from "./Controls";
import { LoadBoundary } from "./LoadingState";
import {
	nextScheduleAction,
	scheduleMinute,
	scheduleTime,
} from "./recording-schedule";
import { useSettingMutation } from "./useSettingMutation";

type Draft = { start: string; stop: string; revision?: number };

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
		if (!draft || !state || !saved) return;
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
								<p>每天到点开始或停止，仍可随时手动控制</p>
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
								<strong>
									每天 {scheduleTime(saved.start_minute)} 开始，
									{scheduleTime(saved.stop_minute)} 停止
								</strong>
								<p>
									跟随电脑本地时间，从下一个时间点执行，不改变当前录制状态。
								</p>
							</div>
							<button
								type="button"
								className="quiet-button"
								disabled={locked}
								onClick={() => {
									mutation.setError("");
									setDraft({
										start: scheduleTime(saved.start_minute),
										stop: scheduleTime(saved.stop_minute),
										revision: state?.revision,
									});
								}}
							>
								编辑时间
							</button>
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
				<button
					type="button"
					className="quiet-button"
					disabled={mutation.busy}
					onClick={() => void mutation.confirm()}
				>
					确认上次结果
				</button>
			)}
			{draft && (
				<Modal title="编辑录制时间" busy={locked} onClose={close}>
					<form
						onSubmit={(event) => {
							event.preventDefault();
							void save();
						}}
					>
						<div className="schedule-times">
							<label>
								每天开始录制
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
								每天停止录制
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
							</label>
						</div>
						<p className="schedule-note">
							{saved?.enabled
								? "保存后从下一个时间点执行，不改变当前录制状态。"
								: "定时录制已关闭。保存时间不会开启定时录制。"}
						</p>
						{mutation.error && (
							<p role="alert" className="schedule-error">
								{mutation.error}
							</p>
						)}
						<div className="modal-actions">
							<button
								type="button"
								className="quiet-button"
								disabled={locked}
								onClick={close}
							>
								取消
							</button>
							<button
								type="submit"
								className="primary-button"
								disabled={mutation.busy}
							>
								{mutation.busy
									? "正在应用…"
									: mutation.uncertain
										? "确认上次结果"
										: "保存时间"}
							</button>
						</div>
					</form>
				</Modal>
			)}
		</div>
	);
}
