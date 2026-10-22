import type { ReactNode } from "react";
import { Button } from "./Controls";

export function LoadingSkeleton({
	label = "正在加载",
	rows = 3,
	compact = false,
}: {
	label?: string;
	rows?: number;
	compact?: boolean;
}) {
	return (
		<div
			className={`loading-skeleton${compact ? " compact" : ""}`}
			role="status"
			aria-label={label}
			aria-busy="true"
		>
			{Array.from({ length: rows }, (_, row) => `placeholder-${row}`).map(
				(key) => (
					<div className="loading-skeleton-row" key={key} aria-hidden="true">
						<span className="skeleton-bar skeleton-meta" />
						<span className="skeleton-bar" />
						<span className="skeleton-bar skeleton-short" />
					</div>
				),
			)}
		</div>
	);
}

// Presentation only: readiness and errors stay with each existing request owner.
export function LoadBoundary({
	ready,
	error,
	retry,
	label,
	children,
	rows,
	compact,
}: {
	ready: boolean;
	error?: string | null;
	retry?: () => void;
	label: string;
	children: ReactNode;
	rows?: number;
	compact?: boolean;
}) {
	return (
		<>
			{error && (
				<div className="detail-load-error" role="alert">
					<span>
						{label}读取失败：{error}
					</span>
					{retry && (
						<Button type="button" tone="quiet" onClick={retry}>
							重试
						</Button>
					)}
				</div>
			)}
			{ready ? (
				children
			) : !error ? (
				<LoadingSkeleton
					label={`正在加载${label}`}
					rows={rows}
					compact={compact}
				/>
			) : null}
		</>
	);
}
