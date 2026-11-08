import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { core, type TimelineItem } from "./api";
import { Button, Modal } from "./Controls";
import {
	inputLabel,
	type ManualWorkflowInput,
	selectedIds,
	useWorkflowRequestId,
	WorkflowError,
	type WorkflowResult,
	type WorkflowRun,
	type WorkflowSettings,
	type WorkflowStrategy,
	workflow,
} from "./workflow-api";

type Strategy = Omit<WorkflowStrategy, "prompt">;
type Strategies = { revision: number; strategies: Strategy[] };
type Choices = { coarse: boolean; prepare_precise: boolean };

function messageOf(error: unknown) {
	return error instanceof Error ? error.message : "操作未完成";
}

export function WorkflowPicker({
	eventId,
	revision,
	onClose,
	onRun,
}: {
	eventId: string;
	revision: number;
	onClose: () => void;
	onRun: (run: WorkflowRun) => void;
}) {
	const [catalog, setCatalog] = useState<Strategies | null>(null);
	const [choice, setChoice] = useState<{
		strategy: Strategy;
		available: Choices;
	} | null>(null);
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	const requestId = useWorkflowRequestId();

	useEffect(() => {
		let active = true;
		void workflow<Strategies>({ op: "strategies" }).then(
			(value) => {
				if (active) setCatalog(value);
			},
			(error) => {
				if (active) setMessage(messageOf(error));
			},
		);
		return () => {
			active = false;
		};
	}, []);

	async function run(
		strategy: Strategy,
		input: ManualWorkflowInput = "configured",
	) {
		if (!catalog || busy) return;
		const action = {
			op: "run",
			event_id: eventId,
			strategy_id: strategy.id,
			expected_revision: revision,
			config_revision: catalog.revision,
			input,
		};
		setBusy(true);
		setMessage("");
		try {
			onRun(await workflow<WorkflowRun>(action, requestId(action)));
		} catch (error) {
			if (
				error instanceof WorkflowError &&
				error.code === "precise_input_unavailable"
			) {
				setChoice({ strategy, available: error.details as Choices });
			} else {
				setMessage(messageOf(error));
			}
		} finally {
			setBusy(false);
		}
	}

	return (
		<Modal
			title={choice ? choice.strategy.name : "执行策略"}
			busy={busy}
			feedback={message}
			onClose={onClose}
		>
			{choice ? (
				<>
					<p>
						当前没有可用的精转稿。你可以直接用粗转稿，或等精转稿就绪后执行。
					</p>
					{!choice.available.coarse && <p>当前没有可用的粗转稿。</p>}
					{!choice.available.prepare_precise && (
						<p>当前没有可用的原声或精转任务。</p>
					)}
					<div className="modal-actions">
						<Button
							tone="quiet"
							disabled={busy}
							onClick={() => setChoice(null)}
						>
							返回
						</Button>
						<Button
							tone="secondary"
							disabled={busy || !choice.available.coarse}
							onClick={() => void run(choice.strategy, "coarse")}
						>
							直接执行
						</Button>
						<Button
							tone="primary"
							disabled={busy || !choice.available.prepare_precise}
							onClick={() => void run(choice.strategy, "prepare_precise")}
						>
							先精转，再执行
						</Button>
					</div>
				</>
			) : (
				<div className="workflow-choices">
					{catalog?.strategies.map((strategy) => (
						<Button
							key={strategy.id}
							tone="quiet"
							disabled={busy}
							onClick={() => void run(strategy)}
						>
							<span>{strategy.name}</span>
							<small>{inputLabel(strategy.input)}</small>
						</Button>
					))}
					{catalog?.strategies.length === 0 && (
						<p>请先在「智能分析 → 自动流程」添加策略。</p>
					)}
					{!catalog && !message && <p>正在读取策略…</p>}
				</div>
			)}
		</Modal>
	);
}

