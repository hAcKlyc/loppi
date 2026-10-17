import { useCallback, useEffect, useRef, useState } from "react";
import { core, type Person, userError, type VoiceSample } from "./api";
import { Icon, Modal } from "./Controls";
import { SamplePlayback, sampleParts } from "./sample-playback";

type Mutation = (
	request: { type: string; [key: string]: unknown },
	success: string,
) => Promise<boolean>;
export function PeopleView({
	people,
	busy,
	onMutate,
}: {
	people: Person[];
	busy: boolean;
	onMutate: Mutation;
}) {
	const [selected, setSelected] = useState<string | null>(null);
	const [creating, setCreating] = useState(false);
	const person = people.find((p) => p.id === selected);
	return (
		<section className="page-container people-page">
			{person ? (
				<PersonDetail
					key={person.id}
					person={person}
					busy={busy}
					onMutate={onMutate}
					onBack={() => setSelected(null)}
				/>
			) : (
				<>
					<div className="page-heading" data-tauri-drag-region>
						<h1>人物库</h1>
						<button
							type="button"
							className="quiet-button button-with-icon"
							disabled={busy}
							onClick={() => setCreating(true)}
						>
							<Icon name="plus" />
							新建人物
						</button>
					</div>
					<div className="panel-card people-list">
						{!people.length && (
							<div className="list-empty">
								还没有人物，可新建人物或在转录中为说话人命名。
							</div>
						)}
						{people.map((p) => (
							<button
								type="button"
								className="people-list-row"
								key={p.id}
								onClick={() => setSelected(p.id)}
							>
								<span className="person-avatar">
									{Array.from(p.display_name)[0]}
								</span>
								<span className="people-list-info">
									<strong>{p.display_name}</strong>
									<small>
										{p.sample_count
											? `${p.sample_count} 个声纹样例`
											: "待补充声纹"}
									</small>
								</span>
								<Icon name="next" />
							</button>
						))}
					</div>
				</>
			)}
			{creating && (
				<CreatePerson
					busy={busy}
					onClose={() => setCreating(false)}
					onMutate={onMutate}
				/>
			)}
		</section>
	);
}

function CreatePerson({
	busy,
	onClose,
	onMutate,
}: {
	busy: boolean;
	onClose: () => void;
	onMutate: Mutation;
}) {
	const [name, setName] = useState("");
	const [error, setError] = useState("");
	const submitting = useRef(false);
	return (
		<Modal title="新建人物" busy={busy} onClose={onClose}>
			<form
				onSubmit={async (event) => {
					event.preventDefault();
					if (submitting.current || !name.trim()) return;
					submitting.current = true;
					try {
						if (
							await onMutate(
								{ type: "create_person", display_name: name.trim() },
								"人物已添加",
							)
						)
							onClose();
						else setError("创建未完成，请重试");
					} finally {
						submitting.current = false;
					}
				}}
			>
				<label className="form-field">
					姓名
					<input
						value={name}
						maxLength={80}
						placeholder="输入人物姓名"
						disabled={busy}
						onChange={(e) => {
							setName(e.target.value);
							setError("");
						}}
					/>
				</label>
				{error && (
					<p className="form-error" role="alert">
						{error}
					</p>
				)}
				<div className="modal-actions">
					<button
						type="button"
						className="text-action"
						disabled={busy}
						onClick={onClose}
					>
						取消
					</button>
					<button
						type="submit"
						className="primary-button"
						disabled={busy || !name.trim()}
					>
						{busy ? "创建中…" : "创建人物"}
					</button>
				</div>
			</form>
		</Modal>
	);
}

