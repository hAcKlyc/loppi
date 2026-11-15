import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

type UpdateStatus = {
	revision: number;
	available: { currentVersion: string; version: string } | null;
	phase: "idle" | "checking" | "downloading" | "stopping" | "installing";
	checked: boolean;
	error: string;
};

const initialStatus: UpdateStatus = {
	revision: -1,
	available: null,
	phase: "idle",
	checked: false,
	error: "",
};

export function useAppUpdate() {
	const [currentVersion, setCurrentVersion] = useState("");
	const [status, setStatus] = useState(initialStatus);
	const [requestError, setRequestError] = useState("");
	const revision = useRef(initialStatus.revision);
	const working = useRef(false);

	useEffect(() => {
		if (!isTauri()) return;
		let disposed = false;
		let unlisten: UnlistenFn | undefined;
		const accept = (next: UpdateStatus) => {
			if (disposed || next.revision <= revision.current) return;
			revision.current = next.revision;
			setStatus(next);
			setRequestError("");
		};
		void getVersion()
			.then((version) => {
				if (!disposed) setCurrentVersion(version);
			})
			.catch(() => {});
		void listen<UpdateStatus>("loppi-update-status", ({ payload }) =>
			accept(payload),
		)
			.then(async (stop) => {
				if (disposed) {
					stop();
					return;
				}
				unlisten = stop;
				accept(await invoke<UpdateStatus>("update_status"));
			})
			.catch((error) => {
				if (!disposed) setRequestError(String(error));
			});
		return () => {
			disposed = true;
			unlisten?.();
		};
	}, []);

	const request = async (command: string, args?: { version: string }) => {
		if (working.current || !isTauri()) return;
		working.current = true;
		setRequestError("");
		try {
			await invoke(command, args);
		} catch (error) {
			setRequestError(String(error));
		} finally {
			working.current = false;
		}
	};

	return {
		currentVersion: status.available?.currentVersion || currentVersion,
		available: status.available,
		phase: status.phase,
		error: requestError || status.error,
		checked: status.checked,
		check: () => request("check_update"),
		install: async () => {
			if (status.available) {
				await request("install_update", { version: status.available.version });
			}
		},
	};
}

export type AppUpdate = ReturnType<typeof useAppUpdate>;