type Preview = {
	plan: string;
	revision: number;
	labels: { id: string; name: string }[];
	actions: {
		kind: string;
		name?: string;
		reuse?: boolean;
		may_add?: boolean;
		reuse_precise?: boolean;
	}[];
	cancel_strategies: [string, string][];
	cancel_asr: string[];
};

export function WorkflowTagEditor({
	item,
	onClose,
	onSaved,
}: {
	item: TimelineItem;
	onClose: () => void;
	onSaved: () => void;
}) {
	const [settings, setSettings] = useState<WorkflowSettings | null>(null);
	const [ids, setIds] = useState(
		item.workflow?.labels.map((label) => label.id) ?? [],
	);
	const [preview, setPreview] = useState<Preview | null>(null);
	const [revision, setRevision] = useState(item.revision);
	const [message, setMessage] = useState("");
	const [busy, setBusy] = useState(false);
	const requestId = useWorkflowRequestId();

	useEffect(() => {
		let active = true;
		void workflow<WorkflowSettings>({ op: "config_show" }).then(
			(value) => {
				if (active) setSettings(value);
			},
			(error) => {
				if (active) setMessage(messageOf(error));
			},
		);
		return () => {
			active = false;
		};
	}, []);

	async function submit() {
		if (!item.id || revision == null || !settings || !ids.length || busy)
			return;
		const action = {
			op: "tags",
			event_id: item.id,
			expected_revision: preview?.revision ?? revision,
			tag_ids: ids,
			plan: preview?.plan,
		};
		setBusy(true);
		setMessage("");
		try {
			const result = await workflow<Preview>(action, requestId(action));
			if (preview) onSaved();
			else setPreview(result);
		} catch (error) {
			if (error instanceof WorkflowError && error.code === "plan_stale") {
				setPreview(error.details as Preview);
			} else if (
				error instanceof WorkflowError &&
				[
					"tag_selection_stale",
					"invalid_tag_selection",
					"unknown_tag",
					"revision_conflict",
				].includes(error.code)
			) {
				setPreview(null);
				const current = (error.details as { revision?: number } | undefined)
					?.revision;
				if (current !== undefined) setRevision(current);
				try {
					const value = await workflow<WorkflowSettings>({ op: "config_show" });
					setSettings(value);
					setIds((selected) => {
						const valid = selected.filter(
							(id) =>
								id === value.other_tag ||
								value.config.tags.some((tag) => tag.id === id),
						);
						return value.config.mode === "single" && valid.length > 1
							? []
							: valid;
					});
				} catch (refreshError) {
					setMessage(messageOf(refreshError));
					return;
				}
			}
			setMessage(messageOf(error));
		} finally {
			setBusy(false);
		}
	}

	async function retryTag() {
		const action = {
			op: "retag",
			event_id: item.id,
			expected_revision: revision,
		};
		setBusy(true);
		try {
			await workflow(action, requestId(action));
			onSaved();
		} catch (error) {
			setMessage(messageOf(error));
		} finally {
			setBusy(false);
		}
	}

	const tags = settings
		? [...settings.config.tags, { id: settings.other_tag, name: "其他" }]
		: [];
	const single = settings?.config.mode === "single";
	return (
		<Modal
			title="编辑事件标签"
			busy={busy}
			feedback={message}
			onClose={onClose}
		>
			{!preview ? (
				<div className="workflow-tag-options">
					{tags.map((tag) => (
						<label key={tag.id}>
							<input
								type={single ? "radio" : "checkbox"}
								name={single ? "event-tag" : undefined}
								checked={ids.includes(tag.id)}
								disabled={busy}
								onChange={(event) =>
									setIds(
										single || tag.id === settings?.other_tag
											? event.target.checked
												? [tag.id]
												: []
											: selectedIds(
													ids.filter((id) => id !== settings?.other_tag),
													tag.id,
													event.target.checked,
												),
									)
								}
							/>
							{tag.name}
						</label>
					))}
				</div>
			) : (
				<div>
					<p>{preview.labels.map((label) => label.name).join("、")}</p>
					{preview.actions.length > 0 && (
						<>
							<strong>将补跑</strong>
							<ul>
								{preview.actions.map((action) => (
									<li key={action.name ?? action.kind}>
										{action.name ?? "精转录"}
										{action.kind === "asr" && action.may_add
											? " · 复用适用的稿件和任务，补齐缺失精转"
											: action.reuse || action.reuse_precise
												? " · 复用现有精转"
												: ""}
									</li>
								))}
							</ul>
						</>
					)}
					{(preview.cancel_strategies.length > 0 ||
						preview.cancel_asr.length > 0) && (
						<>
							<strong>将撤掉</strong>
							<ul>
								{preview.cancel_asr.length > 0 && (
									<li>精转录 · {preview.cancel_asr.length} 项</li>
								)}
								{preview.cancel_strategies.map(([id, name]) => (
									<li key={id}>{name}</li>
								))}
							</ul>
						</>
					)}
				</div>
			)}
			<div className="modal-actions">
				{item.workflow?.tag_state === "failed" && !preview && (
					<Button tone="quiet" disabled={busy} onClick={() => void retryTag()}>
						重试打标
					</Button>
				)}
				{preview && (
					<Button tone="quiet" disabled={busy} onClick={() => setPreview(null)}>
						返回
					</Button>
				)}
				<Button
					tone="primary"
					disabled={busy || !settings || !ids.length}
					onClick={() => void submit()}
				>
					{preview ? "保存并处理" : "预览处理"}
				</Button>
			</div>
		</Modal>
	);
}

