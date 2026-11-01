import { useCallback, useEffect, useRef, useState } from "react";
import { core, userError } from "./api";
import { Button, Field, Icon, Modal, SettingRow, Switch } from "./Controls";
import { LoadBoundary } from "./LoadingState";

type Entry = {
	entity_id: string;
	name: string;
	kind: string;
	aliases: string[];
	enabled: boolean;
};
type Settings = { llm: boolean; local_asr: boolean; cloud_asr: boolean };
type Vocabulary = { entries: Entry[]; settings: Settings };
type Action =
	| { action: "list" }
	| {
			action: "upsert";
			id: string | null;
			name: string;
			kind: string;
			aliases: string[];
			enabled: boolean;
	  }
	| { action: "remove"; id: string }
	| ({ action: "configure" } & Partial<Settings>);

const kinds = [
	{ value: "product", label: "产品" },
	{ value: "project", label: "项目" },
	{ value: "organization", label: "组织" },
	{ value: "person", label: "人名" },
	{ value: "term", label: "专业术语" },
];
const kindLabel = (kind: string) =>
	kinds.find((item) => item.value === kind)?.label ?? kind;

function vocabularyError(cause: unknown): string {
	const message = cause instanceof Error ? cause.message : String(cause);
	if (message.includes("already assigned"))
		return "这个名称或别名已被其他启用的词条使用，请调整后保存。";
	if (message.includes("100 entries"))
		return "词库最多保存 100 个词条，请先删除不再使用的词条。";
	if (message.includes("64 KiB")) return "词库内容已达上限，请精简词条或别名。";
	if (message.includes("invalid vocabulary entry"))
		return "请检查词条：名称最多 100 字，类型最多 50 字，别名最多 20 个且每个不超过 100 字，不能包含控制字符。";
	return userError(cause);
}

async function requestVocabulary(action: Action): Promise<Vocabulary> {
	const response = await core<{
		type: "context";
		id: string;
		result:
			| { ok: true; data: Vocabulary }
			| { ok: false; error: { code: string; message: string } };
	}>({ type: "vocabulary", action });
	if (!response.result.ok)
		throw new Error(
			`${response.result.error.code}: ${response.result.error.message}`,
		);
	return response.result.data;
}

