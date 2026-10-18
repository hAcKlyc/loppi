import type { Moment } from "./api";

const labels = { mark: "重点", comment: "笔记", screenshot: "截图" };

function MomentIcon({ kind }: { kind: Moment["kind"] }) {
	return (
		<svg
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.7"
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
		>
			{kind === "mark" ? (
				<path d="M6 3h12v18l-6-4-6 4Z" />
			) : kind === "comment" ? (
				<>
					<path d="M5 3h14v18H5Z" />
					<path d="M9 8h6M9 12h6M9 16h4" />
				</>
			) : (
				<>
					<rect x="3" y="4" width="18" height="16" rx="2" />
					<circle cx="8" cy="9" r="1" />
					<path d="m3 17 5-5 4 4 4-6 5 7" />
				</>
			)}
		</svg>
	);
}

export function MomentList({
	moments,
	formatTime,
	onPlay,
	onScreenshot,
	loadingImage,
}: {
	moments: Moment[];
	formatTime: (at: number) => string;
	onPlay: (at: number) => void;
	onScreenshot: (id: string) => void;
	loadingImage: string | null;
}) {
	return (
		<ol className="notes-stream" aria-label="现场记录，按时间排列">
			{moments.length === 0 && (
				<li className="notes-empty">
					<strong>还没有现场记录</strong>
					<span>
						用顶部兔子标记重点、截屏，
						<br />
						也可以在下方写一条笔记。
					</span>
				</li>
			)}
			{moments.map((moment) => (
				<li className={`moment-record ${moment.kind}`} key={moment.id}>
					<div className="moment-meta">
						<span className="moment-kind">
							<MomentIcon kind={moment.kind} />
							{labels[moment.kind]}
						</span>
						<button
							className="note-time"
							type="button"
							title="从此刻回听"
							aria-label={`从 ${formatTime(moment.anchor_ms)} 回听`}
							onClick={() => onPlay(moment.anchor_ms)}
						>
							{formatTime(moment.anchor_ms)}
							<span aria-hidden="true"> ↗</span>
						</button>
					</div>
					{moment.kind === "comment" && (
						<p className="moment-text">{moment.text}</p>
					)}
					{moment.kind === "screenshot" && (
						<>
							<button
								type="button"
								className="screenshot-card"
								disabled={loadingImage !== null}
								aria-label={`查看 ${formatTime(moment.anchor_ms)} 的截图`}
								onClick={() => onScreenshot(moment.id)}
							>
								<span>
									{loadingImage === moment.id ? "正在打开…" : "查看截图"}
								</span>
								<span aria-hidden="true">↗</span>
							</button>
							{moment.ocr_text?.trim() ? (
								<details className="moment-ocr">
									<summary>识别文字</summary>
									<p className="moment-ocr-text">{moment.ocr_text}</p>
								</details>
							) : (
								<p
									className={`moment-status ${moment.ocr_state === "failed" ? "failed" : ""}`}
								>
									{moment.ocr_state === "failed"
										? "图片已保存，文字识别未成功"
										: moment.ocr_state === "complete"
											? "未识别到文字"
											: "暂无识别文字"}
								</p>
							)}
						</>
					)}
				</li>
			))}
		</ol>
	);
}