const labels: Record<string, string> = {
	waiting_precise: "等待精转",
	queued: "已排队",
	running: "生成中",
	succeeded: "已生成",
	failed: "处理失败",
	cancelled: "已撤掉",
	superseded: "事件内容已变更",
};
const reasons: Record<string, string> = {
	asr_failed: "精转失败",
	partial_asr_failed: "部分精转失败",
	partial_transcript: "精转稿不完整",
	no_transcript: "没有识别到文字",
	audio_unavailable: "没有可用原声",
	input_unavailable: "没有可用转录",
	input_too_large: "转录过长，无法一次处理",
	invalid_workflow_output: "生成结果格式无效",
	queue_paused: "云端处理已暂停",
	provider_unconfigured: "云端服务尚未配置",
};

export function workflowRunStatus(run: WorkflowRun) {
	const state = labels[run.state] ?? run.state;
	const reason = run.reason && reasons[run.reason];
	return reason ? `${state} · ${reason}` : state;
}

export function workflowRetryTask(run: WorkflowRun) {
	return run.state === "failed" && run.task_id
		? run.task_id
		: run.retry_asr_task_id;
}

type WorkflowTab = {
	run: WorkflowRun;
	versions: string[];
};

export function useWorkflowTabs(eventId: string | null, refreshToken: number) {
	const [runs, setRuns] = useState<WorkflowRun[]>([]);
	const [message, setMessage] = useState("");
	const [reload, setReload] = useState(0);

	// biome-ignore lint/correctness/useExhaustiveDependencies: Core and local refresh tokens invalidate the metadata projection.
	useEffect(() => {
		if (!eventId) return;
		let live = true;
		async function load() {
			const runs: WorkflowRun[] = [];
			let cursor: string | null | undefined;
			do {
				const page: { runs: WorkflowRun[]; next_cursor?: string | null } =
					await workflow({
						op: "runs",
						event_id: eventId,
						latest_only: true,
						cursor: cursor ?? undefined,
					});
				runs.push(...page.runs);
				cursor = page.next_cursor;
			} while (live && cursor);
			if (live) {
				setRuns(runs);
				setMessage("");
			}
		}
		void load().catch((error) => {
			if (live) setMessage(messageOf(error));
		});
		return () => {
			live = false;
		};
	}, [eventId, refreshToken, reload]);

	const tabs = new Map<string, WorkflowTab>();
	for (const run of runs) {
		const tab = tabs.get(run.strategy_id) ?? { run, versions: [] };
		if (run.result_version_id) tab.versions.push(run.result_version_id);
		tabs.set(run.strategy_id, tab);
	}
	function refresh() {
		setReload((prior) => prior + 1);
	}

	function accept(run: WorkflowRun) {
		setRuns((prior) => [
			run,
			...prior.filter((item) => item.run_id !== run.run_id),
		]);
		refresh();
	}

	async function retry(taskId: string) {
		try {
			await core({ type: "retry_cloud_task", task_id: taskId });
			refresh();
		} catch (error) {
			setMessage(messageOf(error));
		}
	}

	return {
		tabs: [...tabs.values()].sort((a, b) =>
			a.run.name.localeCompare(b.run.name, "zh-CN"),
		),
		message,
		accept,
		refresh,
		retry,
	};
}

