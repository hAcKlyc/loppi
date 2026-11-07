import { useCallback, useEffect, useState } from "react";
import { Button, Modal } from "./Controls";
import {
	type WorkflowSettings as Settings,
	selectedIds,
	useWorkflowRequestId,
	type WorkflowConfig,
	WorkflowError,
	type WorkflowLabel,
	type WorkflowStrategy,
	workflow,
} from "./workflow-api";

const NEW_ID = "00000000-0000-0000-0000-000000000000";

function TagChoices({
	tags,
	ids,
	disabled,
	onChange,
}: {
	tags: (WorkflowLabel & { removed?: boolean })[];
	ids: string[];
	disabled: boolean;
	onChange: (ids: string[]) => void;
}) {
	return (
		<div className="workflow-tag-options">
			{tags.map((tag) => (
				<label key={tag.id}>
					<input
						type="checkbox"
						disabled={disabled || (tag.removed && !ids.includes(tag.id))}
						checked={ids.includes(tag.id)}
						onChange={(event) =>
							onChange(selectedIds(ids, tag.id, event.target.checked))
						}
					/>
					{tag.name}
					{tag.removed ? "（已移除，取消勾选以解除引用）" : ""}
				</label>
			))}
		</div>
	);
}

export function WorkflowSettings() {
	const [saved, setSaved] = useState<Settings | null>(null);
	const [draft, setDraft] = useState<WorkflowConfig | null>(null);
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	const [affected, setAffected] = useState<
		{ name?: string; tag_ids: string[] }[]
	>([]);
	const [canAdd, setCanAdd] = useState(false);
	const requestId = useWorkflowRequestId();

	const load = useCallback(async () => {
		try {
			const settings = await workflow<Settings>({ op: "config_show" });
			setSaved(settings);
			setDraft(settings.config);
			setMessage("");
		} catch (error) {
			setMessage(String(error instanceof Error ? error.message : error));
		}
	}, []);
	useEffect(() => {
		void load();
	}, [load]);

	function update(next: WorkflowConfig) {
		setDraft(next);
		setMessage("");
		setAffected([]);
		setCanAdd(false);
	}

	async function save(add = false) {
		if (!saved || !draft || busy) return;
		setBusy(true);
		setMessage("");
		try {
			const action = {
				op: "config_set",
				config: draft,
				expected_revision: saved.revision,
				add_required_asr_tags: add,
			};
			const value = await workflow<Settings>(action, requestId(action));
			setSaved(value);
			update(value.config);
			setMessage("设置已保存");
		} catch (error) {
			if (error instanceof WorkflowError) {
				const details = error.details as { affected?: typeof affected } | null;
				setAffected(details?.affected ?? []);
				setCanAdd(error.code === "asr_rule_conflict");
			}
			setMessage(error instanceof Error ? error.message : "设置未保存");
		} finally {
			setBusy(false);
		}
	}

	if (!draft || !saved) {
		return (
			<div className="panel-card setting-panel" role="status">
				{message || "正在读取智能标签与自动流程…"}
				{message && (
					<Button tone="quiet" onClick={() => void load()}>
						重试
					</Button>
				)}
			</div>
		);
	}

	const tags = [
		...draft.tags.filter((tag) => tag.id !== NEW_ID),
		...saved.config.tags
			.filter((tag) => !draft.tags.some((current) => current.id === tag.id))
			.map((tag) => ({ ...tag, removed: true })),
		{ id: saved.other_tag, name: "其他" },
	];
	function updateStrategy(index: number, strategy: WorkflowStrategy) {
		if (!draft) return;
		update({
			...draft,
			strategies: draft.strategies.map((item, i) =>
				i === index ? strategy : item,
			),
		});
	}
	const dirty = draft !== saved.config;
	const tagName = (id: string) =>
		tags.find((tag) => tag.id === id)?.name ??
		saved.config.tags.find((tag) => tag.id === id)?.name ??
		id;

	return (
		<div className="workflow-settings">
			<section className="panel-card setting-panel">
				<div className="setting-row">
					<div>
						<strong>智能标签</strong>
						<p>按你定义的标准，在整理粗转录时为事件打标。</p>
					</div>
					<label>
						标注机制{" "}
						<select
							value={draft.mode}
							disabled={busy}
							onChange={(event) =>
								update({
									...draft,
									mode: event.target.value === "single" ? "single" : "multiple",
								})
							}
						>
							<option value="single">单选</option>
							<option value="multiple">多选</option>
						</select>
					</label>
				</div>
				{draft.tags.map((tag, index) => (
					<div
						className="workflow-setting-item"
						key={tag.id === NEW_ID ? `tag-${index}` : tag.id}
					>
						<label>
							标签名称
							<input
								disabled={busy}
								value={tag.name}
								onChange={(event) =>
									update({
										...draft,
										tags: draft.tags.map((item, i) =>
											i === index
												? { ...item, name: event.target.value }
												: item,
										),
									})
								}
							/>
						</label>
						<label>
							判断标准
							<textarea
								disabled={busy}
								value={tag.criteria}
								onChange={(event) =>
									update({
										...draft,
										tags: draft.tags.map((item, i) =>
											i === index
												? { ...item, criteria: event.target.value }
												: item,
										),
									})
								}
							/>
						</label>
						<Button
							tone="quiet"
							disabled={busy}
							onClick={() =>
								update({
									...draft,
									tags: draft.tags.filter((_, i) => i !== index),
								})
							}
						>
							移除标签
						</Button>
					</div>
				))}
				<div className="setting-row">
					<div>
						<strong>其他</strong>
						<p>有效判断没有命中具体标签时使用。</p>
					</div>
					<Button
						tone="quiet"
						disabled={busy}
						onClick={() =>
							update({
								...draft,
								tags: [...draft.tags, { id: NEW_ID, name: "", criteria: "" }],
							})
						}
					>
						添加标签
					</Button>
				</div>
			</section>
			<section className="panel-card setting-panel">
				<div className="setting-row">
					<div>
						<strong>自动流程</strong>
						<p>
							事件确定后判断精转和生成。精转会发送音频，生成会发送所选转录稿。
						</p>
					</div>
					<label>
						自动精转录{" "}
						<select
							value={draft.auto_asr.mode}
							disabled={busy}
							onChange={(event) =>
								update({
									...draft,
									auto_asr:
										event.target.value === "all"
											? { mode: "all" }
											: { mode: "tags", tag_ids: [] },
								})
							}
						>
							<option value="all">全部有效事件</option>
							<option value="tags">包含指定标签</option>
						</select>
					</label>
				</div>
				{draft.auto_asr.mode === "tags" && (
					<TagChoices
						tags={tags}
						ids={draft.auto_asr.tag_ids}
						disabled={busy}
						onChange={(tag_ids) =>
							update({ ...draft, auto_asr: { mode: "tags", tag_ids } })
						}
					/>
				)}
				{draft.strategies.map((strategy, index) => (
					<div
						className="workflow-setting-item"
						key={strategy.id === NEW_ID ? `strategy-${index}` : strategy.id}
					>
						<label>
							策略名称
							<input
								value={strategy.name}
								disabled={busy}
								onChange={(event) =>
									updateStrategy(index, {
										...strategy,
										name: event.target.value,
									})
								}
							/>
						</label>
						<label>
							Prompt
							<textarea
								rows={4}
								value={strategy.prompt}
								disabled={busy}
								onChange={(event) =>
									updateStrategy(index, {
										...strategy,
										prompt: event.target.value,
									})
								}
							/>
						</label>
						<label>
							输入来源
							<select
								value={strategy.input}
								disabled={busy}
								onChange={(event) =>
									updateStrategy(index, {
										...strategy,
										input:
											event.target.value === "precise" ? "precise" : "coarse",
									})
								}
							>
								<option value="coarse">粗转录</option>
								<option value="precise">精转录</option>
							</select>
						</label>
						<span>
							触发标签
							{strategy.tag_ids.length === 0
								? " · 仅手动"
								: " · 命中任一时自动执行"}
						</span>
						<TagChoices
							tags={tags}
							ids={strategy.tag_ids}
							disabled={busy}
							onChange={(tag_ids) =>
								updateStrategy(index, { ...strategy, tag_ids })
							}
						/>
						<Button
							tone="quiet"
							disabled={busy}
							onClick={() =>
								update({
									...draft,
									strategies: draft.strategies.filter((_, i) => i !== index),
								})
							}
						>
							移除策略
						</Button>
					</div>
				))}
				<div className="setting-row">
					<Button
						tone="quiet"
						disabled={busy}
						onClick={() =>
							update({
								...draft,
								strategies: [
									...draft.strategies,
									{
										id: NEW_ID,
										name: "",
										prompt: "",
										input: "coarse",
										tag_ids: [],
									},
								],
							})
						}
					>
						添加策略
					</Button>
					<Button
						tone="primary"
						disabled={!dirty || busy}
						onClick={() => void save()}
					>
						保存设置
					</Button>
				</div>
				{message && <p role="status">{message}</p>}
				{affected.length > 0 && (
					<Modal
						title={canAdd ? "补齐自动精转标签" : "配置仍有依赖"}
						description={message}
						busy={busy}
						onClose={() => setAffected([])}
					>
						<ul>
							{affected.map((item) => (
								<li key={`${item.name ?? "asr"}-${item.tag_ids.join(",")}`}>
									{item.name ?? "自动精转录"} ·{" "}
									{item.tag_ids.map(tagName).join("、")}
								</li>
							))}
						</ul>
						<div className="modal-actions">
							<Button
								tone="quiet"
								disabled={busy}
								onClick={() => setAffected([])}
							>
								返回设置
							</Button>
							{canAdd && (
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
			</section>
		</div>
	);
}
