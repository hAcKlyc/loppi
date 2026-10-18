import { useId, useRef, useState } from "react";
import { userError } from "./api";
import { type CaptureStatusAction, captureStatusView } from "./capture-status";
import {
	EntryRequests,
	type EntrySnapshot,
	entryAppearance,
} from "./entry-api";
import { LoadBoundary } from "./LoadingState";
import { Rabbit } from "./Rabbit";
import "./capture-status.css";

const labels: Record<CaptureStatusAction, string> = {
	pause: "暂停",
	resume: "继续",
	start: "开始",
	settings: "设置",
	retry: "重试",
};

function ControlIcon({ pause }: { pause: boolean }) {
	return (
		<svg viewBox="0 0 20 20" fill="none" aria-hidden="true">
			{pause ? (
				<path
					d="M7 5v10M13 5v10"
					stroke="currentColor"
					strokeWidth="2"
					strokeLinecap="round"
				/>
			) : (
				<path d="m7 4 9 6-9 6Z" fill="currentColor" />
			)}
		</svg>
	);
}

export function useCaptureControl({
	snapshot,
	onRefresh,
	onSettings,
}: {
	snapshot: EntrySnapshot | null;
	onRefresh: () => Promise<void>;
	onSettings: () => void;
}) {
	const view = captureStatusView(snapshot);
	const requests = useRef(new EntryRequests());
	const active = useRef(false);
	const [busy, setBusy] = useState(false);
	const [uncertain, setUncertain] = useState(false);
	const [feedback, setFeedback] = useState<string | null>(null);
	const perform = async () => {
		if (active.current) return;
		if (!uncertain && view.action === "settings") {
			onSettings();
			return;
		}
		if (!uncertain && !view.action) return;
		active.current = true;
		setBusy(true);
		setFeedback(null);
		try {
			if (!uncertain && view.action === "retry") {
				await onRefresh();
			} else {
				const result = await requests.current.send("control", Date.now(), null);
				setUncertain(result.outcome === "uncertain");
				if (result.outcome === "uncertain")
					setFeedback("操作结果待确认，请点击「确认」核对");
				else if (result.outcome === "rejected")
					setFeedback(userError(result.message ?? "操作未完成，请重试"));
				// A receipt acknowledges intent. Only a fresh host snapshot can
				// change the displayed recording or paused state.
				await onRefresh();
			}
		} catch (cause) {
			setUncertain(requests.current.isUncertain("control"));
			setFeedback(userError(cause));
		} finally {
			active.current = false;
			setBusy(false);
		}
	};
	const actionLabel = busy
		? "处理中"
		: uncertain
			? "确认"
			: view.action
				? labels[view.action]
				: "请稍候";
	return { busy, uncertain, feedback, actionLabel, perform };
}

export function CaptureStatus({
	loadError,
	onRetry,
	snapshot,
	control,
}: {
	snapshot: EntrySnapshot | null;
	loadError?: string;
	onRetry?: () => void;
	control: ReturnType<typeof useCaptureControl>;
}) {
	const view = captureStatusView(snapshot);
	const { busy, uncertain, feedback, actionLabel, perform } = control;
	const appearance = entryAppearance(snapshot);
	const statusId = useId();
	if (!snapshot)
		return (
			<div className="capture-status-wrap">
				<LoadBoundary
					ready={false}
					error={loadError}
					retry={onRetry}
					label="记录状态"
					rows={1}
					compact
				>
					{null}
				</LoadBoundary>
			</div>
		);
	return (
		<div className="capture-status-wrap">
			<section
				className={`capture-status ${view.tone}`}
				aria-label="声音记录状态"
				title={view.detail || undefined}
			>
				<button
					type="button"
					className="capture-status-control"
					disabled={busy || (!view.action && !uncertain)}
					onClick={() => void perform()}
					aria-busy={busy}
					aria-label={
						uncertain
							? "确认上一次记录操作"
							: `${actionLabel}${["pause", "resume", "start"].includes(view.action ?? "") ? "记录" : ""}`
					}
					title={actionLabel}
					aria-describedby={statusId}
				>
					<span className="capture-status-rabbit">
						<Rabbit state={appearance} action={null} />
					</span>
					<span className="capture-status-copy">
						<strong>{view.label}</strong>
						<span>{view.subtitle}</span>
					</span>
					<span
						className="capture-control-overlay"
						data-persistent={busy || uncertain}
						aria-hidden="true"
					>
						{!busy &&
						!uncertain &&
						["pause", "resume", "start"].includes(view.action ?? "") ? (
							<>
								<ControlIcon pause={view.action === "pause"} />
								<span>{actionLabel}记录</span>
							</>
						) : (
							actionLabel
						)}
					</span>
				</button>
				<span
					id={statusId}
					className="capture-status-announcement"
					role="status"
					aria-live="polite"
				>
					{view.label}，{view.subtitle}
				</span>
			</section>
			{feedback && (
				<div className="capture-status-feedback" role="alert">
					{feedback}
				</div>
			)}
		</div>
	);
}
