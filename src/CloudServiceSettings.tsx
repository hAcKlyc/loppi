import { useEffect, useId, useRef, useState } from "react";
import type { CloudProvider, CloudQueue } from "./api";
import { CloudKeyInput } from "./CloudKeyInput";
import { Button, Modal } from "./Controls";
import { recognitionServices } from "./recognition-services";
import { cloudOperation } from "./setup";

export function CloudServiceSettings({
	provider,
	queue,
	refresh,
	locate = false,
}: {
	provider: CloudProvider;
	queue?: CloudQueue;
	refresh: () => Promise<void>;
	locate?: boolean;
}) {
	const row = useRef<HTMLElement>(null);
	const id = useId();
	useEffect(() => {
		if (!locate || !row.current) return;
		row.current.scrollIntoView({ block: "center" });
		row.current.focus({ preventScroll: true });
	}, [locate]);
	const [editing, setEditing] = useState(false);
	const [busy, setBusy] = useState(false);
	const configured = queue?.configured;
	const operation = cloudOperation(provider);
	let status = "正在读取配置…";
	let queueNote = "";
	if (operation) status = "配置结果待确认";
	else if (queue) {
		if (!queue.configured) status = "未配置";
		else if (queue.blocked_reason) {
			status =
				queue.blocked_reason === "provider_http_402"
					? "已配置 · 余额不足"
					: "已配置 · 处理受阻";
			queueNote =
				queue.blocked_reason === "provider_http_402"
					? "充值后，在智能识别中继续；失败项需手动重试。"
					: "请在智能识别中查看详情。";
		} else if (queue.paused) {
			status = "已配置 · 已暂停";
			queueNote = "录音与云端处理分别控制，请在智能识别中继续。";
		} else if (queue.processing > 0)
			status = `已配置 · ${queue.processing} 项处理中`;
		else if (queue.pending > 0) status = `已配置 · ${queue.pending} 项待处理`;
		else status = "已配置 · 等待任务";
		if (queue.configured && !queueNote && queue.failed > 0)
			queueNote = `${queue.failed} 项处理失败，请在智能识别中查看。`;
	}
	const { title, description } = recognitionServices[provider];
	const close = () => setEditing(false);
	return (
		<section
			className="setting-row cloud-setting-row"
			ref={row}
			tabIndex={-1}
			aria-labelledby={`${id}-title`}
			aria-describedby={`${id}-description`}
			data-cloud-provider={provider}
		>
			<div>
				<div className="setting-heading">
					<strong id={`${id}-title`}>{title}</strong>
					<span
						className={`setting-status${!operation && queue?.configured && queue.blocked_reason ? " is-warning" : ""}`}
						role="status"
					>
						{status}
					</span>
				</div>
				<p id={`${id}-description`}>{description}</p>
				{queueNote && <p>{queueNote}</p>}
			</div>
			<Button
				type="button"
				tone="quiet"
				disabled={configured === undefined}
				onClick={() => setEditing(true)}
			>
				{operation ? "确认配置" : configured ? "修改密钥" : "配置"}
			</Button>
			{editing && (
				<Modal title={title} busy={busy} onClose={close}>
					<p className="schedule-note">
						{provider === "llm"
							? "配置后，新截图会发送给 DeepSeek，生成图片摘要和文字转录；事件文字会用于自动整理。"
							: "配置后，需要精转的音频会发送给火山引擎进行语音识别。"}
						验证会连接服务商，可能产生费用。
					</p>
					<CloudKeyInput
						provider={provider}
						configured={configured}
						onRefresh={refresh}
						onActivity={setBusy}
						onSaved={close}
						onCancel={close}
					/>
				</Modal>
			)}
		</section>
	);
}
