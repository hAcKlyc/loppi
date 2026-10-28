import { useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { LocalSpeaker, Person } from "./api";
import { Button, Icon, IconButton } from "./Controls";
import { LoadBoundary } from "./LoadingState";

export type SpeakerChoice =
	| { kind: "existing"; personId: string }
	| { kind: "create"; name: string }
	| { kind: "local"; groupId: string };
export type SpeakerAction =
	| { kind: "bind"; choice: SpeakerChoice; entire: boolean }
	| { kind: "unbind"; entire: boolean }
	| { kind: "sample" };

type Props = {
	anchor: HTMLElement;
	people: Person[];
	peopleReady?: boolean;
	peopleError?: string;
	onPeopleRetry?: () => void;
	localSpeakers: LocalSpeaker[];
	currentGroup: string | null;
	personId: string | null;
	confirmed: boolean;
	onClose: () => void;
	onApply: (action: SpeakerAction) => Promise<void>;
};

// Mount once per opened sentence: drafts never survive cancel, target switches or reopening.
export function SpeakerPicker({
	anchor,
	people,
	peopleReady = true,
	peopleError,
	onPeopleRetry,
	localSpeakers,
	currentGroup,
	personId,
	confirmed,
	onClose,
	onApply,
}: Props) {
	const id = useId();
	const panel = useRef<HTMLDivElement>(null);
	const search = useRef<HTMLInputElement>(null);
	const submitting = useRef(false);
	const restoreFocus = useRef(true);
	const [query, setQuery] = useState("");
	const [choice, setChoice] = useState<SpeakerChoice | null>(
		personId ? { kind: "existing", personId } : null,
	);
	const [entire, setEntire] = useState(true);
	const [pending, setPending] = useState(false);
	const [error, setError] = useState("");
	const closeRef = useRef(onClose);
	closeRef.current = onClose;
	const trimmed = query.trim();
	const normalized = trimmed.toLocaleLowerCase();
	const matches = people.filter((person) =>
		person.display_name.toLocaleLowerCase().includes(normalized),
	);
	const canCreate =
		peopleReady &&
		trimmed.length > 0 &&
		!people.some(
			(person) => person.display_name.trim().toLocaleLowerCase() === normalized,
		);
	const validChoice =
		choice?.kind === "create"
			? canCreate && choice.name === trimmed
			: choice?.kind === "local"
				? localSpeakers.some(
						(s) =>
							s.group_id === choice.groupId &&
							s.anonymous &&
							s.group_id !== currentGroup,
					)
				: choice?.kind === "existing" &&
					people.some((person) => person.id === choice.personId);

	useLayoutEffect(() => {
		const element = panel.current;
		if (!element) return;
		const position = () => {
			const rect = anchor.getBoundingClientRect();
			const margin = 12;
			const gap = 8;
			const height = window.innerHeight;
			const width = window.innerWidth;
			element.style.maxHeight = `${height - margin * 2}px`;
			const box = element.getBoundingClientRect();
			const right = rect.right + gap;
			const left =
				right + box.width <= width - margin
					? right
					: rect.left - gap - box.width >= margin
						? rect.left - gap - box.width
						: Math.max(margin, width - box.width - margin);
			element.style.left = `${left}px`;
			const searchBox = search.current?.getBoundingClientRect();
			const searchOffset = searchBox
				? searchBox.top - box.top + searchBox.height / 2
				: 0;
			const alignedTop = rect.top + rect.height / 2 - searchOffset;
			element.style.top = `${Math.max(margin, Math.min(alignedTop, height - box.height - margin))}px`;
			element.dataset.side = left >= rect.right ? "right" : "left";
		};
		const dismiss = (event: PointerEvent) => {
			if (
				!submitting.current &&
				event.target instanceof Node &&
				!element.contains(event.target) &&
				!anchor.contains(event.target)
			) {
				restoreFocus.current = false;
				closeRef.current();
			}
		};
		const scroll = (event: Event) => {
			if (
				!submitting.current &&
				event.target instanceof Node &&
				!element.contains(event.target)
			)
				closeRef.current();
		};
		position();
		const initial = search.current?.disabled
			? element.querySelector<HTMLButtonElement>("button:not(:disabled)")
			: search.current;
		initial?.focus({ preventScroll: true });
		const observer = new ResizeObserver(position);
		observer.observe(element);
		window.addEventListener("resize", position);
		document.addEventListener("pointerdown", dismiss, true);
		document.addEventListener("scroll", scroll, true);
		return () => {
			observer.disconnect();
			window.removeEventListener("resize", position);
			document.removeEventListener("pointerdown", dismiss, true);
			document.removeEventListener("scroll", scroll, true);
			if (
				restoreFocus.current &&
				anchor.isConnected &&
				(element.contains(document.activeElement) ||
					document.activeElement === document.body)
			) {
				anchor.focus({ preventScroll: true });
			}
		};
	}, [anchor]);

	const apply = async (action: SpeakerAction) => {
		if (submitting.current) return;
		submitting.current = true;
		setPending(true);
		setError("");
		try {
			await onApply(action);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			submitting.current = false;
			setPending(false);
		}
	};

	return createPortal(
		<div
			ref={panel}
			className="speaker-picker"
			role="dialog"
			aria-labelledby={`${id}-title`}
			aria-busy={pending}
			onMouseDown={(event) => {
				if (event.button !== 0 || !(event.target instanceof Element)) return;
				const target = event.target.closest("button, label");
				const control =
					target instanceof HTMLLabelElement ? target.control : target;
				if (
					(control instanceof HTMLButtonElement ||
						control instanceof HTMLInputElement) &&
					!control.disabled
				) {
					// WebKit otherwise blurs the search to the outer dialog before click,
					// dismissing this picker before a choice, scope or submit can run.
					event.preventDefault();
					control.focus({ preventScroll: true });
				}
			}}
			onBlur={(event) => {
				if (
					!submitting.current &&
					event.relatedTarget instanceof Node &&
					!event.currentTarget.contains(event.relatedTarget) &&
					!anchor.contains(event.relatedTarget)
				) {
					restoreFocus.current = false;
					onClose();
				}
			}}
			onKeyDown={(event) => {
				if (event.key === "Escape") {
					event.preventDefault();
					event.stopPropagation();
					if (!submitting.current) onClose();
				}
			}}
		>
			<header className="speaker-picker-heading">
				<div>
					<h3 id={`${id}-title`}>指定人物</h3>
				</div>
				<IconButton
					type="button"
					label="关闭人物选择"
					disabled={pending}
					onClick={onClose}
				>
					<Icon name="close" />
				</IconButton>
			</header>
			<div className="speaker-picker-body">
				<input
					ref={search}
					id={`${id}-search`}
					aria-label="搜索人物或输入新名字"
					className="speaker-picker-input"
					type="search"
					placeholder="搜索人物或输入新名字…"
					autoComplete="off"
					disabled={pending || !peopleReady}
					value={query}
					onChange={(event) => {
						setQuery(event.target.value);
						setChoice((current) =>
							current?.kind === "create"
								? { kind: "create", name: event.target.value.trim() }
								: null,
						);
						setError("");
					}}
				/>
				<div className="speaker-picker-options">
					<LoadBoundary
						ready={peopleReady}
						error={peopleError}
						retry={onPeopleRetry}
						label="人物"
						rows={2}
					>
						{matches.map((person) => {
							const selected =
								choice?.kind === "existing" && choice.personId === person.id;
							return (
								<button
									key={person.id}
									type="button"
									className="speaker-picker-option"
									aria-pressed={selected}
									disabled={pending || !peopleReady}
									onClick={() => {
										setChoice({ kind: "existing", personId: person.id });
										setError("");
									}}
								>
									<span className="speaker-picker-avatar" aria-hidden="true">
										{Array.from(person.display_name)[0]}
									</span>
									<span
										className="speaker-picker-option-name"
										title={person.display_name}
									>
										{person.display_name}
									</span>
									{person.sample_count === 0 && (
										<small className="speaker-picker-sample-status">
											待补充声纹
										</small>
									)}
									<span className="speaker-picker-check" aria-hidden="true">
										{selected ? "✓" : ""}
									</span>
								</button>
							);
						})}
						{(!trimmed || canCreate) && (
							<button
								type="button"
								className="speaker-picker-option speaker-picker-create"
								aria-pressed={choice?.kind === "create"}
								disabled={pending || !peopleReady}
								onClick={() => {
									setChoice({ kind: "create", name: trimmed });
									if (!trimmed) search.current?.focus();
									setError("");
								}}
							>
								<span aria-hidden="true">＋</span>
								<span className="speaker-picker-option-name">
									{trimmed
										? `新建「${trimmed}」并建立声纹`
										: "新建人物并建立声纹"}
								</span>
								<span className="speaker-picker-check" aria-hidden="true">
									{choice?.kind === "create" ? "✓" : ""}
								</span>
							</button>
						)}
						{localSpeakers
							.filter((s) => s.anonymous && s.name.includes(trimmed))
							.map((speaker) => (
								<button
									key={speaker.group_id}
									type="button"
									className="speaker-picker-option speaker-picker-local"
									aria-pressed={
										choice?.kind === "local" &&
										choice.groupId === speaker.group_id
									}
									disabled={pending || speaker.group_id === currentGroup}
									onClick={() => {
										setChoice({ kind: "local", groupId: speaker.group_id });
										setError("");
									}}
								>
									<span className="speaker-picker-avatar" aria-hidden="true">
										{speaker.name.replace("说话人 ", "")}
									</span>
									<span className="speaker-picker-option-name">
										{speaker.name}
									</span>
									<span className="speaker-picker-check" aria-hidden="true">
										{speaker.group_id === currentGroup
											? "·"
											: choice?.kind === "local" &&
													choice.groupId === speaker.group_id
												? "✓"
												: ""}
									</span>
								</button>
							))}
						{people.length === 0 && !trimmed && (
							<p className="speaker-picker-empty">
								还没有人物，输入姓名即可创建。
							</p>
						)}
					</LoadBoundary>
				</div>
				<fieldset
					className="speaker-picker-scope"
					disabled={pending || !peopleReady}
				>
					<legend>应用范围</legend>
					<label>
						<input
							type="radio"
							name={`${id}-scope`}
							checked={!entire}
							onChange={() => setEntire(false)}
						/>
						<span>仅这句话</span>
					</label>
					<label>
						<input
							type="radio"
							name={`${id}-scope`}
							checked={entire}
							onChange={() => setEntire(true)}
						/>
						<span>本场中该说话人的全部发言</span>
					</label>
				</fieldset>
				{personId && (
					<div className="speaker-picker-secondary">
						<button
							type="button"
							disabled={pending || !peopleReady}
							onClick={() => void apply({ kind: "unbind", entire })}
						>
							取消人物归属
						</button>
						<button
							type="button"
							disabled={pending || !confirmed}
							title={
								confirmed
									? "将这句话的声音加入人物样例"
									: "请先确认人物归属，再添加声纹样例"
							}
							onClick={() => void apply({ kind: "sample" })}
						>
							加入声纹样例
						</button>
					</div>
				)}
				{error && (
					<p className="speaker-picker-error" role="alert">
						{error}
					</p>
				)}
			</div>
			<footer className="speaker-picker-footer">
				<button
					type="button"
					className="speaker-picker-cancel"
					disabled={pending}
					onClick={onClose}
				>
					取消
				</button>
				<Button
					type="button"
					tone="primary"
					disabled={pending || !peopleReady || !validChoice}
					onClick={() => {
						if (choice && validChoice)
							void apply({ kind: "bind", choice, entire });
					}}
				>
					{pending ? "正在保存…" : "确认"}
				</Button>
			</footer>
		</div>,
		anchor.closest("dialog") ?? document.body,
	);
}
