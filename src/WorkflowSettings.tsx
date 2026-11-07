import { useEffect, useState } from "react";
import { Button, Field, Modal, SegmentedOptions, TabPanel } from "./Controls";
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

export type WorkflowSection = "labels" | "automation";
type LabelOption = WorkflowLabel & { removed?: boolean };
type Editor =
	| { kind: "tag"; index: number | null; value: WorkflowTag }
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
	disabled,
}: {
	tags: LabelOption[];
	ids: string[];
	onChange: (ids: string[]) => void;
	disabled?: boolean;
}) {
	return (
		<div className="workflow-tag-options">
			{tags
				.filter((tag) => !tag.removed || ids.includes(tag.id))
				.map((tag) => (
					<label key={tag.id}>
						<input
							type="checkbox"
							disabled={disabled}
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
			setMessage("已保存。新配置适用于之后进入流程的事件。");
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
					? document
							.getElementById(`${tabsId}-auto-asr`)
							?.querySelector<HTMLInputElement>("input:checked")
					: document.getElementById(`${tabsId}-strategy-${target}`);
			(control ?? document.getElementById(`${tabsId}-tab-automation`))?.focus();
		});
	}

	if (!section) return null;
	if (!draft || !saved) {
		return (
			<TabPanel tabsId={tabsId} value={section} selected={section}>
				<div className="panel-card" role="status">
					{message || "正在读取智能标签与自动流程…"}
					{message && (
						<Button tone="quiet" onClick={reload}>
							重试
						</Button>
					)}
				</div>
			</TabPanel>
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
	const editorText =
		editor?.kind === "tag" ? editor.value.criteria : editor?.value.prompt;
	const editorValid = Boolean(editor?.value.name && editorText);
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
		if (editor.kind === "tag") {
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
		if (!draft || !editor || editor.index === null) return;
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

	return (
		<TabPanel tabsId={tabsId} value={section} selected={section}>
			<div className="workflow-settings">
				{section === "labels" ? (
					<>
						<section className="panel-card workflow-config-card">
							<div className="workflow-config-heading">
								<div>
									<h2>标注方式</h2>
									<p>
										整理粗转录时，按你的判断标准给事件打标。修改只影响之后的自动标注。
									</p>
								</div>
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
							<p className="workflow-config-hint">
								{draft.mode === "single"
									? "每个事件取最符合的一项。"
									: "每个事件可以命中多个具体标签。"}
							</p>
						</section>
						<section className="panel-card workflow-config-card">
							<div className="workflow-config-heading">
								<div>
									<h2>标签库</h2>
									<p>定义容易区分的场景。新标签保存后可用于自动流程。</p>
								</div>
								<Button
									tone="secondary"
									disabled={busy}
									onClick={() =>
										setEditor({
											kind: "tag",
											index: null,
											value: { id: NEW_ID, name: "", criteria: "" },
										})
									}
								>
									添加标签
								</Button>
							</div>
							{draft.tags.length === 0 && (
								<p className="workflow-config-empty">还没有自定义标签。</p>
							)}
							{draft.tags.map((tag, index) => (
								<div
									className="workflow-config-row"
									key={tag.id === NEW_ID ? `new-${index}` : tag.id}
								>
									<div>
										<strong className="workflow-chip">{tag.name}</strong>
										<p>{tag.criteria}</p>
									</div>
									<Button
										tone="quiet"
										aria-label={`编辑标签 · ${tag.name}`}
										disabled={busy}
										onClick={() =>
											setEditor({ kind: "tag", index, value: tag })
										}
									>
										编辑
									</Button>
								</div>
							))}
							<div className="workflow-config-row workflow-fallback-row">
								<div>
									<strong className="workflow-chip">其他</strong>
									<p>判断有效、但没有命中具体标签时使用。</p>
								</div>
								<small>系统兜底</small>
							</div>
						</section>
					</>
				) : (
					<>
						<section
							id={`${tabsId}-auto-asr`}
							className="panel-card workflow-config-card"
						>
							<div className="workflow-config-heading">
								<div>
									<h2>自动精转</h2>
									<p>事件确定后判断是否安排精转录。已有稿件会优先复用。</p>
								</div>
								<SegmentedOptions
									label="自动精转条件"
									value={draft.auto_asr.mode}
									options={[
										{ value: "all", label: "全部有效事件" },
										{ value: "tags", label: "包含指定标签" },
									]}
									disabled={busy}
									onChange={(mode) =>
										update({
											...draft,
											auto_asr:
												mode === "all"
													? { mode: "all" }
													: { mode: "tags", tag_ids: [] },
										})
									}
								/>
							</div>
							{draft.auto_asr.mode === "tags" && (
								<div className="workflow-config-condition">
									<span>命中任意一项时安排精转</span>
									<TagChoices
										tags={labels}
										ids={draft.auto_asr.tag_ids}
										disabled={busy}
										onChange={(tag_ids) =>
											update({ ...draft, auto_asr: { mode: "tags", tag_ids } })
										}
									/>
								</div>
							)}
							<p className="workflow-config-hint">
								需要补转时，选中的事件音频会发送给已配置的精转服务。
							</p>
						</section>
						<section className="panel-card workflow-config-card">
							<div className="workflow-config-heading">
								<div>
									<h2>生成策略</h2>
									<p>
										设定转录来源和生成指令。粗转策略可直接执行，精转策略等待稿件就绪。
									</p>
								</div>
								<Button
									tone="secondary"
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
									添加策略
								</Button>
							</div>
							{draft.strategies.length === 0 && (
								<p className="workflow-config-empty">
									还没有生成策略。可从会议纪要或待办清单开始。
								</p>
							)}
							{draft.strategies.map((strategy, index) => (
								<div
									className="workflow-config-row"
									key={strategy.id === NEW_ID ? `new-${index}` : strategy.id}
								>
									<div>
										<strong>{strategy.name}</strong>
										<div className="workflow-strategy-meta">
											<span className="workflow-chip">
												{inputLabel(strategy.input)}
											</span>
											<span>
												{strategy.tag_ids.length ? "自动触发" : "仅手动"}
											</span>
											{strategy.tag_ids.map((id) => (
												<span className="workflow-chip" key={id}>
													{labelName(id)}
												</span>
											))}
										</div>
									</div>
									<Button
										tone="quiet"
										id={`${tabsId}-strategy-${index}`}
										aria-label={`编辑策略 · ${strategy.name}`}
										disabled={busy}
										onClick={() =>
											setEditor({ kind: "strategy", index, value: strategy })
										}
									>
										编辑
									</Button>
								</div>
							))}
						</section>
					</>
				)}
				{(dirty || message) && (
					<div className="workflow-config-actions">
						<p role="status">
							{message || "有未保存的改动，保存后两页配置一起生效。"}
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
					title={`${editor.index === null ? "添加" : "编辑"}${editor.kind === "tag" ? "标签" : "策略"}`}
					onClose={() => setEditor(null)}
				>
					<form
						className="workflow-config-editor"
						onSubmit={(event) => {
							event.preventDefault();
							applyEditor();
						}}
					>
						<Field label={editor.kind === "tag" ? "标签名称" : "策略名称"}>
							{(attributes) => (
								<input
									{...attributes}
									value={editor.value.name}
									placeholder={
										editor.kind === "tag" ? "例如：工作会议" : "例如：会议纪要"
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
							<Field
								label="判断标准"
								description="描述哪些内容应命中这个标签，必要时写清排除条件。"
							>
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
								<Field label="输入来源">
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
								<Field
									label="生成指令"
									description="说明要提取的信息和输出格式。所选转录稿会发送给已配置的生成服务。"
								>
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
									去自动流程调整
								</Button>
							</div>
						)}
						<div className="modal-actions">
							{editor.index !== null && (
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
							去自动流程调整
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
		</TabPanel>
	);
}
