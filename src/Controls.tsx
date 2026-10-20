import { type ReactNode, useId, useLayoutEffect, useRef } from "react";

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
		| "plus";
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
	};
	return (
		<svg
			aria-hidden="true"
			className="control-icon"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.6"
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

/** Native modal owns focus trapping, Escape and focus restoration. */
export function Modal({
	title,
	busy,
	onClose,
	children,
}: {
	title: string;
	busy?: boolean;
	onClose: () => void;
	children: ReactNode;
}) {
	const ref = useRef<HTMLDialogElement>(null);
	const id = useId();
	useLayoutEffect(() => {
		const dialog = ref.current;
		const opener = document.activeElement;
		dialog?.showModal();
		dialog?.querySelector<HTMLInputElement>("input")?.focus();
		return () => {
			// Close before React detaches the dialog so native focus restoration
			// still sees its opener; passive effect cleanup runs too late.
			dialog?.close();
			// Successful saves also re-enable the opener in this commit.
			queueMicrotask(() => {
				if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
			});
		};
	}, []);
	return (
		<dialog
			ref={ref}
			className="modal app-dialog"
			aria-labelledby={id}
			onKeyDown={(event) => {
				if (event.key === "Escape") {
					event.preventDefault();
					if (!busy) onClose();
				}
			}}
			onCancel={(event) => {
				event.preventDefault();
				if (!busy) onClose();
			}}
			onClick={(event) => {
				if (event.target !== event.currentTarget || busy) return;
				const r = event.currentTarget.getBoundingClientRect();
				if (
					event.clientX < r.left ||
					event.clientX > r.right ||
					event.clientY < r.top ||
					event.clientY > r.bottom
				)
					onClose();
			}}
		>
			<div className="modal-title">
				<h2 id={id}>{title}</h2>
				<button
					type="button"
					className="icon-button"
					aria-label="关闭弹窗"
					disabled={busy}
					onClick={onClose}
				>
					<Icon name="close" />
				</button>
			</div>
			{children}
		</dialog>
	);
}