export function VocabularyView() {
	const [data, setData] = useState<Vocabulary | null>(null);
	const [loadError, setLoadError] = useState("");
	const [error, setError] = useState("");
	const [busy, setBusy] = useState(false);
	const [editor, setEditor] = useState<Entry | "new" | null>(null);
	const [notice, setNotice] = useState("");
	const epoch = useRef(0);
	const submitting = useRef(false);
	const load = useCallback(async () => {
		if (submitting.current) return;
		const token = ++epoch.current;
		setLoadError("");
		try {
			const result = await requestVocabulary({ action: "list" });
			if (token === epoch.current) setData(result);
		} catch (cause) {
			if (token === epoch.current) setLoadError(vocabularyError(cause));
		}
	}, []);
	useEffect(() => {
		void load();
		return () => {
			epoch.current++;
		};
	}, [load]);
	const mutate = async (action: Action, message: string): Promise<boolean> => {
		if (submitting.current) return false;
		submitting.current = true;
		const token = ++epoch.current;
		setBusy(true);
		setError("");
		setNotice("");
		try {
			const result = await requestVocabulary(action);
			if (token !== epoch.current) return false;
			setData(result);
			setLoadError("");
			setNotice(message);
			return true;
		} catch (cause) {
			if (token === epoch.current) setError(vocabularyError(cause));
			return false;
		} finally {
			submitting.current = false;
			if (token === epoch.current) setBusy(false);
		}
	};
	return (
		<div className="vocabulary-page">
			<div className="page-heading" data-tauri-drag-region>
				<h1>词库</h1>
				<div className="vocabulary-actions">
					<Button disabled={busy} onClick={() => void load()}>
						刷新
					</Button>
					<Button
						className="button-with-icon"
						disabled={busy || !data || data.entries.length >= 100}
						onClick={() => {
							setError("");
							setEditor("new");
						}}
					>
						<Icon name="plus" />
						新建词条
					</Button>
				</div>
			</div>
			<p className="vocabulary-intro">
				记住常用的产品名、项目名和专有名词，让转录与摘要用对词。
			</p>
			<p className="vocabulary-feedback" role="status">
				{notice}
			</p>
			{error && !editor && (
				<p className="form-error" role="alert">
					{error}
				</p>
			)}
			<LoadBoundary
				ready={data !== null}
				error={loadError}
				retry={() => void load()}
				label="词库"
			>
				{data && (
					<>
						<div className="vocabulary-list-heading">
							<h2>我的词条</h2>
							<span>{data.entries.length} / 100</span>
						</div>
						<div className="panel-card vocabulary-list">
							{data.entries.length === 0 && (
								<div className="list-empty">
									<strong>从一个常用名字开始</strong>
									<p>例如添加「看山」，把常被听成的「开山」设为别名。</p>
									<Button
										disabled={busy}
										onClick={() => {
											setError("");
											setEditor("new");
										}}
									>
										添加第一个词条
									</Button>
								</div>
							)}
							{data.entries.map((entry) => (
								<button
									type="button"
									className="vocabulary-row"
									key={entry.entity_id}
									disabled={busy}
									onClick={() => {
										setError("");
										setEditor(entry);
									}}
								>
									<span className="vocabulary-word">
										<strong>{entry.name}</strong>
										<small>
											{entry.aliases.length
												? `别名 / 常见误写：${entry.aliases.join("、")}`
												: "尚未添加别名"}
										</small>
									</span>
									<span className="vocabulary-kind">
										{kindLabel(entry.kind)}
									</span>
									{!entry.enabled && (
										<span className="vocabulary-disabled">已停用</span>
									)}
									<Icon name="next" />
								</button>
							))}
						</div>
						<div className="vocabulary-settings">
							<h2>应用词库</h2>
							<p>
								对后续识别和生成生效。已有文字与摘要保持原样；已提交的精转任务沿用提交时的词库。
							</p>
							{(
								[
									[
										"llm",
										"摘要与理解",
										"帮助整理事件时使用规范名称，关联同一对象。",
									],
									[
										"local_asr",
										"本地转录",
										"优先识别词库中的名称，实际效果取决于音频与模型。",
									],
									[
										"cloud_asr",
										"云端精转",
										"将启用的规范名称作为热词随精转请求发送给云端服务。",
									],
								] as const
							).map(([key, title, description]) => (
								<SettingRow
									key={key}
									title={title}
									description={description}
									action={
										<Switch
											label={title}
											checked={data.settings[key]}
											disabled={busy}
											onChange={() =>
												void mutate(
													{ action: "configure", [key]: !data.settings[key] },
													`${title}词库已${data.settings[key] ? "关闭" : "开启"}`,
												)
											}
										/>
									}
								/>
							))}
						</div>
					</>
				)}
			</LoadBoundary>
			{editor && (
				<EntryEditor
					key={editor === "new" ? "new" : editor.entity_id}
					entry={editor === "new" ? null : editor}
					entries={data?.entries ?? []}
					busy={busy}
					error={error}
					onClearError={() => setError("")}
					onClose={() => {
						setEditor(null);
						setError("");
					}}
					onSave={mutate}
				/>
			)}
		</div>
	);
}

