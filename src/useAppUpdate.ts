import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";

export type AvailableUpdate = {
	currentVersion: string;
	version: string;
};
export type UpdateProgress = {
	phase: "downloading" | "stopping" | "installing";
};

export function useAppUpdate() {
	const [currentVersion, setCurrentVersion] = useState("");
	const [available, setAvailable] = useState<AvailableUpdate | null>(null);
	const [phase, setPhase] = useState<
		"idle" | "checking" | UpdateProgress["phase"]
	>("idle");
	const [error, setError] = useState("");
	const [checked, setChecked] = useState(false);
	const working = useRef(false);
	const attempted = useRef(false);
	const check = useCallback(async () => {
		if (working.current || !isTauri()) return;
		working.current = true;
		attempted.current = true;
		setPhase("checking");
		setError("");
		try {
			const channel = new Channel<UpdateProgress>();
			channel.onmessage = (value) => {
				setPhase(value.phase);
			};
			const update = await invoke<AvailableUpdate | null>("check_update", {
				progress: channel,
			});
			setAvailable(update);
			if (update) setCurrentVersion(update.currentVersion);
			setChecked(true);
		} catch (cause) {
			setError(String(cause));
		} finally {
			working.current = false;
			setPhase("idle");
		}
	}, []);
	useEffect(() => {
		if (!isTauri()) return;
		void getVersion()
			.then(setCurrentVersion)
			.catch(() => {});
		const timer = window.setTimeout(() => {
			if (!attempted.current) void check();
		}, 10_000);
		return () => window.clearTimeout(timer);
	}, [check]);
	const install = async () => {
		if (working.current || !available) return;
		working.current = true;
		setError("");
		setPhase("stopping");
		const channel = new Channel<UpdateProgress>();
		channel.onmessage = (value) => {
			setPhase(value.phase);
		};
		try {
			await invoke("install_update", {
				version: available.version,
				progress: channel,
			});
		} catch (cause) {
			setError(String(cause));
			setPhase("idle");
			working.current = false;
		}
	};
	return {
		currentVersion,
		available,
		phase,
		error,
		checked,
		check,
		install,
	};
}

export type AppUpdate = ReturnType<typeof useAppUpdate>;
