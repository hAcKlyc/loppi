import { useId, useRef, useState } from "react";
import type { CloudProvider } from "./api";

const SAVED_MASK = "••••••••••••";
export function CloudKeyInput({
	provider,
	configured,
	busy,
	onMutate,
}: {
	provider: CloudProvider;
	configured?: boolean;
	busy: boolean;
	onMutate: (
		request: { type: string; [key: string]: unknown },
		success: string,
	) => Promise<boolean>;
}) {
	// null is unchanged. A mask is presentation only and can never become a credential.
	const [draft, setDraft] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);
	const [failed, setFailed] = useState(false);
	const pending = useRef(false);
	const id = useId();
	const commit = async () => {
		if (draft === null || pending.current) return;
		const key = draft.trim();
		if (!key) {
			setDraft(null);
			setFailed(false);
			return;
		}
		if (key.includes("•")) return;
		pending.current = true;
		setSaving(true);
		setFailed(false);
		try {
			const saved = await onMutate(
				{
					type: "configure_cloud",
					provider,
					api_key: key,
					model: provider === "llm" ? "deepseek-flash" : null,
				},
				provider === "llm" ? "DeepSeek 已配置" : "火山引擎已配置",
			);
			if (saved) setDraft(null);
			else setFailed(true);
		} catch {
			setFailed(true);
		} finally {
			pending.current = false;
			setSaving(false);
		}
	};
	return (
		<label className="cloud-provider-key">
			API Key
			<input
				type="password"
				autoComplete="off"
				spellCheck={false}
				value={draft ?? (configured ? SAVED_MASK : "")}
				disabled={busy || saving || configured === undefined}
				aria-invalid={failed}
				aria-describedby={id}
				placeholder={configured === undefined ? "正在读取配置…" : "输入密钥"}
				onFocus={(event) => {
					if (draft === null && configured) event.currentTarget.select();
				}}
				onChange={(event) => {
					setDraft(event.target.value.replace(/•/g, ""));
					setFailed(false);
				}}
				onKeyDown={(event) => {
					if (event.key === "Enter" && !event.nativeEvent.isComposing) {
						event.preventDefault();
						event.currentTarget.blur();
					}
					if (event.key === "Escape") {
						setDraft(null);
						setFailed(false);
					}
				}}
				onBlur={() => void commit()}
			/>
			<span
				id={id}
				className={`cloud-key-feedback${failed ? " error" : ""}`}
				aria-live="polite"
			>
				{saving
					? "正在验证…"
					: failed
						? configured
							? "新密钥未保存，原配置仍有效"
							: "验证未通过，密钥未保存"
						: configured && draft === null
							? "已保存 · 选中后输入可替换"
							: "按回车或离开输入框后验证"}
			</span>
		</label>
	);
}
