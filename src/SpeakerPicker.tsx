import { useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Person } from "./api";

export type SpeakerChoice =
	| { kind: "existing"; personId: string }
	| { kind: "create"; name: string };
export type SpeakerAction =
	| { kind: "bind"; choice: SpeakerChoice; entire: boolean }
	| { kind: "unbind"; entire: boolean }
	| { kind: "sample" };

type Props = {
	anchor: HTMLElement;
	people: Person[];
	name: string;
	personId: string | null;
	confirmed: boolean;
	onClose: () => void;
	onApply: (action: SpeakerAction) => Promise<void>;
};

// Mount once per opened sentence: drafts never survive cancel, target switches or reopening.
export function SpeakerPicker({
	anchor,
	people,
	name,
	personId,
	confirmed,
	onClose,
	onApply,
}: Props) {
	const id = useId();
	const panel = useRef<HTMLDivElement>(null);
	const search = useRef<HTMLInputElement>(null);
	const submitting = useRef(false);
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
		trimmed.length > 0 &&
		!people.some(
			(person) => person.display_name.trim().toLocaleLowerCase() === normalized,
		);
	const validChoice =
		choice?.kind === "create"
			? canCreate && choice.name === trimmed
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
			const below = Math.max(0, height - rect.bottom - gap - margin);
			const above = Math.max(0, rect.top - gap - margin);
			const naturalHeight = element.scrollHeight;
			const upwards = below < naturalHeight && above > below;
			element.style.maxHeight = `${Math.max(0, upwards ? above : below)}px`;
			const box = element.getBoundingClientRect();
			element.style.left = `${Math.max(margin, Math.min(rect.left, width - box.width - margin))}px`;
			element.style.top = `${upwards ? Math.max(margin, rect.top - gap - box.height) : rect.bottom + gap}px`;
			element.dataset.side = upwards ? "above" : "below";
		};
		const dismiss = (event: PointerEvent) => {
			if (
				!submitting.current &&
				event.target instanceof Node &&
				!element.contains(event.target) &&
				!anchor.contains(event.target)
			)
				closeRef.current();
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
		search.current?.focus({ preventScroll: true });
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
			aria-describedby={`${id}-description`}
			aria-busy={pending}
			onKeyDown={(event) => {
				if (event.key === "Escape") {
					event.stopPropagation();
					if (!submitting.current) onClose();
				}
				if (event.key === "Tab") {
					const controls = panel.current?.querySelectorAll<HTMLElement>(
						"button:not(:disabled), input:not(:disabled)",
					);
					if (!controls?.length) {
						event.preventDefault();
						return;
					}
					const first = controls[0],
						last = controls[controls.length - 1];
					if (event.shiftKey && document.activeElement === first) {
						event.preventDefault();
						last.focus();
					} else if (!event.shiftKey && document.activeElement === last) {
						event.preventDefault();
						first.focus();
					}
				}
			}}
		>
			<header className="speaker-picker-heading">
				<div>
					<h3 id={`${id}-title`}>指定人物</h3>
					<p id={`${id}-description`}>为「{name}」设置人物身份</p>
				</div>
				<button
					type="button"
					className="icon-button"
					aria-label="关闭人物选择"
					disabled={pending}
					onClick={onClose}
				>
					×
				</button>
			</header>
			<div className="speaker-picker-body">
				<label className="speaker-picker-search" htmlFor={`${id}-search`}>
					人物姓名
				</label>
				<input
					ref={search}
					id={`${id}-search`}
					className="speaker-picker-input"
					type="search"
					placeholder="搜索人物或输入新名字…"
					autoComplete="off"
					disabled={pending}
					value={query}
					onChange={(event) => {
						setQuery(event.target.value);
						setChoice(null);
						setError("");
					}}
				/>
				<div className="speaker-picker-options">
					{matches.map((person) => {
						const selected =
							choice?.kind === "existing" && choice.personId === person.id;
						return (
							<button
								key={person.id}
								type="button"
								className="speaker-picker-option"
								aria-pressed={selected}
								disabled={pending}
								onClick={() => {
									setChoice({ kind: "existing", personId: person.id });
									setError("");
								}}
							>
								<span className="speaker-picker-avatar" aria-hidden="true">
									{Array.from(person.display_name)[0]}
								</span>
								<span className="speaker-picker-option-name">
									{person.display_name}
								</span>
								<span className="speaker-picker-check" aria-hidden="true">
									{selected ? "✓" : ""}
								</span>
							</button>
						);
					})}
					{canCreate && (
						<button
							type="button"
							className="speaker-picker-option speaker-picker-create"
							aria-pressed={choice?.kind === "create"}
							disabled={pending}
							onClick={() => {
								setChoice({ kind: "create", name: trimmed });
								setError("");
							}}
						>
							<span aria-hidden="true">＋</span>
							<span className="speaker-picker-option-name">
								创建人物「{trimmed}」并指定
							</span>
							<span className="speaker-picker-check" aria-hidden="true">
								{choice?.kind === "create" ? "✓" : ""}
							</span>
						</button>
					)}
					{people.length === 0 && !trimmed && (
						<p className="speaker-picker-empty">
							还没有人物，输入姓名即可创建。
						</p>
					)}
				</div>
				<fieldset className="speaker-picker-scope" disabled={pending}>
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
					<p>按当前说话人分组应用，不影响其他场次。</p>
				</fieldset>
				{personId && (
					<div className="speaker-picker-secondary">
						<button
							type="button"
							disabled={pending}
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
				<button
					type="button"
					className="primary-button"
					disabled={pending || !validChoice}
					onClick={() => {
						if (choice && validChoice)
							void apply({ kind: "bind", choice, entire });
					}}
				>
					{pending ? "正在保存…" : "确认"}
				</button>
			</footer>
		</div>,
		document.body,
	);
}
