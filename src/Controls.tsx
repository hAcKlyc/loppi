import {
	type ButtonHTMLAttributes,
	type HTMLAttributes,
	type ReactNode,
	type Ref,
	type RefObject,
	useEffect,
	useId,
	useLayoutEffect,
	useRef,
	useState,
} from "react";

export function Icon({
	name,
}: {
	name:
		| "back"
		| "next"
		| "play"
		| "stop"
		| "edit"
		| "delete"
		| "close"
		| "plus"
		| "more"
		| "pause";
}) {
	const paths = {
		back: "M15 5l-7 7 7 7",
		next: "M9 5l7 7-7 7",
		play: "M8 5l11 7-11 7V5Z",
		stop: "M7 7h10v10H7Z",
		edit: "m15 4 5 5M4 20l5-1L20 8a2 2 0 0 0-5-5L4 14v6Z",
		delete: "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 10v7M14 10v7",
		close: "m6 6 12 12M6 18 18 6",
		plus: "M12 5v14M5 12h14",
		more: "M5 12h.01M12 12h.01M19 12h.01",
		pause: "M9 5v14M15 5v14",
	};
	return (
		<svg
			aria-hidden="true"
			className="control-icon"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="var(--stroke-icon)"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d={paths[name]} />
		</svg>
	);
}

export function Switch({
	label,
	checked,
	disabled,
	onChange,
}: {
	label: string;
	checked: boolean;
	disabled?: boolean;
	onChange: () => void;
}) {
	return (
		<button
			type="button"
			role="switch"
			aria-label={label}
			aria-checked={checked}
			disabled={disabled}
			className={`switch${checked ? " on" : ""}`}
			onClick={onChange}
		/>
	);
}

export function SegmentedOptions<T extends string>({
	label,
	value,
	options,
	disabled,
	onChange,
}: {
	label: string;
	value: T;
	options: { value: T; label: string }[];
	disabled?: boolean;
	onChange: (value: T) => void;
}) {
	const id = useId();
	return (
		<fieldset
			className="segmented-options"
			aria-label={label}
			disabled={disabled}
		>
			{options.map((option) => (
				<label key={option.value}>
					<input
						type="radio"
						name={id}
						value={option.value}
						checked={value === option.value}
						onChange={() => onChange(option.value)}
					/>
					<span>{option.label}</span>
				</label>
			))}
		</fieldset>
	);
}

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
	tone?: "primary" | "secondary" | "quiet" | "danger";
	size?: "compact" | "default" | "comfortable";
	busy?: boolean;
	busyLabel?: string;
	ref?: Ref<HTMLButtonElement>;
};

export function Button({
	tone = "quiet",
	size = "default",
	busy,
	busyLabel,
	disabled,
	className = "",
	children,
	type = "button",
	onClick,
	...props
}: ButtonProps) {
	return (
		<button
			{...props}
			type={type}
			disabled={disabled || busy}
			aria-busy={busy || undefined}
			onClick={(event) => {
				// WebKit pointer activation does not focus buttons by default. Record the
				// same return point as keyboard activation before a caller opens a modal.
				event.currentTarget.focus({ preventScroll: true });
				onClick?.(event);
			}}
			className={`${tone}-button control-button control-${size} ${className}`}
		>
			{busyLabel ? (
				<span className="button-labels">
					<span aria-hidden={busy || undefined}>{children}</span>
					<span aria-hidden={!busy || undefined}>{busyLabel}</span>
				</span>
			) : (
				children
			)}
		</button>
	);
}

export function IconButton({
	label,
	className = "",
	...props
}: Omit<ButtonProps, "aria-label"> & { label: string }) {
	return (
		<Button
			{...props}
			aria-label={label}
			data-tooltip={label}
			className={`icon-button ${className}`}
		/>
	);
}

type FieldAttributes = {
	id: string;
	"aria-describedby"?: string;
	"aria-invalid"?: true;
};
export function Field({
	label,
	description,
	error,
	children,
	className = "",
}: {
	label: string;
	description?: ReactNode;
	error?: string;
	className?: string;
	children: (attributes: FieldAttributes) => ReactNode;
}) {
	const id = useId();
	const descriptions = [description && `${id}-hint`, error && `${id}-error`]
		.filter(Boolean)
		.join(" ");
	return (
		<div className={`form-field ${className}`}>
			<label htmlFor={id}>{label}</label>
			{children({
				id,
				"aria-describedby": descriptions || undefined,
				"aria-invalid": error ? true : undefined,
			})}
			{description && (
				<p className="field-description" id={`${id}-hint`}>
					{description}
				</p>
			)}
			{error && (
				<p className="form-error" id={`${id}-error`} role="alert">
					{error}
				</p>
			)}
		</div>
	);
}

