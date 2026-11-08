import { useEffect, useRef, useState } from "react";
import type { AnalysisSection } from "./AnalysisOverview";
import {
	Button,
	Field,
	Icon,
	IconButton,
	Modal,
	SegmentedOptions,
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

type WorkflowSection = Exclude<AnalysisSection, "coarse">;
type LabelOption = WorkflowLabel & { removed?: boolean };
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

function TagChoices({
	tags,
	ids,
	onChange,
}: {
	tags: LabelOption[];
	ids: string[];
	onChange: (ids: string[]) => void;
}) {
	return (
		<div className="workflow-tag-options">
			{tags
				.filter((tag) => !tag.removed || ids.includes(tag.id))
				.map((tag) => (
					<label key={tag.id}>
						<input
							type="checkbox"
							checked={ids.includes(tag.id)}
							onChange={(event) =>
								onChange(selectedIds(ids, tag.id, event.target.checked))
							}
						/>
						{tag.name}
						{tag.removed && "（已移除，请取消勾选）"}
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
	const [draft, setDraft] = useState<WorkflowConfig | null>(null);
	const [loadAttempt, setLoadAttempt] = useState(0);
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	const [editor, setEditor] = useState<Editor | null>(null);
	const selectedAsrMode = useRef<HTMLInputElement>(null);
	const [conflict, setConflict] = useState(false);
	const [dependency, setDependency] = useState<{
		canAdd: boolean;
		affected: Dependency[];
	} | null>(null);
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
				setDraft(value.config);
				setMessage("");
			},
			(error: unknown) => {
				if (live)
					setMessage(error instanceof Error ? error.message : "无法读取配置");
			},
		);
		return () => {
			live = false;
		};
	}, [active, saved, loadAttempt]);

	function update(next: WorkflowConfig) {
		setDraft(next);
		setMessage("");
		setDependency(null);
	}

	async function save(addRequiredTags = false) {
		if (!saved || !draft || busy || conflict) return;
		setBusy(true);
		setMessage("");
		try {
			const action = {
				op: "config_set",
				config: draft,
				expected_revision: saved.revision,
				add_required_asr_tags: addRequiredTags,
			};
			const value = await workflow<Settings>(action, requestId(action));
			setSaved(value);
			setDraft(value.config);
			setDependency(null);
			setMessage("已保存 · 对后续事件生效");
		} catch (error) {
			if (error instanceof WorkflowError) {
				const affected =
					(error.details as { affected?: Dependency[] } | null)?.affected ?? [];
				setDependency(
					affected.length
						? { canAdd: error.code === "asr_rule_conflict", affected }
						: null,
				);
				setConflict(error.code === "revision_conflict");
			}
			setMessage(error instanceof Error ? error.message : "配置未保存");
		} finally {
			setBusy(false);
		}
	}

	function reload() {
		setSaved(null);
		setDraft(null);
		setMessage("");
		setConflict(false);
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
	if (!draft || !saved) {
		return (
			<div className="panel-card" role="status">
				{message || "正在读取智能分析配置…"}
				{message && (
					<Button tone="quiet" onClick={reload}>
						重试
					</Button>
				)}
			</div>
		);
	}

	const dirty = draft !== saved.config;
	const labels = [
		...draft.tags.filter((tag) => tag.id !== NEW_ID),
		...saved.config.tags
			.filter((tag) => !draft.tags.some((item) => item.id === tag.id))
			.map((tag) => ({ ...tag, removed: true })),
		{ id: saved.other_tag, name: "其他" },
	];
	const labelName = (id: string) =>
		labels.find((tag) => tag.id === id)?.name ?? id;
	const editorValid =
		editor?.kind === "auto_asr" ||
		Boolean(
			editor?.value.name &&
				(editor.kind === "tag" ? editor.value.criteria : editor.value.prompt),
		);
	const editedTagId =
		editor?.kind === "tag" && editor.value.id !== NEW_ID
			? editor.value.id
			: undefined;
	const asrReference =
		editedTagId !== undefined &&
		draft.auto_asr.mode === "tags" &&
		draft.auto_asr.tag_ids.includes(editedTagId);
	const strategyReferences =
		editedTagId === undefined
			? []
			: draft.strategies.flatMap((strategy, index) =>
					strategy.tag_ids.includes(editedTagId)
						? [{ name: strategy.name, index }]
						: [],
				);
	const referencedBy = [
		...(asrReference ? ["自动精转"] : []),
		...strategyReferences.map(({ name }) => name),
	];

	function openAffectedRule(rule: Dependency | undefined) {
		if (rule?.kind === "auto_asr") {
			openAutomation("auto_asr");
			return;
		}
		// Core trims new names before reporting conflicts. Use its whitespace
		// semantics only to find the row, without modifying the editor value.
		const index =
			rule &&
			draft?.strategies.findIndex((strategy) =>
				rule.strategy_id === NEW_ID
					? strategy.id === NEW_ID &&
						strategy.name.replace(
							/^\p{White_Space}+|\p{White_Space}+$/gu,
							"",
						) === rule.name
					: strategy.id === rule.strategy_id,
			);
		openAutomation(index);
	}

	function applyEditor() {
		if (!draft || !editor || !editorValid) return;
		if (editor.kind === "auto_asr") {
			if (editor.value !== draft.auto_asr)
				update({ ...draft, auto_asr: editor.value });
		} else if (editor.kind === "tag") {
			update({
				...draft,
				tags: editList(draft.tags, editor.index, editor.value),
			});
		} else {
			update({
				...draft,
				strategies: editList(draft.strategies, editor.index, editor.value),
			});
		}
		setEditor(null);
	}

	function removeEditor() {
		if (
			!draft ||
			!editor ||
			editor.kind === "auto_asr" ||
			editor.index === null
		)
			return;
		update(
			editor.kind === "tag"
				? {
						...draft,
						tags: draft.tags.filter((_, index) => index !== editor.index),
					}
				: {
						...draft,
						strategies: draft.strategies.filter(
							(_, index) => index !== editor.index,
						),
					},
		);
		setEditor(null);
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

	const asrTriggers =
		draft.auto_asr.mode === "all"
			? "全部有效事件"
			: triggerSummary(draft.auto_asr.tag_ids, "未选择标签");
	const editAutoAsr = (
		<Button
			id={`${tabsId}-auto-asr`}
			tone="quiet"
			size="compact"
			className="workflow-flow-edit"
			aria-label="编辑自动精转"
			disabled={busy}
			onClick={() => setEditor({ kind: "auto_asr", value: draft.auto_asr })}
		>
			编辑
		</Button>
	);

	return (
		<>
			<div className="workflow-settings">
				{section === "labels" ? (
					<>
						<div className="workflow-config-rule">
							<span>每个事件</span>
							<SegmentedOptions
								label="标注方式"
								value={draft.mode}
								options={[
									{ value: "single", label: "单选" },
									{ value: "multiple", label: "多选" },
								]}
								disabled={busy}
								onChange={(mode) => update({ ...draft, mode })}
							/>
						</div>
						<table className="workflow-config-table" aria-label="标签库">
							<thead>
								<tr>
									<th scope="col">标签</th>
									<th scope="col">判断标准</th>
									<th scope="col" aria-label="操作">
										<Button
											tone="primary"
											size="compact"
											disabled={busy}
											onClick={() =>
												setEditor({
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
								{draft.tags.map((tag, index) => (
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
												disabled={busy}
												onClick={() =>
													setEditor({ kind: "tag", index, value: tag })
												}
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
				) : section === "precise" ? (
					<div className="panel-card setting-row">
						<div>
							<strong>自动精转</strong>
							<div>{asrTriggers}</div>
						</div>
						{editAutoAsr}
					</div>
				) : (
					<table
						className="workflow-config-table workflow-config-strategies"
						aria-label="自动流程"
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
										disabled={busy}
										onClick={() =>
											setEditor({
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
								<td>{asrTriggers}</td>
								<td>{editAutoAsr}</td>
							</tr>
							{draft.strategies.map((strategy, index) => (
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
											disabled={busy}
											onClick={() =>
												setEditor({
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
				{(dirty || message) && (
					<div className="workflow-config-actions">
						<p role="status">
							{message ||
								(draft.mode !== saved.config.mode
									? "有未保存的改动 · 新标注方式适用于后续事件"
									: "有未保存的改动")}
						</p>
						{conflict ? (
							<Button tone="secondary" onClick={reload}>
								放弃草稿并重新加载
							</Button>
						) : (
							dirty && (
								<div>
									<Button
										tone="quiet"
										disabled={busy}
										onClick={() => update(saved.config)}
									>
										放弃改动
									</Button>
									<Button
										tone="primary"
										busy={busy}
										busyLabel="正在保存"
										onClick={() => void save()}
									>
										保存配置
									</Button>
								</div>
							)
						)}
					</div>
				)}
			</div>
			{editor && (
				<Modal
					title={
						editor.kind === "auto_asr"
							? "编辑自动精转"
							: `${editor.index === null ? "添加" : "编辑"}${editor.kind === "tag" ? "标签" : "策略"}`
					}
					onClose={() => setEditor(null)}
					initialFocusRef={
						editor.kind === "auto_asr" ? selectedAsrMode : undefined
					}
				>
					<form
						className="workflow-config-editor"
						onSubmit={(event) => {
							event.preventDefault();
							applyEditor();
						}}
					>
						{editor.kind === "auto_asr" ? (
							<>
								<div className="workflow-config-rule-options">
									<span>触发条件</span>
									<SegmentedOptions
										label="自动精转条件"
										value={editor.value.mode}
										selectedRef={selectedAsrMode}
										options={[
											{ value: "all", label: "全部有效事件" },
											{ value: "tags", label: "包含任一标签" },
										]}
										onChange={(mode) =>
											setEditor({
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
											ids={editor.value.tag_ids}
											onChange={(tag_ids) =>
												setEditor({
													kind: "auto_asr",
													value: { mode: "tags", tag_ids },
												})
											}
										/>
									)}
								</div>
								<details className="workflow-config-disclosure">
									<summary>音频发送</summary>
									<p>
										补转时会把符合条件的音频发送给精转服务。已有稿件会优先复用。
									</p>
								</details>
							</>
						) : (
							<>
								<Field label={editor.kind === "tag" ? "标签名称" : "策略名称"}>
									{(attributes) => (
										<input
											{...attributes}
											value={editor.value.name}
											placeholder={
												editor.kind === "tag"
													? "例如：工作会议"
													: "例如：会议纪要"
											}
											onChange={(event) =>
												setEditor({
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
												rows={4}
												value={editor.value.criteria}
												placeholder="例如：讨论项目进展、决策或工作安排。排除一般寒暄。"
												onChange={(event) =>
													setEditor({
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
													value={editor.value.input}
													onChange={(event) =>
														setEditor({
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
													rows={6}
													value={editor.value.prompt}
													placeholder="例如：用 Markdown 列出决策与待办，保留明确的负责人和截止日期。"
													onChange={(event) =>
														setEditor({
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
												ids={editor.value.tag_ids}
												onChange={(tag_ids) =>
													setEditor({
														...editor,
														value: { ...editor.value, tag_ids },
													})
												}
											/>
											{draft.tags.some((tag) => tag.id === NEW_ID) && (
												<small>新建标签保存配置后才可选择。</small>
											)}
										</div>
									</>
								)}
							</>
						)}
						{referencedBy.length > 0 && (
							<div className="workflow-config-hint">
								已被 {referencedBy.join("、")} 引用。
								<Button
									tone="quiet"
									onClick={() => {
										setEditor(null);
										openAutomation(
											asrReference ? "auto_asr" : strategyReferences[0]?.index,
										);
									}}
								>
									去自定义策略调整
								</Button>
							</div>
						)}
						<div className="modal-actions">
							{editor.kind !== "auto_asr" && editor.index !== null && (
								<Button tone="quiet" onClick={removeEditor}>
									{editor.kind === "tag" ? "移除标签" : "移除策略"}
								</Button>
							)}
							<Button tone="quiet" onClick={() => setEditor(null)}>
								取消
							</Button>
							<Button type="submit" tone="primary" disabled={!editorValid}>
								应用到草稿
							</Button>
						</div>
					</form>
				</Modal>
			)}
			{dependency && (
				<Modal
					title={dependency.canAdd ? "补齐自动精转标签" : "配置仍有依赖"}
					description={message}
					busy={busy}
					onClose={() => setDependency(null)}
				>
					<ul>
						{dependency.affected.map((item) => (
							<li key={`${item.name ?? "asr"}-${item.tag_ids.join(",")}`}>
								{item.name ?? "自动精转"}
								{item.tag_ids.length
									? ` · ${item.tag_ids.map(labelName).join("、")}`
									: " · 请至少选一个标签"}
							</li>
						))}
					</ul>
					<div className="modal-actions">
						<Button
							tone="quiet"
							disabled={busy}
							onClick={() => {
								setDependency(null);
								openAffectedRule(dependency.affected[0]);
							}}
						>
							去自定义策略调整
						</Button>
						{dependency.canAdd && (
							<Button
								tone="primary"
								disabled={busy}
								onClick={() => void save(true)}
							>
								加入并保存
							</Button>
						)}
					</div>
				</Modal>
			)}
		</>
	);
}
