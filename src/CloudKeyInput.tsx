import { useEffect, useId, useRef, useState } from "react";
import { type CloudProvider, userError } from "./api";
import { cloudOperation, confirmCloud, saveCloud } from "./setup";

const SAVED_MASK = "••••••••••••";
export function CloudKeyInput({
	provider,
	configured,
	onRefresh,
	onActivity,
	onSaved,
	onCancel,
	compact = false,
}: {
	provider: CloudProvider;
	configured?: boolean;
	onRefresh: () => Promise<void>;
	onActivity?: (busy: boolean) => void;
	onSaved?: () => void;
	onCancel?: () => void;
	compact?: boolean;
}) {
	const [draft, setDraft] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);
	const [error, setError] = useState("");
	const [uncertain, setUncertain] = useState(!!cloudOperation(provider));
	const pending = useRef(false);
	const id = useId();
	useEffect(() => {
		onActivity?.(saving || uncertain);
		return () => onActivity?.(false);
	}, [saving, uncertain, onActivity]);
	const confirm = async () => {
		if (pending.current) return;
		pending.current = true;
		setSaving(true);
		let saved = false;
		try {
			const result = await confirmCloud(provider);
			if (result === "saved") {
				setDraft(null);
				setError("");
				setUncertain(false);
				saved = true;
			} else if (result === "pending") setError("验证仍在进行，请稍后再次确认");
			else {
				setUncertain(false);
				setError(
					"上次结果未确认，请核对已保存状态；需要时主动重新验证，可能产生费用",
				);
			}
		} catch (e) {
			setError(userError(e));
			setUncertain(!!cloudOperation(provider));
		} finally {
			try {
				await onRefresh();
			} catch {
				setError(
					saved
						? "密钥已保存，状态刷新失败，请关闭后重新读取"
						: "状态刷新失败，请稍后重试",
				);
				saved = false;
			} finally {
				pending.current = false;
				setSaving(false);
				if (saved) onSaved?.();
			}
		}
	};
	const commit = async (retry = false) => {
		if (draft === null || pending.current || uncertain || (error && !retry))
			return;
		const key = draft.trim();
		if (!key) {
			setDraft(null);
			return;
		}
		if (key.includes("•")) return;
		pending.current = true;
		setSaving(true);
		setError("");
		let saved = false;
		try {
			await saveCloud(provider, key);
			setDraft(null);
			saved = true;
		} catch (e) {
			setUncertain(!!cloudOperation(provider));
			setError(
				cloudOperation(provider)
					? "验证结果待确认，请先查询上次结果"
					: `${userError(e)}${configured ? " · 原配置仍保留" : ""}`,
			);
		} finally {
			try {
				await onRefresh();
			} catch {
				setError(
					saved
						? "密钥已保存，状态刷新失败，请关闭后重新读取"
						: "状态刷新失败，请稍后重试",
				);
				saved = false;
			} finally {
				pending.current = false;
				setSaving(false);
				if (saved) onSaved?.();
			}
		}
	};
	return (
		<div
			className={`cloud-provider-key${compact ? " cloud-provider-key-compact" : ""}`}
		>
			<label
				htmlFor={id}
				className={compact ? "setup-visually-hidden" : undefined}
			>
				API Key
			</label>
			<div className={compact ? "setup-key-field" : undefined}>
				<input
					id={id}
					aria-label={
						provider === "llm" ? "DeepSeek API Key" : "火山引擎 API Key"
					}
					type="password"
					autoComplete="off"
					spellCheck={false}
					value={draft ?? (configured ? SAVED_MASK : "")}
					disabled={saving || uncertain || configured === undefined}
					aria-invalid={!!error}
					aria-describedby={`${id}-status`}
					placeholder={
						configured === undefined
							? "正在读取配置…"
							: compact
								? "API Key"
								: "输入密钥"
					}
					onFocus={(e) => {
						if (draft === null && configured) e.currentTarget.select();
					}}
					onChange={(e) => {
						setDraft(e.target.value.replace(/•/g, ""));
						setError("");
					}}
					onKeyDown={(e) => {
						if (e.key === "Enter" && !e.nativeEvent.isComposing) {
							e.preventDefault();
							if (compact) e.currentTarget.blur();
							else void commit(true);
						}
						if (e.key === "Escape") {
							setDraft(null);
							setError("");
						}
					}}
					onBlur={compact ? () => void commit() : undefined}
				/>
				{compact && (
					<span className="setup-key-state" aria-hidden="true">
						{saving
							? "验证中…"
							: configured && draft === null && !error && !uncertain
								? "✓"
								: ""}
					</span>
				)}
			</div>
			<span
				id={`${id}-status`}
				className={`cloud-key-feedback${error ? " error" : ""}${compact && !error ? " setup-visually-hidden" : ""}`}
				aria-live="polite"
			>
				{saving
					? "正在验证或确认…"
					: error ||
						(configured && draft === null
							? "已保存 · 选中后输入可替换"
							: compact
								? "按回车或离开输入框后验证"
								: "输入新密钥后，点击验证并保存")}
			</span>
			{uncertain && (
				<button
					type="button"
					className="quiet-button"
					disabled={saving}
					onClick={() => void confirm()}
				>
					确认上次结果
				</button>
			)}
			{compact && error && !uncertain && draft && (
				<button
					type="button"
					className="quiet-button"
					disabled={saving}
					onClick={() => void commit(true)}
				>
					重新验证
				</button>
			)}
			{!compact && (
				<div className="modal-actions">
					<button
						type="button"
						className="quiet-button"
						disabled={saving || uncertain}
						onClick={onCancel}
					>
						取消
					</button>
					{!uncertain && (
						<button
							type="button"
							className="primary-button"
							disabled={saving || !draft?.trim()}
							onClick={() => void commit(true)}
						>
							{saving ? "正在验证…" : "验证并保存"}
						</button>
					)}
				</div>
			)}
		</div>
	);
}