export function SettingRow({
	title,
	status,
	description,
	action,
}: {
	title: ReactNode;
	status?: ReactNode;
	description?: ReactNode;
	action: ReactNode;
}) {
	return (
		<div className="setting-row">
			<div>
				<div className="setting-heading">
					<strong>{title}</strong>
					{status}
				</div>
				{description && <p>{description}</p>}
			</div>
			{action}
		</div>
	);
}

export function Tabs<T extends string>({
	id,
	label,
	value,
	items,
	onChange,
	disabled,
	className = "",
}: {
	id: string;
	label: string;
	value: T;
	items: readonly { value: T; label: string }[];
	onChange: (value: T) => void;
	disabled?: boolean;
	className?: string;
}) {
	const list = useRef<HTMLDivElement>(null);
	const [focused, setFocused] = useState(value);
	useEffect(() => setFocused(value), [value]);
	return (
		<div
			ref={list}
			className={`detail-tabs ${className}`}
			role="tablist"
			aria-label={label}
			onBlur={(event) => {
				if (!event.currentTarget.contains(event.relatedTarget))
					setFocused(value);
			}}
		>
			{items.map((item, index) => (
				<button
					type="button"
					key={item.value}
					id={`${id}-tab-${item.value}`}
					role="tab"
					aria-selected={value === item.value}
					aria-controls={`${id}-panel-${item.value}`}
					tabIndex={focused === item.value ? 0 : -1}
					onFocus={() => setFocused(item.value)}
					disabled={disabled}
					className={value === item.value ? "active" : ""}
					onClick={() => onChange(item.value)}
					onKeyDown={(event) => {
						let next: number;
						if (event.key === "ArrowRight") next = (index + 1) % items.length;
						else if (event.key === "ArrowLeft")
							next = (index + items.length - 1) % items.length;
						else if (event.key === "Home") next = 0;
						else if (event.key === "End") next = items.length - 1;
						else return;
						event.preventDefault();
						list.current
							?.querySelectorAll<HTMLButtonElement>("[role=tab]")
							[next]?.focus();
					}}
				>
					{item.label}
				</button>
			))}
		</div>
	);
}

export function TabPanel({
	tabsId,
	value,
	selected,
	children,
	style,
	...props
}: HTMLAttributes<HTMLDivElement> & {
	tabsId: string;
	value: string;
	selected: string;
	ref?: Ref<HTMLDivElement>;
}) {
	return (
		<div
			{...props}
			id={`${tabsId}-panel-${value}`}
			role="tabpanel"
			aria-labelledby={`${tabsId}-tab-${value}`}
			// biome-ignore lint/a11y/noNoninteractiveTabindex: APG tab panels are keyboard destinations, including empty/loading panels.
			tabIndex={0}
			hidden={value !== selected}
			style={{
				...style,
				display: value === selected ? style?.display : "none",
			}}
		>
			{children}
		</div>
	);
}

export type CloseReason = "close-button" | "escape" | "backdrop";

type DialogProps = {
	children: ReactNode;
	className?: string;
	label?: string;
	labelledBy?: string;
	describedBy?: string;
	closeDisabled?: boolean;
	initialFocusRef?: RefObject<HTMLElement | null>;
	returnFocusRef?: RefObject<HTMLElement | null>;
	onClose: (reason: CloseReason) => void;
	surfaceDismiss?: boolean;
};