function PersonDetail({
	person,
	busy,
	onMutate,
	onBack,
}: {
	person: Person;
	busy: boolean;
	onMutate: Mutation;
	onBack: () => void;
}) {
	const [editing, setEditing] = useState(false);
	const [name, setName] = useState(person.display_name);
	const [samples, setSamples] = useState<VoiceSample[] | null>(null);
	const [loadError, setLoadError] = useState("");
	const [error, setError] = useState("");
	const [confirm, setConfirm] = useState<"person" | VoiceSample | null>(null);
	const [playing, setPlaying] = useState<string | null>(null);
	const [elapsed, setElapsed] = useState(0);
	const request = useRef(0);
	const currentPerson = useRef(person);
	currentPerson.current = person;
	const submitting = useRef(false);
	const player = useRef<SamplePlayback | null>(null);
	const nameInput = useRef<HTMLInputElement>(null);
	useEffect(() => {
		const playback = new SamplePlayback(
			(id, at) => {
				setPlaying(id);
				setElapsed(at);
			},
			(cause) => setError(userError(cause)),
		);
		player.current = playback;
		return () => {
			playback.dispose();
			player.current = null;
		};
	}, []);
	const load = useCallback(async () => {
		const token = ++request.current;
		try {
			const result = await core<{
				type: "voice_samples";
				id: string;
				samples: VoiceSample[];
			}>({ type: "voice_samples", person_id: person.id });
			if (
				token !== request.current ||
				currentPerson.current.revision !== person.revision
			)
				return;
			setSamples(result.samples);
			setLoadError("");
		} catch (cause) {
			if (token === request.current) setLoadError(userError(cause));
		}
	}, [person.id, person.revision]);
	useEffect(() => {
		void load();
		return () => {
			request.current++;
		};
	}, [load]);
	useEffect(() => {
		if (
			playing &&
			samples &&
			!samples.some((s) => s.id === playing && s.audio_available !== false)
		)
			player.current?.stop();
	}, [playing, samples]);
	useEffect(() => {
		if (editing) {
			nameInput.current?.focus();
			nameInput.current?.select();
		}
	}, [editing]);
	const rename = async () => {
		if (submitting.current || !name.trim()) return;
		if (name.trim() === person.display_name) {
			setEditing(false);
			return;
		}
		submitting.current = true;
		setError("");
		try {
			if (
				await onMutate(
					{
						type: "rename_person",
						person_id: person.id,
						display_name: name.trim(),
						expected_revision: person.revision,
					},
					"姓名已更新",
				)
			)
				setEditing(false);
			else setError("姓名未保存，请重试");
		} finally {
			submitting.current = false;
		}
	};
	return (
		<>
			<div
				className="page-heading person-detail-heading"
				data-tauri-drag-region
			>
				<button
					type="button"
					className="text-action button-with-icon"
					disabled={busy}
					onClick={onBack}
				>
					<Icon name="back" />
					人物库
				</button>
			</div>
			<div className="person-profile">
				<span className="person-avatar profile-avatar">
					{Array.from(person.display_name)[0]}
				</span>
				<div className="person-profile-info">
					{editing ? (
						<form
							className="person-name-editor"
							onSubmit={(e) => {
								e.preventDefault();
								void rename();
							}}
						>
							<input
								ref={nameInput}
								aria-label="人物姓名"
								value={name}
								maxLength={80}
								disabled={busy}
								onChange={(e) => setName(e.target.value)}
								onKeyDown={(e) => {
									if (e.key === "Escape" && !busy) setEditing(false);
								}}
							/>
							<button
								type="submit"
								className="quiet-button"
								disabled={busy || !name.trim()}
							>
								保存
							</button>
							<button
								type="button"
								className="text-action"
								disabled={busy}
								onClick={() => setEditing(false)}
							>
								取消
							</button>
						</form>
					) : (
						<button
							type="button"
							className="person-name-button"
							aria-label={`修改${person.display_name}的姓名`}
							disabled={busy}
							onClick={() => {
								setName(person.display_name);
								setEditing(true);
								setError("");
							}}
						>
							<h1>{person.display_name}</h1>
							<Icon name="edit" />
						</button>
					)}
					<p>
						{person.sample_count
							? `${person.sample_count} 个声纹样例`
							: "待补充声纹"}
					</p>
				</div>
				<button
					type="button"
					className="text-action destructive button-with-icon"
					disabled={busy}
					onClick={() => {
						setConfirm("person");
						setError("");
					}}
				>
					<Icon name="delete" />
					删除人物
				</button>
			</div>
			{error && !confirm && (
				<p role="alert" className="form-error">
					{error}
				</p>
			)}
			<div className="sample-heading">
				<h2>声纹样例</h2>
				<span>在转录中确认人物后，自动筛选并补充</span>
			</div>
			<div className="panel-card voice-sample-list">
				{loadError && (
					<div className="list-empty" role="alert">
						{loadError}
						<button
							type="button"
							className="text-action"
							onClick={() => void load()}
						>
							重试
						</button>
					</div>
				)}
				{!samples && !loadError && (
					<div className="list-empty" role="status">
						正在加载声纹…
					</div>
				)}
				{samples?.length === 0 && !loadError && (
					<div className="list-empty">
						暂无声纹样例。在转录中选择此人物，可从合格的声音建立声纹。
					</div>
				)}
				{samples?.map((sample, index) => {
					const duration = sampleParts(sample).reduce(
						(sum, part) => sum + part.ended_ms - part.started_ms,
						0,
					);
					const active = sample.id === playing;
					return (
						<div
							className={`voice-sample-row${active ? " playing" : ""}`}
							key={sample.id}
						>
							<button
								type="button"
								className="sample-play-button"
								aria-label={
									active ? `停止播放样例 ${index + 1}` : `播放样例 ${index + 1}`
								}
								disabled={sample.audio_available === false}
								onClick={() => {
									setError("");
									if (active) player.current?.stop();
									else player.current?.start(sample);
								}}
							>
								<Icon name={active ? "stop" : "play"} />
							</button>
							<div className="sample-description">
								<strong>
									{new Date(sample.started_ms).toLocaleString("zh-CN", {
										year: "numeric",
										month: "2-digit",
										day: "2-digit",
										hour: "2-digit",
										minute: "2-digit",
									})}
								</strong>
								<span>
									{sample.source === "system" ? "电脑声音" : "麦克风"} ·{" "}
									{(duration / 1000).toFixed(1)} 秒
									{sampleParts(sample).length > 1
										? ` · ${sampleParts(sample).length} 个片段`
										: ""}
									{sample.audio_available === false ? " · 原音不可用" : ""}
								</span>
							</div>
							{active && (
								<span className="sample-progress" aria-live="off">
									{(elapsed / 1000).toFixed(1)} / {(duration / 1000).toFixed(1)}{" "}
									秒
								</span>
							)}
							<button
								type="button"
								className="icon-button destructive"
								aria-label={`删除样例 ${index + 1}`}
								disabled={busy}
								onClick={() => {
									player.current?.stop();
									setConfirm(sample);
									setError("");
								}}
							>
								<Icon name="delete" />
							</button>
						</div>
					);
				})}
			</div>
			{confirm && (
				<Modal
					title={confirm === "person" ? "删除人物？" : "删除声纹样例？"}
					busy={busy}
					onClose={() => setConfirm(null)}
				>
					<p>
						{confirm === "person"
							? `将删除「${person.display_name}」及其声纹样例，历史转录与录音仍保留。`
							: "这份样例将不再用于识别人物，原始录音与转录仍保留。"}
					</p>
					{error && (
						<p role="alert" className="form-error">
							{error}
						</p>
					)}
					<div className="modal-actions">
						<button
							type="button"
							className="quiet-button"
							disabled={busy}
							onClick={() => setConfirm(null)}
						>
							取消
						</button>
						<button
							type="button"
							className="danger-button"
							disabled={busy}
							onClick={async () => {
								if (submitting.current) return;
								submitting.current = true;
								player.current?.stop();
								try {
									const saved = await onMutate(
										confirm === "person"
											? {
													type: "delete_person",
													person_id: person.id,
													expected_revision: person.revision,
												}
											: {
													type: "remove_voice_sample",
													sample_id: confirm.id,
													expected_revision: confirm.revision,
												},
										confirm === "person" ? "人物已删除" : "声纹样例已删除",
									);
									if (saved) {
										setConfirm(null);
										if (confirm === "person") onBack();
										else await load();
									} else {
										setError("删除未完成，请取消后重试");
										await load();
									}
								} finally {
									submitting.current = false;
								}
							}}
						>
							{busy ? "删除中…" : "确认删除"}
						</button>
					</div>
				</Modal>
			)}
		</>
	);
}
