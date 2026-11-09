import { useEffect, useRef, useState } from "react";
import { userError } from "./api";
import {
	Button,
	Field,
	Icon,
	IconButton,
	Modal,
	SegmentedOptions,
	TabPanel,
} from "./Controls";
import {
	inputLabel,
	type WorkflowSettings as Settings,
	selectedIds,
	useWorkflowRequestId,
	type WorkflowConfig,
	WorkflowError,
	type WorkflowLabel,
	type WorkflowStrategy,
	type WorkflowTag,
	workflow,
} from "./workflow-api";

const NEW_ID = "00000000-0000-0000-0000-000000000000";
const HAS_TEXT = /[^\p{White_Space}]/u;

export type WorkflowSection = "labels" | "automation";
type Editor =
	| { kind: "tag"; index: number | null; value: WorkflowTag }
	| { kind: "auto_asr"; value: WorkflowConfig["auto_asr"] }
	| { kind: "strategy"; index: number | null; value: WorkflowStrategy };
type Dependency = {
	kind?: "auto_asr";
	strategy_id?: string;
	name?: string;
	tag_ids: string[];
};
type ConfigWrite = {
	op: "config_set";
	config: WorkflowConfig;
	expected_revision: number;
	add_required_asr_tags: boolean;
};
type Feedback =
	| { kind: "applied" }
	| { kind: "retry"; action: ConfigWrite }
	| { kind: "error"; message: string; code?: string; affected: Dependency[] };

function TagChoices({
	tags,
	ids,
	onChange,
	disabled,
}: {
	tags: WorkflowLabel[];
	ids: string[];
	onChange: (ids: string[]) => void;
	disabled: boolean;
}) {
	return (
		<div className="workflow-tag-options">
			{tags.map((tag) => (
				<label key={tag.id}>
					<input
						type="checkbox"
						checked={ids.includes(tag.id)}
						disabled={disabled}
						onChange={(event) =>
							onChange(selectedIds(ids, tag.id, event.target.checked))
						}
					/>
					{tag.name}
				</label>
			))}
		</div>
	);
}

function editList<T>(items: T[], index: number | null, value: T) {
	return index === null
		? [...items, value]
		: items.map((item, position) => (position === index ? value : item));
}