export function WorkflowResultPanel({
	eventId,
	tab,
	message,
	onRetry,
}: {
	eventId: string;
	tab: WorkflowTab;
	message: string;
	onRetry: (id: string) => void;
}) {
	const { run, versions } = tab;
	const version = versions.join(",");
	const [result, setResult] = useState<WorkflowResult | null>(null);
	const [error, setError] = useState("");
	const [loading, setLoading] = useState(false);

	useEffect(() => {
		if (!version) {
			setResult(null);
			return;
		}
		let live = true;
		setError("");
		void workflow<WorkflowResult>({
			op: "result",
			event_id: eventId,
			strategy_id: run.strategy_id,
		}).then(
			(value) => {
				if (live)
					setResult((prior) =>
						prior?.result_version_id === value.result_version_id
							? prior
							: value,
					);
			},
			(error) => {
				if (live) {
					setResult(null);
					setError(messageOf(error));
				}
			},
		);
		return () => {
			live = false;
		};
	}, [eventId, run.strategy_id, version]);

	async function readMore() {
		if (!result?.next_cursor || loading) return;
		const currentResult = result;
		setLoading(true);
		try {
			const next = await workflow<WorkflowResult>({
				op: "result",
				event_id: eventId,
				strategy_id: currentResult.strategy_id,
				version_id: currentResult.result_version_id,
				cursor: currentResult.next_cursor,
			});
			setResult((prior) =>
				prior === currentResult
					? { ...next, body_markdown: prior.body_markdown + next.body_markdown }
					: prior,
			);
		} catch (error) {
			setError(messageOf(error));
		} finally {
			setLoading(false);
		}
	}

	const retryTask = workflowRetryTask(run);
	return (
		<div className="workflow-results">
			{error || message ? <p role="status">{error || message}</p> : null}
			{version && result ? (
				<>
					<article className="workflow-markdown">
						<Markdown
							remarkPlugins={[remarkGfm]}
							skipHtml
							components={{
								img: ({ alt }) => <span>{alt}</span>,
								a: ({ children }) => <span>{children}</span>,
								table: ({ node: _node, ...props }) => (
									<div className="workflow-table-scroll">
										<table {...props} />
									</div>
								),
							}}
						>
							{result.body_markdown}
						</Markdown>
					</article>
					{result.next_cursor && (
						<Button
							tone="quiet"
							disabled={loading}
							onClick={() => void readMore()}
						>
							继续阅读
						</Button>
					)}
				</>
			) : (
				<div className="detail-empty">
					{!error && (
						<p role="status">
							{version ? "正在读取…" : workflowRunStatus(run)}
						</p>
					)}
					{!version && retryTask && (
						<Button tone="quiet" onClick={() => onRetry(retryTask)}>
							{run.task_id ? "重试生成" : "重试精转"}
						</Button>
					)}
				</div>
			)}
		</div>
	);
}
