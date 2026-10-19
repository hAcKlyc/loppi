import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import { core, type ServiceState, userError } from "./api";
import { LoadBoundary } from "./LoadingState";
import {
	type ModelState,
	modelErrors,
	modelLabels,
	type PrivacyStatus,
} from "./setup";

export function ModelPreparation({
	models,
	error,
	onRefresh,
	compact = false,
}: {
	models: ModelState | null;
	error?: string;
	onRefresh: () => Promise<void>;
	compact?: boolean;
}) {
	const [busy, setBusy] = useState(false);
	const [failure, setFailure] = useState("");
	const active = useRef(false);
	const phases = models ? [models.phase, models.speaker_phase] : [];
	const running = phases.some(
		(p) => !["missing", "failed", "ready"].includes(p),
	);
	const needed = phases.some((p) => ["missing", "failed"].includes(p));
	const prepare = async () => {
		if (active.current) return;
		active.current = true;
		setBusy(true);
		setFailure("");
		try {
			await core({ type: "prepare_models" });
		} catch (e) {
			setFailure(`准备结果待确认：${userError(e)}。请刷新状态后再操作。`);
		} finally {
			await onRefresh();
			active.current = false;
			setBusy(false);
		}
	};
	if (compact) {
		return (
			<LoadBoundary
				ready={models !== null}
				error={error}
				retry={() => void onRefresh()}
				label="本地模型"
				rows={1}
				compact
			>
				<div className="setup-model-compact">
					{models?.phase === "ready" ? (
						<span className="setup-ready" role="status">
							✓ 已就绪
						</span>
					) : (
						<button
							className="quiet-button"
							type="button"
							disabled={busy || running}
							onClick={() => void prepare()}
							aria-live="polite"
						>
							{busy
								? "正在确认…"
								: running
									? `${modelLabels[models?.phase ?? ""] ?? "正在准备…"}${models?.phase === "downloading" ? ` · ${models.progress_percent}%` : ""}`
									: models?.phase === "failed"
										? "重试安装"
										: "下载并安装 ↓"}
						</button>
					)}
					{models?.error && (
						<p className="error" role="alert">
							{modelErrors[models.error] ?? "组件准备失败，请重试"}
						</p>
					)}
					{failure && (
						<p className="error" role="alert">
							{failure}{" "}
							<button
								className="setup-text-button"
								type="button"
								onClick={() => void onRefresh()}
							>
								刷新状态
							</button>
						</p>
					)}
					{models?.phase === "ready" && models.speaker_phase !== "ready" && (
						<details className="setup-help">
							<summary>
								{models.speaker_phase === "failed"
									? "人物识别待重试"
									: models.speaker_phase === "missing"
										? "人物识别未安装"
										: "人物识别准备中"}
							</summary>
							<p>
								{modelLabels[models.speaker_phase]} · 不影响完成设置
								{models.speaker_error
									? ` · ${modelErrors[models.speaker_error] ?? "请重试"}`
									: ""}
							</p>
							{needed && (
								<button
									className="quiet-button"
									type="button"
									disabled={busy || running}
									onClick={() => void prepare()}
								>
									准备人物识别
								</button>
							)}
						</details>
					)}
				</div>
			</LoadBoundary>
		);
	}
	return (
		<LoadBoundary
			ready={models !== null}
			error={error}
			retry={() => void onRefresh()}
			label="本地组件"
			rows={1}
		>
			<div className="setting-row">
				<div>
					<strong>本地语音组件</strong>
					<p>在这台电脑上识别人声，生成初步文字。</p>
					<p aria-live="polite">
						{modelLabels[models?.phase ?? ""]}
						{models?.phase === "downloading"
							? ` · ${models.progress_percent}%`
							: ""}
					</p>
					{models?.error && (
						<p className="error">
							{modelErrors[models.error] ?? "组件准备失败，请重试"}
						</p>
					)}
					{models?.speaker_phase !== "ready" && (
						<p>
							人物识别组件（不影响完成设置）：
							{modelLabels[models?.speaker_phase ?? ""]}
							{models?.speaker_error
								? ` · ${modelErrors[models.speaker_error] ?? "请重试"}`
								: ""}
						</p>
					)}
					{failure && (
						<p className="error" role="alert">
							{failure}
						</p>
					)}
					{models?.phase === "failed" && (
						<p>完整组件会复用，未完成的下载会重新开始。</p>
					)}
				</div>
				{needed && (
					<button
						className="quiet-button"
						type="button"
						disabled={busy || running}
						onClick={() => void prepare()}
					>
						{busy
							? "正在确认…"
							: phases.includes("failed")
								? "重试"
								: "下载并安装"}
					</button>
				)}
			</div>
		</LoadBoundary>
	);
}
export function SourcePreparation({
	state,
	privacy,
	error,
	onRefresh,
	onActivity,
	compact = false,
}: {
	state: ServiceState | null;
	privacy: PrivacyStatus | null;
	error?: string;
	onRefresh: () => Promise<void>;
	onActivity?: (busy: boolean) => void;
	compact?: boolean;
}) {
	const [busy, setBusy] = useState<string | null>(null);
	const [failure, setFailure] = useState("");
	const active = useRef(false);
	const perform = async (label: string, action: () => Promise<unknown>) => {
		if (active.current) return;
		active.current = true;
		setBusy(label);
		onActivity?.(true);
		setFailure("");
		try {
			await action();
		} catch (e) {
			setFailure(userError(e));
		} finally {
			await onRefresh();
			active.current = false;
			setBusy(null);
			onActivity?.(false);
		}
	};
	const [sourceOptionsOpen, setSourceOptionsOpen] = useState(false);
	const chooseSource = (source: "microphone" | "system", enabled: boolean) =>
		perform(source, () =>
			core({
				type: "set_source",
				source,
				enabled,
				expected_revision: state?.revision,
			}),
		);
	return (
		<LoadBoundary
			ready={state !== null && privacy !== null}
			error={error}
			retry={() => void onRefresh()}
			label="声音来源"
			rows={2}
			compact={compact}
		>
			<div className={compact ? "setup-permission-controls" : undefined}>
				<div className={compact ? "setup-permission-buttons" : undefined}>
					{(["microphone", "system"] as const).map((source) => {
						const mic = source === "microphone";
						const enabled = mic
							? state?.microphone_enabled
							: state?.system_enabled;
						const ready = mic
							? privacy?.microphone === 3
							: privacy?.tap_required
								? privacy.tap_ready
								: privacy?.screen_and_system_audio === 1;
						const unavailable = mic
							? privacy?.microphone_available === false ||
								state?.microphone_phase === "unavailable"
							: state?.system_phase === "unavailable";
						const denied = mic && [1, 2].includes(privacy?.microphone ?? -1);
						const label = mic ? "麦克风" : "电脑声音";
						if (compact) {
							if (!enabled) return null;
							return (
								<div className="setup-permission-item" key={source}>
									{ready && !unavailable ? (
										<span className="setup-permission-ready" role="status">
											{label} ·{" "}
											{mic || !privacy?.tap_required ? "已授权" : "已准备"} ✓
										</span>
									) : (
										<button
											className="quiet-button"
											type="button"
											disabled={!!busy}
											onClick={() =>
												void perform(source, () =>
													invoke(
														denied || unavailable
															? "open_privacy_settings"
															: mic
																? "request_microphone_permission"
																: "request_system_audio_permission",
														{ source },
													),
												)
											}
										>
											{busy === source
												? "正在处理…"
												: denied || unavailable
													? `${label} · 系统设置 ↗`
													: `授权${label}`}
										</button>
									)}
									{(denied || unavailable) && (
										<p className="error">
											{unavailable
												? "来源暂不可用"
												: privacy?.microphone === 1
													? "访问受系统限制"
													: "尚未获得授权"}
										</p>
									)}
								</div>
							);
						}
						return (
							<div className="setting-row" key={source}>
								<div>
									<label className="setup-source">
										<input
											type="checkbox"
											checked={enabled ?? false}
											disabled={!!busy || !state}
											onChange={() =>
												void perform(source, () =>
													core({
														type: "set_source",
														source,
														enabled: !enabled,
														expected_revision: state?.revision,
													}),
												)
											}
										/>
										<strong>{label}</strong>
									</label>
									<p>{mic ? "你和身边的声音" : "会议、课程和电脑播放的声音"}</p>
									<p aria-live="polite">
										{unavailable
											? "来源暂不可用，可在系统设置检查设备，或取消选择"
											: ready
												? !mic && privacy?.tap_required
													? "已完成授权准备 · 实际可用性以开始记录为准"
													: "已授权"
												: denied
													? privacy?.microphone === 1
														? "访问受系统限制"
														: "尚未获得授权"
													: "需要系统授权"}
									</p>
								</div>
								<div className="setting-actions">
									{enabled && (!ready || unavailable) && (
										<button
											type="button"
											className="quiet-button"
											disabled={!!busy}
											onClick={() =>
												void perform(source, () =>
													invoke(
														denied || unavailable
															? "open_privacy_settings"
															: mic
																? "request_microphone_permission"
																: "request_system_audio_permission",
														{ source },
													),
												)
											}
										>
											{busy === source
												? "正在处理…"
												: denied || unavailable
													? "打开系统设置"
													: "允许访问"}
										</button>
									)}
									{enabled && !mic && (
										<button
											type="button"
											className="quiet-button"
											disabled={!!busy}
											onClick={() =>
												void perform(source, () =>
													invoke(
														ready
															? "request_system_audio_permission"
															: "open_privacy_settings",
														{ source },
													),
												)
											}
										>
											{ready ? "重新验证" : "系统设置"}
										</button>
									)}
								</div>
							</div>
						);
					})}
				</div>
				{compact && (
					<button
						className="setup-text-button setup-source-options-toggle"
						type="button"
						aria-label="选择声音来源与授权帮助"
						aria-expanded={sourceOptionsOpen}
						onClick={() => setSourceOptionsOpen(!sourceOptionsOpen)}
					>
						调整
					</button>
				)}
			</div>
			{compact && sourceOptionsOpen && (
				<div className="setup-source-options">
					{(["microphone", "system"] as const).map((source) => {
						const enabled =
							source === "microphone"
								? state?.microphone_enabled
								: state?.system_enabled;
						return (
							<label className="setup-source" key={source}>
								<input
									type="checkbox"
									checked={enabled ?? false}
									disabled={!!busy || !state}
									onChange={() => void chooseSource(source, !enabled)}
								/>
								{source === "microphone" ? "麦克风" : "电脑声音"}
							</label>
						);
					})}
					<div className="setup-source-recovery">
						<button
							className="setup-text-button"
							type="button"
							disabled={!!busy}
							onClick={() =>
								void perform("system", () =>
									invoke("open_privacy_settings", { source: "system" }),
								)
							}
						>
							系统设置 ↗
						</button>
						{state?.system_enabled && (
							<button
								className="setup-text-button"
								type="button"
								disabled={!!busy}
								onClick={() =>
									void perform("system", () =>
										invoke("request_system_audio_permission", {
											source: "system",
										}),
									)
								}
							>
								重新检查电脑声音
							</button>
						)}
					</div>
					<p>
						系统设置 → 隐私与安全性 →
						麦克风或屏幕与系统音频录制。返回后自动复查；受系统限制时联系管理员。
					</p>
					<p>
						{privacy?.tap_required
							? "电脑声音“已准备”表示完成过授权检查，实际可用性以开始记录为准。"
							: "屏幕相关许可用于采集电脑声音，不保存屏幕画面。"}
						授权检查不保存或上传声音。
					</p>
				</div>
			)}
			{state && !state.microphone_enabled && !state.system_enabled && (
				<p className="error">至少选择一种声音来源才能完成设置。</p>
			)}
			{failure && (
				<p className="error" role="alert">
					{failure}
				</p>
			)}
			{!compact && (
				<details className="setup-help">
					<summary>授权遇到问题</summary>
					<p>
						打开系统设置 → 隐私与安全性 → 麦克风或屏幕与系统音频录制，允许
						Hear。受系统管理限制时需联系管理员。返回后会重新检查；电脑声音可点击重新验证。授权检查不保存或上传声音。
					</p>
				</details>
			)}
		</LoadBoundary>
	);
}
