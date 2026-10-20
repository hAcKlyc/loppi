import { useState } from "react";
import type { CloudProvider } from "./api";
import { CloudKeyInput } from "./CloudKeyInput";
import { Modal } from "./Controls";
import { cloudOperation } from "./setup";

export function CloudServiceSettings({
	provider,
	configured,
	refresh,
}: {
	provider: CloudProvider;
	configured?: boolean;
	refresh: () => Promise<void>;
}) {
	const [editing, setEditing] = useState(false);
	const [busy, setBusy] = useState(false);
	const title =
		provider === "llm" ? "内容整理 · DeepSeek" : "语音精转 · 火山引擎";
	const close = () => setEditing(false);
	return (
		<div className="setting-row">
			<div>
				<div className="setting-heading">
					<strong>{title}</strong>
					<span className="setting-status" role="status">
						{cloudOperation(provider)
							? "配置结果待确认"
							: configured === undefined
								? "正在读取配置…"
								: configured
									? "已配置"
									: "未配置"}
					</span>
				</div>
				<p>
					{provider === "llm" ? "整理事件与新截图" : "将录音转为更准确的文字"}
				</p>
			</div>
			<button
				type="button"
				className="quiet-button"
				disabled={configured === undefined}
				onClick={() => setEditing(true)}
			>
				{cloudOperation(provider)
					? "确认配置"
					: configured
						? "修改密钥"
						: "配置"}
			</button>
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
		</div>
	);
}