/** Native top layer owns modality; business state and close policy stay with the caller. */
export function Dialog({
	children,
	className = "",
	label,
	labelledBy,
	describedBy,
	closeDisabled,
	initialFocusRef,
	returnFocusRef,
	onClose,
	surfaceDismiss = false,
}: DialogProps) {
	const ref = useRef<HTMLDialogElement>(null);
	useLayoutEffect(() => {
		const dialog = ref.current;
		if (!dialog) return;
		const opener = returnFocusRef?.current ?? document.activeElement;
		const openerFocusVisible =
			opener instanceof HTMLElement && opener.matches(":focus-visible");
		dialog.showModal();
		const initial =
			initialFocusRef?.current ??
			dialog.querySelector<HTMLElement>(
				"input:not(:disabled):not([type=hidden]), textarea:not(:disabled), select:not(:disabled)",
			) ??
			dialog.querySelector<HTMLElement>("[data-dialog-title]");
		initial?.focus({ preventScroll: true });
		return () => {
			const ownedFocus = dialog.contains(document.activeElement);
			dialog.close();
			queueMicrotask(() => {
				const active = document.activeElement;
				// Do not pull focus out of a newly opened modal or an intentional new target.
				if (
					!ownedFocus &&
					active instanceof HTMLElement &&
					active !== document.body &&
					active !== opener &&
					active.isConnected
				)
					return;
				const modals =
					document.querySelectorAll<HTMLDialogElement>("dialog[open]");
				const modal = modals[modals.length - 1];
				const target =
					opener instanceof HTMLElement &&
					opener.isConnected &&
					!opener.matches(":disabled") &&
					(!modal || modal.contains(opener))
						? opener
						: (modal?.querySelector<HTMLElement>(
								"button:not(:disabled), [tabindex='0']",
							) ??
							document.querySelector<HTMLElement>("[data-focus-fallback]"));
				// Escape must not turn a pointer-opened card into a visibly selected
				// card. Keep its return focus; resume the normal ring on keyboard use.
				if (
					target instanceof HTMLButtonElement &&
					target === opener &&
					!openerFocusVisible
				) {
					const clearPointerFocus = () => {
						target.removeAttribute("data-pointer-focus-return");
						target.removeEventListener("blur", clearPointerFocus);
						target.removeEventListener("keydown", clearPointerFocus);
					};
					target.setAttribute("data-pointer-focus-return", "");
					target.addEventListener("blur", clearPointerFocus, { once: true });
					target.addEventListener("keydown", clearPointerFocus, { once: true });
				}
				target?.focus({ preventScroll: true });
			});
		};
	}, [initialFocusRef, returnFocusRef]);
	return (
		<dialog
			ref={ref}
			className={`dialog-surface ${className}`}
			aria-label={label}
			aria-labelledby={labelledBy}
			aria-describedby={describedBy}
			onKeyDown={(event) => {
				// WebKit may skip buttons according to system keyboard preferences. Keep
				// this modal's full Tab sequence deterministic without a global listener.
				if (
					event.key === "Tab" &&
					!event.defaultPrevented &&
					(event.target as HTMLElement).closest("dialog") ===
						event.currentTarget
				) {
					const controls = [
						...event.currentTarget.querySelectorAll<HTMLElement>(
							"button, input, textarea, select, summary, a[href], [tabindex], [contenteditable=true]",
						),
					].filter(
						(element) =>
							element.tabIndex >= 0 &&
							!element.matches(":disabled") &&
							element.getClientRects().length > 0 &&
							element.closest("dialog") === event.currentTarget,
					);
					const stops = controls.filter((element) => {
						if (
							!(element instanceof HTMLInputElement) ||
							element.type !== "radio" ||
							!element.name
						)
							return true;
						const group = controls.filter(
							(candidate) =>
								candidate instanceof HTMLInputElement &&
								candidate.type === "radio" &&
								candidate.name === element.name &&
								candidate.form === element.form,
						) as HTMLInputElement[];
						return (
							element === (group.find((radio) => radio.checked) ?? group[0])
						);
					});
					event.preventDefault();
					const current = stops.indexOf(document.activeElement as HTMLElement);
					const next = event.shiftKey
						? current <= 0
							? stops.length - 1
							: current - 1
						: (current + 1) % stops.length;
					(stops[next] ?? event.currentTarget).focus();
					return;
				}
				if (
					event.key !== "Escape" ||
					event.defaultPrevented ||
					(event.target as HTMLElement).closest("dialog") !==
						event.currentTarget
				)
					return;
				event.preventDefault();
				event.stopPropagation();
				if (!closeDisabled) onClose("escape");
			}}
			onCancel={(event) => {
				event.preventDefault();
				event.stopPropagation();
				if (event.target === event.currentTarget && !closeDisabled)
					onClose("escape");
			}}
			onClick={(event) => {
				if (event.target !== event.currentTarget || closeDisabled) return;
				const r = event.currentTarget.getBoundingClientRect();
				if (
					surfaceDismiss ||
					event.clientX < r.left ||
					event.clientX > r.right ||
					event.clientY < r.top ||
					event.clientY > r.bottom
				)
					onClose("backdrop");
			}}
		>
			{children}
		</dialog>
	);
}

export function Modal({
	title,
	description,
	busy,
	onClose,
	children,
	initialFocusRef,
	feedback,
}: {
	title: string;
	description?: string;
	busy?: boolean;
	onClose: (reason: CloseReason) => void;
	children: ReactNode;
	initialFocusRef?: RefObject<HTMLElement | null>;
	feedback?: string;
}) {
	const id = useId();
	return (
		<Dialog
			className="modal app-dialog"
			labelledBy={id}
			describedBy={description ? `${id}-description` : undefined}
			closeDisabled={busy}
			initialFocusRef={initialFocusRef}
			onClose={onClose}
		>
			<div className="modal-title">
				<h2 id={id} tabIndex={-1} data-dialog-title>
					{title}
				</h2>
				<IconButton
					label="关闭弹窗"
					disabled={busy}
					onClick={() => onClose("close-button")}
				>
					<Icon name="close" />
				</IconButton>
			</div>
			{description && <p id={`${id}-description`}>{description}</p>}
			{children}
			{(busy || feedback) && (
				<p className="dialog-feedback" role="status">
					{busy ? "正在处理，请稍候…" : feedback}
				</p>
			)}
		</Dialog>
	);
}
