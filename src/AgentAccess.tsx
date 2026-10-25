import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button, SegmentedOptions, Tabs } from "./Controls";
import "./AgentAccess.css";

type Connection = {
	cli_path: string;
	cli_available: boolean;
	codex_config: string;
	mcp_config: object;
	codex_command: string;
	skill_command: string;
	example_command: string;
	status: {
		ok: boolean;
		data?: { online: boolean };
		error?: { code: string; message: string };
	};
};

export function AgentAccess() {
	const [mode, setMode] = useState<"mcp" | "cli">("mcp");
	const [target, setTarget] = useState<"codex" | "generic">("codex");
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
		} catch {
			if (generation === request.current) {
				setConnection(null);
				setError("暂时无法读取接入信息，请重试。");
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
		<section className="agent-access" aria-labelledby="agent-access-title">
			<header className="agent-page-heading" data-tauri-drag-region>
				<h1 id="agent-access-title">Agent 接入</h1>
				<p>让你的 Agent 找回 hear 里的谈话与日常。</p>
			</header>
			<div className="agent-intro">
				<div className="agent-intro-copy">
					<span className="agent-eyebrow">从这里，接着聊</span>
					<p className="agent-example">“上次聊到哪里了？”</p>
					<p>
						回顾今天的讨论，找回一个约定，核对原话。
						<br />
						相关记录会带着时间与出处，交给你正在使用的 Agent。
					</p>
				</div>
				<div className="agent-path" aria-hidden="true">
					<div className="agent-path-source">
						<span />
						<span />
						<span />
					</div>
					<svg viewBox="0 0 76 40" fill="none" aria-hidden="true">
						<path d="M0 20h67m-8-7 8 7-8 7" />
					</svg>
					<div className="agent-path-destination">↳</div>
					<span>hear</span>
					<span />
					<span>你的 Agent</span>
				</div>
			</div>
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
										? "需要重新打开 hear"
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
							未找到随 App 安装的命令行工具。请使用完整的 hear 安装包。
						</p>
					) : mode === "mcp" ? (
						<>
							<div className="agent-method-intro">
								<p>把 hear 加入 Agent 的 MCP 配置，即可检索与读取记录。</p>
								<SegmentedOptions
									label="配置格式"
									value={target}
									options={[
										{ value: "codex", label: "Codex" },
										{ value: "generic", label: "通用 JSON" },
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
										? "添加到 ~/.codex/config.toml 后重连。"
										: "合并到 Agent 的 MCP 配置后重连。"}
								</span>
							</div>
						</>
					) : (
						<>
							<p className="agent-method-description">
								适合能执行本机命令的 Agent。安装 Skill，让它按需找到记录。
							</p>
							<pre className="agent-code">
								<code>{connection.skill_command}</code>
							</pre>
							<div className="agent-actions">
								<Button
									tone="primary"
									onClick={() => void copy(connection.skill_command)}
								>
									复制安装命令
								</Button>
								<span>在终端运行一次。已有自定义 Skill 不会被覆盖。</span>
							</div>
							<details className="agent-cli-example">
								<summary>试一条读取命令</summary>
								<pre className="agent-code">
									<code>{connection.example_command}</code>
								</pre>
								<Button
									size="compact"
									onClick={() => void copy(connection.example_command)}
								>
									复制命令
								</Button>
							</details>
						</>
					)}
					<div className="agent-copy-feedback" role="status" aria-live="polite">
						{feedback}
					</div>
				</div>
				{connection && !online && !error && (
					<p className="agent-connection-notice">
						{incompatible
							? "当前后台与命令行版本不同。重新打开 hear 后再连接 Agent。"
							: "请保持 hear 运行，再从 Agent 读取记录。"}
					</p>
				)}
			</section>
			<footer className="agent-footnote">
				<span>只读已有记录 · 不启动录音 · 无需联网端口</span>
				<p>使用云端模型的 Agent 时，取出的内容会进入该 Agent 的上下文。</p>
			</footer>
		</section>
	);
}