export function WorkflowSettings({
	section,
	tabsId,
	onSection,
}: {
	section: WorkflowSection | null;
	tabsId: string;
	onSection: (section: WorkflowSection) => void;
}) {
	const [saved, setSaved] = useState<Settings | null>(null);
	const [loadAttempt, setLoadAttempt] = useState(0);
	const [loadError, setLoadError] = useState("");
	const [busy, setBusy] = useState(false);
	const saving = useRef(false);
	const [feedback, setFeedback] = useState<Feedback | null>(null);
	const [editor, setEditor] = useState<Editor | null>(null);
	const selectedAsrMode = useRef<HTMLInputElement>(null);
	const requestId = useWorkflowRequestId();
	const active = section !== null;

	// biome-ignore lint/correctness/useExhaustiveDependencies: Explicit retry invalidates the read.
	useEffect(() => {
		if (!active || saved) return;
		let live = true;
		void workflow<Settings>({ op: "config_show" }).then(
			(value) => {
				if (!live) return;
				setSaved(value);
				setLoadError("");
			},
			(error: unknown) => {
				if (live) setLoadError(userError(error));
			},
		);
		return () => {
			live = false;
		};
	}, [active, saved, loadAttempt]);

	function edit(next: Editor | null) {
		setEditor(next);
		setFeedback(null);
	}

	async function save(action: ConfigWrite) {
		if (saving.current) return;
		saving.current = true;
		setBusy(true);
		setFeedback(null);
		try {
			const value = await workflow<Settings>(action, requestId(action));
			setSaved(value);
			setEditor(null);
			setFeedback({ kind: "applied" });
		} catch (error) {
			if (error instanceof WorkflowError) {
				setFeedback({
					kind: "error",
					code: error.code,
					message: error.message,
					affected:
						(error.details as { affected?: Dependency[] } | null)?.affected ??
						[],
				});
			} else if (
				error instanceof Error &&
				error.message.startsWith("core_transport:")
			) {
				setFeedback({ kind: "retry", action });
			} else {
				setFeedback({ kind: "error", message: userError(error), affected: [] });
			}
		} finally {
			saving.current = false;
			setBusy(false);
		}
	}

	function submit(config: WorkflowConfig, addRequiredTags = false) {
		if (!saved || locked) return;
		void save({
			op: "config_set",
			config,
			expected_revision: saved.revision,
			add_required_asr_tags: addRequiredTags,
		});
	}

	function reload() {
		edit(null);
		setSaved(null);
		setLoadError("");
		setLoadAttempt((value) => value + 1);
	}

	function openAutomation(target: number | "auto_asr" | undefined) {
		onSection("automation");
		requestAnimationFrame(() => {
			const control =
				target === "auto_asr"
					? document.getElementById(`${tabsId}-auto-asr`)
					: document.getElementById(`${tabsId}-strategy-${target}`);
			(control ?? document.getElementById(`${tabsId}-tab-automation`))?.focus();
		});
	}

	if (!section) return null;
	if (!saved) {
		return (
			<TabPanel tabsId={tabsId} value={section} selected={section}>
				<div className="panel-card" role="status">
					{loadError || "正在读取标签与策略配置…"}
					{loadError && (
						<Button tone="quiet" onClick={reload}>
							重试
						</Button>
					)}
				</div>
			</TabPanel>
		);
	}

	const { config } = saved;
	const error = feedback?.kind === "error" ? feedback : null;
	const retry = feedback?.kind === "retry" ? feedback.action : null;
	const conflict = error?.code === "revision_conflict";
	const locked = busy || retry !== null || conflict;
	const extendAsr =
		editor?.kind === "strategy" && error?.code === "asr_rule_conflict";
	const labels = [...config.tags, { id: saved.other_tag, name: "其他" }];
	const labelName = (id: string) =>
		labels.find((tag) => tag.id === id)?.name ?? id;
	const editorValid =
		editor?.kind === "auto_asr"
			? editor.value.mode === "all" || editor.value.tag_ids.length > 0
			: Boolean(
					editor &&
						HAS_TEXT.test(editor.value.name) &&
						HAS_TEXT.test(
							editor.kind === "tag"
								? editor.value.criteria
								: editor.value.prompt,
						),
				);
	const editedTagId =
		editor?.kind === "tag" && editor.value.id !== NEW_ID
			? editor.value.id
			: undefined;
	const asrReference =
		editedTagId !== undefined &&
		config.auto_asr.mode === "tags" &&
		config.auto_asr.tag_ids.includes(editedTagId);
	const strategyReferences =
		editedTagId === undefined
			? []
			: config.strategies.flatMap((strategy, index) =>
					strategy.tag_ids.includes(editedTagId)
						? [{ name: strategy.name, index }]
						: [],
				);
	const referencedBy = [
		...(asrReference ? ["自动精转"] : []),
		...strategyReferences.map(({ name }) => name),
	];

	function openAffectedRule(rule: Dependency) {
		edit(null);
		openAutomation(
			rule.kind === "auto_asr"
				? "auto_asr"
				: config.strategies.findIndex(
						(strategy) => strategy.id === rule.strategy_id,
					),
		);
	}

	function closeEditor() {
		setEditor(null);
		if (!retry && !conflict) setFeedback(null);
	}

	function applyEditor(addRequiredTags = false) {
		if (!editor || !editorValid || locked) return;
		const previous =
			editor.kind === "auto_asr"
				? config.auto_asr
				: editor.index === null
					? undefined
					: (editor.kind === "tag" ? config.tags : config.strategies)[
							editor.index
						];
		if (editor.value === previous) {
			edit(null);
			return;
		}
		if (editor.kind === "auto_asr") {
			submit({ ...config, auto_asr: editor.value });
		} else if (editor.kind === "tag") {
			submit({
				...config,
				tags: editList(config.tags, editor.index, editor.value),
			});
		} else {
			submit(
				{
					...config,
					strategies: editList(config.strategies, editor.index, editor.value),
				},
				addRequiredTags,
			);
		}
	}

	function removeEditor() {
		if (!editor || editor.kind === "auto_asr" || editor.index === null) return;
		submit(
			editor.kind === "tag"
				? {
						...config,
						tags: config.tags.filter((_, index) => index !== editor.index),
					}
				: {
						...config,
						strategies: config.strategies.filter(
							(_, index) => index !== editor.index,
						),
					},
		);
	}

	function triggerSummary(ids: string[], empty: string) {
		return ids.length ? (
			<div className="workflow-strategy-tags">
				{ids.map((id) => (
					<span className="workflow-chip" key={id}>
						{labelName(id)}
					</span>
				))}
			</div>
		) : (
			empty
		);
	}

	const failureMessage = retry
		? "未收到设置结果，可重试同一次操作。"
		: error?.message;
	const resolution = retry ? (
		<Button tone="primary" onClick={() => void save(retry)}>
			重试
		</Button>
	) : conflict ? (
		<Button tone="primary" onClick={reload}>
			读取最新配置
		</Button>
	) : null;

	return (
		<TabPanel tabsId={tabsId} value={section} selected={section}>
			<div className="workflow-settings">
				{!editor && (busy || feedback) && (
					<div className="workflow-config-feedback">
						{failureMessage ? (
							<p role="alert">{failureMessage}</p>
						) : (
							<p role="status">{busy ? "正在应用设置…" : "设置已生效"}</p>
						)}
						{resolution}
					</div>
				)}
				{section === "labels" ? (
					<>
						<div className="workflow-config-rule">
							<span>每个事件支持智能打标签模式</span>
							<div className="workflow-config-rule-options">
								<SegmentedOptions
									label="标注方式"
									value={config.mode}
									options={[
										{ value: "single", label: "单选" },
										{ value: "multiple", label: "多选" },
									]}
									disabled={locked}
									onChange={(mode) => submit({ ...config, mode })}
								/>
								<small>适用于后续事件的自动标注。</small>
							</div>
						</div>
						<table className="workflow-config-table">
							<caption className="workflow-label-heading">场景标签管理</caption>
							<thead>
								<tr>
									<th scope="col">标签</th>
									<th scope="col">判断标准</th>
									<th scope="col" aria-label="操作">
										<Button
											tone="primary"
											size="compact"
											disabled={locked}
											onClick={() =>
												edit({
													kind: "tag",
													index: null,
													value: { id: NEW_ID, name: "", criteria: "" },
												})
											}
										>
											<Icon name="plus" />
											添加标签
										</Button>
									</th>
								</tr>
							</thead>
							<tbody>
								{config.tags.map((tag, index) => (
									<tr key={tag.id === NEW_ID ? `new-${index}` : tag.id}>
										<th scope="row">
											<span className="workflow-chip">{tag.name}</span>
										</th>
										<td>
											<span className="workflow-config-criteria">
												{tag.criteria}
											</span>
										</td>
										<td>
											<IconButton
												label={`编辑标签 · ${tag.name}`}
												tooltip="编辑标签"
												disabled={locked}
												onClick={() => edit({ kind: "tag", index, value: tag })}
											>
												<Icon name="edit" />
											</IconButton>
										</td>
									</tr>
								))}
								<tr className="workflow-fallback-row">
									<th scope="row">
										<span className="workflow-chip">其他</span>
									</th>
									<td>标注成功，无匹配标签</td>
									<td>
										<small>系统兜底</small>
									</td>
								</tr>
							</tbody>
						</table>
					</>
				) : (
					<table
						className="workflow-config-table workflow-config-strategies"
						aria-label="流程与策略"
					>
						<thead>
							<tr>
								<th scope="col">流程</th>
								<th scope="col">输入</th>
								<th scope="col">触发条件</th>
								<th scope="col" aria-label="操作">
									<Button
										tone="primary"
										size="compact"
										disabled={locked}
										onClick={() =>
											edit({
												kind: "strategy",
												index: null,
												value: {
													id: NEW_ID,
													name: "",
													prompt: "",
													input: "coarse",
													tag_ids: [],
												},
											})
										}
									>
										<Icon name="plus" />
										添加策略
									</Button>
								</th>
							</tr>
						</thead>
						<tbody>
							<tr>
								<th scope="row">
									自动精转{" "}
									<small className="workflow-builtin-label">官方</small>
								</th>
								<td>事件音频</td>
								<td>
									{config.auto_asr.mode === "all"
										? "全部有效事件"
										: triggerSummary(config.auto_asr.tag_ids, "未选择标签")}
								</td>
								<td>
									<Button
										id={`${tabsId}-auto-asr`}
										tone="quiet"
										size="compact"
										className="workflow-flow-edit"
										aria-label="编辑自动精转"
										disabled={locked}
										onClick={() =>
											edit({ kind: "auto_asr", value: config.auto_asr })
										}
									>
										编辑
									</Button>
								</td>
							</tr>
							{config.strategies.map((strategy, index) => (
								<tr key={strategy.id === NEW_ID ? `new-${index}` : strategy.id}>
									<th scope="row">{strategy.name}</th>
									<td>{inputLabel(strategy.input)}</td>
									<td>{triggerSummary(strategy.tag_ids, "仅手动")}</td>
									<td>
										<Button
											id={`${tabsId}-strategy-${index}`}
											aria-label={`编辑策略 · ${strategy.name}`}
											tone="quiet"
											size="compact"
											className="workflow-flow-edit"
											disabled={locked}
											onClick={() =>
												edit({
													kind: "strategy",
													index,
													value: strategy,
												})
											}
										>
											编辑
										</Button>
									</td>
								</tr>
							))}
						</tbody>
					</table>
				)}
			</div>
			{editor && (
				<Modal
					title={
						editor.kind === "auto_asr"
							? "编辑自动精转"
							: `${editor.index === null ? "添加" : "编辑"}${editor.kind === "tag" ? "标签" : "策略"}`
					}
					busy={busy}
					onClose={closeEditor}
					initialFocusRef={
						editor.kind === "auto_asr" ? selectedAsrMode : undefined
					}
				>
					<form
						className="workflow-config-editor"
						onSubmit={(event) => {
							event.preventDefault();
							if (!extendAsr) applyEditor();
						}}
					>
						{editor.kind === "auto_asr" ? (
							<>
								<div className="workflow-config-rule-options">
									<span>触发条件</span>
									<SegmentedOptions
										label="自动精转条件"
										value={editor.value.mode}
										disabled={locked}
										selectedRef={selectedAsrMode}
										options={[
											{ value: "all", label: "全部有效事件" },
											{ value: "tags", label: "包含任一标签" },
										]}
										onChange={(mode) =>
											edit({
												kind: "auto_asr",
												value:
													mode === "all"
														? { mode: "all" }
														: { mode: "tags", tag_ids: [] },
											})
										}
									/>
									{editor.value.mode === "tags" && (
										<TagChoices
											tags={labels}
											disabled={locked}
											ids={editor.value.tag_ids}
											onChange={(tag_ids) =>
												edit({
													kind: "auto_asr",
													value: { mode: "tags", tag_ids },
												})
											}
										/>
									)}
									{!editorValid && <small>至少选择一个触发标签。</small>}
								</div>
								<details className="workflow-config-disclosure">
									<summary>音频发送</summary>
									<p>
										设置适用于后续处理。补转时会把符合条件的音频发送给精转服务，已有稿件会优先复用。
									</p>
								</details>
							</>
						) : (
							<>
								<Field label={editor.kind === "tag" ? "标签名称" : "策略名称"}>
									{(attributes) => (
										<input
											{...attributes}
											disabled={locked}
											value={editor.value.name}
											placeholder={
												editor.kind === "tag"
													? "例如：工作会议"
													: "例如：会议纪要"
											}
											onChange={(event) =>
												edit({
													...editor,
													value: { ...editor.value, name: event.target.value },
												} as Editor)
											}
										/>
									)}
								</Field>
								{editor.kind === "tag" ? (
									<Field label="判断标准">
										{(attributes) => (
											<textarea
												{...attributes}
												disabled={locked}
												rows={4}
												value={editor.value.criteria}
												placeholder="例如：讨论项目进展、决策或工作安排。排除一般寒暄。"
												onChange={(event) =>
													edit({
														...editor,
														value: {
															...editor.value,
															criteria: event.target.value,
														},
													})
												}
											/>
										)}
									</Field>
								) : (
									<>
										<Field
											label="输入来源"
											description="所选转录稿会发送给已配置的生成服务。"
										>
											{(attributes) => (
												<select
													{...attributes}
													disabled={locked}
													value={editor.value.input}
													onChange={(event) =>
														edit({
															...editor,
															value: {
																...editor.value,
																input:
																	event.target.value === "precise"
																		? "precise"
																		: "coarse",
															},
														})
													}
												>
													<option value="coarse">粗转录</option>
													<option value="precise">精转录</option>
												</select>
											)}
										</Field>
										<Field label="生成指令">
											{(attributes) => (
												<textarea
													{...attributes}
													disabled={locked}
													rows={6}
													value={editor.value.prompt}
													placeholder="例如：用 Markdown 列出决策与待办，保留明确的负责人和截止日期。"
													onChange={(event) =>
														edit({
															...editor,
															value: {
																...editor.value,
																prompt: event.target.value,
															},
														})
													}
												/>
											)}
										</Field>
										<div className="workflow-config-condition">
											<span>
												触发标签 <small>留空时仅手动执行</small>
											</span>
											<TagChoices
												tags={labels}
												disabled={locked}
												ids={editor.value.tag_ids}
												onChange={(tag_ids) =>
													edit({
														...editor,
														value: { ...editor.value, tag_ids },
													})
												}
											/>
										</div>
									</>
								)}
							</>
						)}
						{referencedBy.length > 0 && !failureMessage && (
							<div className="workflow-config-hint">
								已被 {referencedBy.join("、")} 引用。
								<Button
									tone="quiet"
									disabled={locked}
									onClick={() => {
										edit(null);
										openAutomation(
											asrReference ? "auto_asr" : strategyReferences[0]?.index,
										);
									}}
								>
									去流程与策略调整
								</Button>
							</div>
						)}
						{failureMessage && <p role="alert">{failureMessage}</p>}
						{error && error.affected.length > 0 && (
							<div className="workflow-config-hint">
								<ul>
									{error.affected.map((rule) => (
										<li key={rule.strategy_id ?? "auto_asr"}>
											{rule.name ?? "自动精转"} ·{" "}
											{rule.tag_ids.map(labelName).join("、")}
										</li>
									))}
								</ul>
								{extendAsr ? (
									<p>加入后，符合这些标签的事件音频可能发送给精转服务。</p>
								) : (
									<Button
										tone="quiet"
										onClick={() => openAffectedRule(error.affected[0])}
									>
										去流程与策略调整
									</Button>
								)}
							</div>
						)}
						<div className="modal-actions">
							{editor.kind !== "auto_asr" && editor.index !== null && (
								<Button tone="quiet" disabled={locked} onClick={removeEditor}>
									{editor.kind === "tag" ? "移除标签" : "移除策略"}
								</Button>
							)}
							<Button tone="quiet" disabled={busy} onClick={closeEditor}>
								{retry || conflict ? "关闭" : "取消"}
							</Button>
							{resolution ??
								(extendAsr ? (
									<Button tone="primary" onClick={() => applyEditor(true)}>
										补齐精转标签并保存
									</Button>
								) : (
									<Button
										type="submit"
										tone="primary"
										disabled={locked || !editorValid}
									>
										保存
									</Button>
								))}
						</div>
					</form>
				</Modal>
			)}
		</TabPanel>
	);
}
