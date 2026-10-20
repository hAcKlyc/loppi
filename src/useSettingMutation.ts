import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import { type CoreRequest, type CoreResponse, userError } from "./api";

/** UI-local reuse of Core's existing revision checks and idempotent receipts. */
export function useSettingMutation(refresh: () => Promise<void>) {
	const pending = useRef<{ id: string; request: CoreRequest } | null>(null);
	const active = useRef(false);
	const [busy, setBusy] = useState(false);
	const [uncertain, setUncertain] = useState(false);
	const [error, setError] = useState("");
	const submit = async (
		request: CoreRequest,
	): Promise<"saved" | "conflict" | "failed"> => {
		if (active.current) return "failed";
		active.current = true;
		pending.current ??= { id: crypto.randomUUID(), request };
		setBusy(true);
		setError("");
		try {
			const result = await invoke<CoreResponse>("core_request", {
				request: pending.current.request,
				requestId: pending.current.id,
			});
			if (result.type !== "status" && result.type !== "error")
				throw new Error("unconfirmed response");
			pending.current = null;
			setUncertain(false);
			const accepted = result.type === "status";
			if (!accepted) setError(userError(`${result.code}: ${result.message}`));
			try {
				await refresh();
			} catch {
				if (accepted) setError("设置已提交，状态刷新失败，请重新读取状态");
			}
			return accepted
				? "saved"
				: result.code === "revision_conflict"
					? "conflict"
					: "failed";
		} catch {
			setUncertain(true);
			setError("操作结果待确认，请确认上次结果");
			return "failed";
		} finally {
			active.current = false;
			setBusy(false);
		}
	};
	const confirm = () =>
		pending.current
			? submit(pending.current.request)
			: Promise.resolve("failed" as const);
	return { submit, confirm, busy, uncertain, error, setError };
}
