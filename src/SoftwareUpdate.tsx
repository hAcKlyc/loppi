import { Button } from "./Controls";
import type { AppUpdate } from "./useAppUpdate";
import "./software-update.css";

function updateStatus(update: AppUpdate): string {
	if (update.phase === "checking") return "正在检查更新…";
	if (update.phase === "downloading") return "正在后台下载并验证更新…";
	if (update.phase === "stopping") return "正在结束录音并等待后台退出…";
	if (update.phase === "installing") return "正在安装，即将重启…";
	if (update.available)
		return `${update.available.version} 已准备好，可在左下角重启更新`;
	if (update.checked && !update.error) return "当前已是最新版本";
	return "启动后自动检查并下载更新";
}

export function SoftwareUpdate({ update }: { update: AppUpdate }) {
	return (
		<div className="setting-row software-update-setting">
			<div>
				<strong>
					软件更新{update.currentVersion ? ` · ${update.currentVersion}` : ""}
				</strong>
				<p role="status">{updateStatus(update)}</p>
				{update.error && (
					<p className="warning-text" role="alert">
						{update.error}
					</p>
				)}
			</div>
			<Button
				disabled={update.phase !== "idle"}
				onClick={() => void update.check()}
			>
				检查更新
			</Button>
		</div>
	);
}

export function RestartUpdate({ update }: { update: AppUpdate }) {
	if (!update.available) return null;
	const busy = update.phase !== "idle";
	return (
		<div className="sidebar-update">
			<Button
				className="restart-update-button"
				disabled={busy}
				busy={busy}
				busyLabel={
					update.phase === "checking" || update.phase === "downloading"
						? "正在准备更新…"
						: "正在重启…"
				}
				title={`更新到 ${update.available.version}`}
				onClick={() => void update.install()}
			>
				重启更新
			</Button>
			{update.error && (
				<p className="warning-text" role="alert">
					{update.error}
				</p>
			)}
		</div>
	);
}
