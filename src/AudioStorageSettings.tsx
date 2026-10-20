import { useState } from "react";
import type { ServiceState } from "./api";
import { Modal } from "./Controls";
import { LoadBoundary } from "./LoadingState";
import { useSettingMutation } from "./useSettingMutation";

const GIB = 1_073_741_824;
export function AudioStorageSettings({
	state,
	error,
	refresh,
}: {
	state: ServiceState | null;
	error?: string;
	refresh: () => Promise<void>;
}) {
	const [draft, setDraft] = useState<{
		value: string;
		revision?: number;
	} | null>(null);
	const mutation = useSettingMutation(refresh);
	const locked = mutation.busy || mutation.uncertain;
	const close = () => {
		setDraft(null);
		mutation.setError("");
	};
	const save = async () => {
		if (!draft || !state) return;
		const value = Number(draft.value);
		if (!Number.isFinite(value) || value < 0.0625 || value > 1024) {
			mutation.setError("请输入 0.0625 至 1024 GB 的上限");
			return;
		}
		const result = await mutation.submit({
			type: "set_audio_limit",
			limit_bytes: Math.round(value * GIB),
			expected_revision: draft.revision ?? state.revision,
		});
		if (result === "saved") setDraft(null);
		else if (result === "conflict") setDraft({ ...draft, revision: undefined });
	};
	return (
		<>
			<LoadBoundary
				ready={state !== null}
				error={error}
				retry={() => void refresh()}
				label="音频存储上限"
				rows={1}
			>
				<div className="setting-row">
					<div>
						<strong>
							音频存储上限 ·{" "}
							{state && +(state.audio_limit_bytes / GIB).toFixed(4)} GB
						</strong>
						<p>
							已用 {((state?.audio_used_bytes ?? 0) / GIB).toFixed(2)} GB
							{state?.storage_paused
								? " · 录音已暂停"
								: state?.storage_warning
									? " · 空间即将不足"
									: ""}
						</p>
						<p>达到上限后暂停录音；释放空间或提高上限后，请手动继续录音。</p>
					</div>
					<button
						type="button"
						className="quiet-button"
						disabled={!state}
						onClick={() => {
							mutation.setError("");
							setDraft({
								value: String((state?.audio_limit_bytes ?? 0) / GIB),
								revision: state?.revision,
							});
						}}
					>
						修改上限
					</button>
				</div>
			</LoadBoundary>
			{!draft && mutation.error && (
				<p role="alert" className="schedule-error">
					{mutation.error}
				</p>
			)}
			{draft && (
				<Modal title="修改音频存储上限" busy={locked} onClose={close}>
					<form
						onSubmit={(event) => {
							event.preventDefault();
							void save();
						}}
					>
						<label>
							音频存储上限（GB）
							<input
								type="number"
								min="0.0625"
								max="1024"
								step="any"
								required
								value={draft.value}
								disabled={locked}
								onChange={(event) => {
									setDraft({ ...draft, value: event.target.value });
									mutation.setError("");
								}}
							/>
						</label>
						<p className="schedule-note">
							范围为 0.0625 至 1024 GB。不会删除已有音频。
						</p>
						{state && Number(draft.value) * GIB <= state.audio_used_bytes && (
							<p className="warning-text">
								新上限不高于当前用量，保存后将暂停录音。
							</p>
						)}
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
									? "保存中…"
									: mutation.uncertain
										? "确认上次结果"
										: "保存上限"}
							</button>
						</div>
					</form>
				</Modal>
			)}
		</>
	);
}
