import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import "./App.css";

type ServiceState = {
	revision: number;
	enabled_intent: boolean;
	paused_intent: boolean;
	microphone_enabled: boolean;
	system_enabled: boolean;
	capture_state: "not_ready" | "stopped" | "paused" | "recording";
	reason: string | null;
};

type PrivacyStatus = {
	microphone: number;
	screen_and_system_audio: number;
};

function App() {
	const [state, setState] = useState<ServiceState | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [loginItemStatus, setLoginItemStatus] = useState<number | null>(null);
	const [loginItemBusy, setLoginItemBusy] = useState(false);
	const [privacy, setPrivacy] = useState<PrivacyStatus | null>(null);
	const [permissionBusy, setPermissionBusy] = useState<
		"microphone" | "system" | null
	>(null);
	const [permissionError, setPermissionError] = useState<string | null>(null);

	const refresh = useCallback(async () => {
		try {
			const next = await invoke<ServiceState>("core_status");
			setState(next);
			setError(null);
		} catch {
			setState(null);
			setError("暂时无法连接后台服务，请稍后重试。");
		}
	}, []);

	const refreshLoginItem = useCallback(async () => {
		try {
			setLoginItemStatus(await invoke<number>("login_item_status"));
		} catch {
			setLoginItemStatus(null);
		}
	}, []);

	const refreshPrivacy = useCallback(async () => {
		try {
			setPrivacy(await invoke<PrivacyStatus>("privacy_status"));
		} catch {
			setPrivacy(null);
		}
	}, []);

	const requestPermission = async (source: "microphone" | "system") => {
		setPermissionBusy(source);
		setPermissionError(null);
		try {
			const result = await invoke<number>(
				source === "microphone"
					? "request_microphone_permission"
					: "request_system_audio_permission",
			);
			if (result < 0) {
				setPermissionError("系统授权请求未完成，请稍后重试。");
			} else if (result !== (source === "microphone" ? 3 : 1)) {
				setPermissionError("请在系统设置中允许 hear 使用这项声音来源。");
			}
			await refreshPrivacy();
		} catch {
			setPermissionError("暂时无法请求系统权限，请稍后重试。");
		} finally {
			setPermissionBusy(null);
		}
	};

	const changeLoginItem = async () => {
		setLoginItemBusy(true);
		try {
			const command =
				loginItemStatus === 1 ? "unregister_login_item" : "register_login_item";
			setLoginItemStatus(await invoke<number>(command));
			setError(null);
		} catch {
			setError("无法更改登录启动设置，请检查系统设置后重试。");
			await refreshLoginItem();
		} finally {
			setLoginItemBusy(false);
		}
	};

	useEffect(() => {
		void invoke("gui_ready");
		void refresh();
		void refreshLoginItem();
		void refreshPrivacy();
		const timer = window.setInterval(() => void refresh(), 5000);
		return () => window.clearInterval(timer);
	}, [refresh, refreshLoginItem, refreshPrivacy]);

	const loginItemDescription =
		loginItemStatus === 1
			? "已开启"
			: loginItemStatus === 2
				? "需要在系统设置中批准"
				: loginItemStatus === 0 || loginItemStatus === 3
					? "未开启"
					: "状态未知";

	const description = error
		? "服务暂时不可用"
		: state?.capture_state === "recording"
			? "正在记录"
			: state?.capture_state === "not_ready"
				? "尚未就绪"
				: state?.paused_intent
					? "已暂停"
					: "尚未开始";

	return (
		<main className="shell">
			<header className="topbar">
				<span className="brand-mark">h.</span>
				<span className="brand-name">hear</span>
				<span className="topbar-status">
					<span
						className={`status-dot ${state?.capture_state === "recording" ? "is-live" : ""}`}
					/>
					{description}
				</span>
			</header>
			<section className="content">
				<div className="date-line">今天</div>
				<h1>你的时间线</h1>
				<div className="empty-state">
					<div className="empty-icon" aria-hidden="true">
						◌
					</div>
					<h2>{description}</h2>
					<p>{error ?? "完成首次设置后，记录会出现在这里。"}</p>
					<button type="button" onClick={() => void refresh()}>
						刷新状态
					</button>
				</div>
				<div className="setup-row">
					<div>
						<strong>登录时启动</strong>
						<span>{loginItemDescription}</span>
					</div>
					<button
						type="button"
						disabled={loginItemBusy || loginItemStatus === null}
						onClick={() => void changeLoginItem()}
					>
						{loginItemStatus === 1 ? "关闭" : "开启"}
					</button>
				</div>
				<div className="setup-row">
					<div>
						<strong>麦克风</strong>
						<span>{privacy?.microphone === 3 ? "已允许" : "未允许"}</span>
					</div>
					<button
						type="button"
						disabled={permissionBusy !== null || privacy?.microphone === 3}
						onClick={() => void requestPermission("microphone")}
					>
						{permissionBusy === "microphone" ? "等待系统授权…" : "允许访问"}
					</button>
				</div>
				<div className="setup-row">
					<div>
						<strong>电脑声音</strong>
						<span>
							{privacy?.screen_and_system_audio === 1 ? "已允许" : "未允许"}
						</span>
					</div>
					<button
						type="button"
						disabled={
							permissionBusy !== null || privacy?.screen_and_system_audio === 1
						}
						onClick={() => void requestPermission("system")}
					>
						{permissionBusy === "system" ? "等待系统授权…" : "允许访问"}
					</button>
				</div>
				{permissionError && (
					<p className="permission-error">{permissionError}</p>
				)}
			</section>
		</main>
	);
}

export default App;
