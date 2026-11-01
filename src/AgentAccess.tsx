import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button, SegmentedOptions, Tabs } from "./Controls";
import { desktopPlatform } from "./platform";
import "./AgentAccess.css";

type Connection = {
	cli_path: string;
	cli_available: boolean;
	codex_config: string;
	mcp_config: object;
	codex_command: string;
	skill_prompt: string;
	status: {
		ok: boolean;
		data?: { online: boolean };
		error?: { code: string; message: string };
	};
};

export function AgentAccess() {
	const [mode, setMode] = useState<"mcp" | "cli">("mcp");
	const [target, setTarget] = useState<"codex" | "generic">("generic");
	const [connection, setConnection] = useState<Connection | null>(null);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState("");
	const [feedback, setFeedback] = useState("");
	const request = useRef(0);
	const load = useCallback(async () => {
		const generation = ++request.current;
		setLoading(true);
		setError("");
		try {
			const result = await invoke<Connection>("agent_access_info");
			if (generation === request.current) setConnection(result);
		} catch (cause) {
			if (generation === request.current) {
				setConnection(null);
				const detail =
					typeof cause === "string"
						? cause
						: cause instanceof Error
							? cause.message
							: "";
				setError(
					detail
						? `暂时无法读取接入信息：${detail}`
						: "暂时无法读取接入信息，请重试。",
				);
			}
		} finally {
			if (generation === request.current) setLoading(false);
		}
	}, []);
	useEffect(() => {
		void load();
		return () => {
			++request.current;
		};
	}, [load]);
	const copy = async (text: string) => {
		setFeedback("");
		try {
			if (isTauri()) await invoke("copy_agent_text", { text });
			else await navigator.clipboard.writeText(text);
			setFeedback("已复制");
		} catch {
			setFeedback("复制失败，可选中文字手动复制。");
		}
	};
	const online = connection?.status.ok && connection.status.data?.online;
	const incompatible = connection?.status.error?.code === "core_incompatible";
	const available = connection?.cli_available;
	const configuration = connection
		? target === "codex"
			? connection.codex_config.trim()
			: JSON.stringify(connection.mcp_config, null, 2)
		: "";

	return (
		<section
			className="page-container agent-access"
			aria-labelledby="agent-access-title"
		>
			<header
				className="page-heading agent-page-heading"
				data-tauri-drag-region
			>
				<h1 id="agent-access-title">Agent 接入</h1>
				<p>让你的 Agent 找回 loppi 里的谈话与日常。</p>
			</header>
			<section
				className="agent-connection"
				aria-labelledby="agent-connect-title"
			>
				<div className="agent-connection-heading">
					<h2 id="agent-connect-title">选择接入方式</h2>
					<div className="agent-connection-status">
						<span className={online ? "agent-online" : ""} role="status">
							{loading
								? "正在检查…"
								: error
									? "状态未知"
									: incompatible
										? "需要重新打开 loppi"
										: online
											? "本机可用"
											: "后台未连接"}
						</span>
						<Button
							size="compact"
							busy={loading}
							onClick={() => void load()}
							aria-label="重新检查连接"
						>
							刷新
						</Button>
					</div>
				</div>
				<Tabs
					id="agent-method"
					label="接入方式"
					value={mode}
					items={[
						{ value: "mcp", label: "MCP" },
						{ value: "cli", label: "CLI + Skill" },
					]}
					onChange={(value) => {
						setMode(value);
						setFeedback("");
					}}
				/>
				<div
					role="tabpanel"
					id={`agent-method-panel-${mode}`}
					aria-labelledby={`agent-method-tab-${mode}`}
					className="agent-method-panel"
				>
					{error ? (
						<div className="agent-inline-error" role="alert">
							{error}
						</div>
					) : !connection ? (
						<p className="agent-muted">正在准备接入信息…</p>
					) : !available ? (
						<p className="agent-inline-error">
							未找到随 App 安装的命令行工具。请使用完整的 loppi 安装包。
						</p>
					) : mode === "mcp" ? (
						<>
							<div className="agent-method-intro">
								<p>把 loppi 加入 Agent 的 MCP 配置，即可检索与读取记录。</p>
								<SegmentedOptions
									label="配置格式"
									value={target}
									options={[
										{ value: "generic", label: "通用 JSON" },
										{ value: "codex", label: "Codex" },
									]}
									onChange={(value) => {
										setTarget(value);
										setFeedback("");
									}}
								/>
							</div>
							<pre className="agent-code">
								<code>{configuration}</code>
							</pre>
							<div className="agent-actions">
								<Button tone="primary" onClick={() => void copy(configuration)}>
									复制配置
								</Button>
								<span>
									{target === "codex"
										? desktopPlatform() === "windows"
											? "添加到 Codex 配置文件后重连。"
											: "添加到 ~/.codex/config.toml 后重连。"
										: "合并到 Agent 的 MCP 配置后重连。"}
								</span>
							</div>
						</>
					) : (
						<>
							<p className="agent-method-description">
								复制以下提示词发给你的 Agent，将 Skill 安装到当前工作区。
							</p>
							<pre className="agent-code">
								<code>{connection.skill_prompt}</code>
							</pre>
							<div className="agent-actions">
								<Button
									tone="primary"
									onClick={() => void copy(connection.skill_prompt)}
								>
									复制安装提示词
								</Button>
							</div>
						</>
					)}
					<div className="agent-copy-feedback" role="status" aria-live="polite">
						{feedback}
					</div>
				</div>
				{connection && !online && !error && (
					<p className="agent-connection-notice">
						{incompatible
							? "当前后台与命令行版本不同。重新打开 loppi 后再连接 Agent。"
							: "请保持 loppi 运行，再从 Agent 读取记录。"}
					</p>
				)}
			</section>
		</section>
	);
}
