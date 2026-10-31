import { invoke } from "@tauri-apps/api/core";
import { useCallback, useRef, useState } from "react";
import { type CloudQueue, type ServiceState, userError } from "./api";
import { CloudKeyInput } from "./CloudKeyInput";
import { Button } from "./Controls";
import { LoadBoundary } from "./LoadingState";
import { ModelPreparation, SourcePreparation } from "./SetupGroups";
import { type ModelState, type PrivacyStatus, setupBlocker } from "./setup";
import "./onboarding.css";
export function OnboardingView({
	state,
	privacy,
	models,
	queues,
	queuesReady,
	errors,
	onRefresh,
	onComplete,
	onLater,
}: {
	state: ServiceState | null;
	privacy: PrivacyStatus | null;
	models: ModelState | null;
	queues: CloudQueue[];
	queuesReady: boolean;
	errors: { models: string; privacy: string; queues: string; state: string };
	onRefresh: () => Promise<void>;
	onComplete: () => void;
	onLater: () => void;
}) {
	const [busy, setBusy] = useState(false);
	const [failure, setFailure] = useState("");
	const active = useRef(false);
	const [sourceBusy, setSourceBusy] = useState(false);
	const [llmBusy, setLlmBusy] = useState(false);
	const [asrBusy, setAsrBusy] = useState(false);
	const llmActivity = useCallback((value: boolean) => setLlmBusy(value), []);
	const asrActivity = useCallback((value: boolean) => setAsrBusy(value), []);
	const blocker = sourceBusy
		? "请等待声音来源操作完成"
		: llmBusy || asrBusy
			? "请等待或确认云服务验证结果"
			: setupBlocker(
					state,
					models,
					privacy,
					queues,
					Object.values(errors).find(Boolean),
				);
	const finish = async () => {
		if (active.current || blocker) return;
		active.current = true;
		setBusy(true);
		setFailure("");
		try {
			await invoke("complete_onboarding");
			onComplete();
		} catch (e) {
			setFailure(userError(e));
			await onRefresh();
		} finally {
			active.current = false;
			setBusy(false);
		}
	};
	return (
		<main className="onboarding-page">
			<div className="onboarding-drag" data-tauri-drag-region />
			<div className="onboarding-content">
				<header>
					<h1>准备 loppi</h1>
				</header>
				{state?.enabled_intent && (
					<p className="setup-notice">
						{state.paused_intent ? "当前记录已暂停" : "已有记录意图"} ·
						完成设置不改变记录状态
					</p>
				)}
				<fieldset disabled={busy} className="setup-fields">
					<section className="setup-group">
						<h2>① 下载本地模型</h2>
						<div className="setup-step-content">
							<ModelPreparation
								compact
								models={models}
								error={errors.models}
								onRefresh={onRefresh}
							/>
						</div>
					</section>
					<section className="setup-group">
						<h2>② 授权声音访问</h2>
						<div className="setup-step-content">
							<SourcePreparation
								compact
								onActivity={setSourceBusy}
								state={state}
								privacy={privacy}
								error={errors.privacy || errors.state}
								onRefresh={onRefresh}
							/>
						</div>
					</section>
					<section className="setup-group">
						<h2>③ 配置云服务</h2>
						<div className="setup-step-content">
							<LoadBoundary
								ready={queuesReady}
								error={errors.queues}
								retry={() => void onRefresh()}
								label="云服务"
								rows={2}
								compact
							>
								{(["llm", "asr"] as const).map((provider) => (
									<div className="setup-cloud" key={provider}>
										<div className="setup-cloud-label">
											{provider === "llm" ? "DeepSeek" : "火山引擎"}
											<small>{provider === "llm" ? "整理" : "转录"}</small>
										</div>
										<CloudKeyInput
											compact
											provider={provider}
											configured={
												queues.find((q) => q.provider === provider)?.configured
											}
											onRefresh={onRefresh}
											onActivity={
												provider === "llm" ? llmActivity : asrActivity
											}
										/>
									</div>
								))}
								<details className="setup-help setup-key-help">
									<summary>获取密钥 ↗</summary>
									{(["llm", "asr"] as const).map((provider) => (
										<div key={provider}>
											<p>
												{provider === "llm"
													? "DeepSeek：在开放平台创建 API Key，确认可用余额。"
													: "火山引擎：开通录音文件识别标准版，创建有对应权限的 API Key。"}
											</p>
											<button
												type="button"
												className="setup-text-button"
												onClick={() =>
													void invoke("open_setup_help", { provider }).catch(
														(e) => setFailure(userError(e)),
													)
												}
											>
												{provider === "llm" ? "DeepSeek" : "火山引擎"}官方说明 ↗
											</button>
										</div>
									))}
								</details>
								<details className="setup-help setup-disclosure">
									<summary>云端处理 · 按量付费</summary>
									<p>
										DeepSeek
										接收文字、时间与来源信息，以及主动保存的新截图原图；火山引擎接收待准确转录的音频。声纹与人物库不会因此外发。
									</p>
									<p>密钥保存在本机钥匙串。验证和使用可能产生服务商费用。</p>
								</details>
							</LoadBoundary>
						</div>
					</section>
				</fieldset>
				{(failure || blocker.startsWith("录音空间不足")) && (
					<p className="error" role="alert">
						{failure || blocker}
					</p>
				)}
				<footer>
					<button
						type="button"
						className="setup-text-button"
						disabled={busy || sourceBusy}
						onClick={onLater}
					>
						稍后
					</button>
					<div className="setup-complete-control">
						<Button
							type="button"
							tone="primary"
							disabled={busy}
							aria-disabled={busy || !!blocker}
							aria-describedby="setup-completion-reason"
							onClick={() => void finish()}
						>
							{busy ? "正在核对…" : "完成设置 →"}
						</Button>
						<span
							id="setup-completion-reason"
							className={
								blocker ? "setup-completion-hint" : "setup-visually-hidden"
							}
							role="status"
						>
							{blocker || "进入首页后可主动开始记录"}
						</span>
					</div>
				</footer>
			</div>
		</main>
	);
}