function EntryEditor({
	entry,
	entries,
	busy,
	error,
	onClearError,
	onClose,
	onSave,
}: {
	entry: Entry | null;
	entries: Entry[];
	busy: boolean;
	error: string;
	onClearError: () => void;
	onClose: () => void;
	onSave: (action: Action, message: string) => Promise<boolean>;
}) {
	const [name, setName] = useState(entry?.name ?? "");
	const [kind, setKind] = useState(entry?.kind ?? "product");
	const [aliases, setAliases] = useState(entry?.aliases.join("\n") ?? "");
	const [enabled, setEnabled] = useState(entry?.enabled ?? true);
	const [deleting, setDeleting] = useState(false);
	const [validation, setValidation] = useState("");
	const submit = useRef(false);
	const clear = () => {
		setValidation("");
		onClearError();
	};
	return (
		<Modal
			title={deleting ? "删除词条？" : entry ? "编辑词条" : "新建词条"}
			busy={busy}
			onClose={onClose}
		>
			{deleting ? (
				<>
					<p>
						删除「{entry?.name}
						」后，后续识别与摘要不再使用这个词条。已有记录不会改变。
					</p>
					{error && (
						<p className="form-error" role="alert">
							{error}
						</p>
					)}
					<div className="modal-actions">
						<Button
							disabled={busy}
							onClick={() => {
								setDeleting(false);
								clear();
							}}
						>
							返回编辑
						</Button>
						<Button
							tone="danger"
							busy={busy}
							onClick={async () => {
								if (!entry || submit.current) return;
								submit.current = true;
								try {
									if (
										await onSave(
											{ action: "remove", id: entry.entity_id },
											"词条已删除",
										)
									)
										onClose();
								} finally {
									submit.current = false;
								}
							}}
						>
							确认删除
						</Button>
					</div>
				</>
			) : (
				<form
					onSubmit={async (event) => {
						event.preventDefault();
						if (submit.current || busy) return;
						const cleanName = name.trim();
						const cleanAliases = [
							...new Set(
								aliases
									.split("\n")
									.map((s) => s.trim())
									.filter(
										(s) => s && s.toLowerCase() !== cleanName.toLowerCase(),
									),
							),
						];
						if (!cleanName) {
							setValidation("请输入规范名称。");
							return;
						}
						if (
							Array.from(cleanName).length > 100 ||
							cleanAliases.length > 20 ||
							cleanAliases.some((s) => Array.from(s).length > 100)
						) {
							setValidation("名称和每个别名最多 100 字，别名最多 20 个。");
							return;
						}
						if (
							!entry &&
							entries.some(
								(other) =>
									other.name.trim().toLowerCase() === cleanName.toLowerCase(),
							)
						) {
							setValidation("词库中已有这个规范名称，请直接编辑已有词条。");
							return;
						}
						const terms = [cleanName, ...cleanAliases].map((s) =>
							s.toLowerCase(),
						);
						if (
							enabled &&
							entries.some(
								(other) =>
									other.enabled &&
									other.entity_id !== entry?.entity_id &&
									[other.name, ...other.aliases].some((s) =>
										terms.includes(s.trim().toLowerCase()),
									),
							)
						) {
							setValidation(
								"这个名称或别名已被其他启用的词条使用，请调整后保存。",
							);
							return;
						}
						submit.current = true;
						try {
							if (
								await onSave(
									{
										action: "upsert",
										id: entry?.entity_id ?? null,
										name: cleanName,
										kind,
										aliases: cleanAliases,
										enabled,
									},
									entry ? "词条已更新" : "词条已添加",
								)
							)
								onClose();
						} finally {
							submit.current = false;
						}
					}}
				>
					<Field label="规范名称" description="希望转录与摘要使用的正确写法。">
						{(attributes) => (
							<input
								{...attributes}
								value={name}
								disabled={busy}
								placeholder="例如：看山、MyAgents"
								onChange={(event) => {
									setName(event.target.value);
									clear();
								}}
							/>
						)}
					</Field>
					<Field label="类型">
						{(attributes) => (
							<select
								{...attributes}
								value={kind}
								disabled={busy}
								onChange={(event) => {
									setKind(event.target.value);
									clear();
								}}
							>
								{!kinds.some((item) => item.value === kind) && (
									<option value={kind}>{kind}</option>
								)}
								{kinds.map((item) => (
									<option value={item.value} key={item.value}>
										{item.label}
									</option>
								))}
							</select>
						)}
					</Field>
					<Field
						label="别名 / 常见误写"
						description="可选，每行一个，最多 20 个。例如「开山」。"
					>
						{(attributes) => (
							<textarea
								{...attributes}
								rows={3}
								value={aliases}
								disabled={busy}
								placeholder="输入别名或容易被听错的写法"
								onChange={(event) => {
									setAliases(event.target.value);
									clear();
								}}
							/>
						)}
					</Field>
					<SettingRow
						title="启用词条"
						description="停用后保留词条，暂不用于识别和生成。"
						action={
							<Switch
								label="启用词条"
								checked={enabled}
								disabled={busy}
								onChange={() => {
									setEnabled(!enabled);
									clear();
								}}
							/>
						}
					/>
					{(validation || error) && (
						<p className="form-error" role="alert">
							{validation || error}
						</p>
					)}
					<div className="modal-actions vocabulary-editor-actions">
						{entry && (
							<Button
								className="vocabulary-delete"
								tone="danger"
								disabled={busy}
								onClick={() => {
									clear();
									setDeleting(true);
								}}
							>
								删除词条
							</Button>
						)}
						<Button disabled={busy} onClick={onClose}>
							取消
						</Button>
						<Button
							type="submit"
							tone="primary"
							busy={busy}
							disabled={!name.trim()}
						>
							{busy ? "保存中…" : "保存词条"}
						</Button>
					</div>
				</form>
			)}
		</Modal>
	);
}
