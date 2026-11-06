import { useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Moment } from "./api";
import { Button } from "./Controls";

const labels = { mark: "重点", comment: "笔记", screenshot: "截图" };

function MomentIcon({ kind }: { kind: Moment["kind"] }) {
	return (
		<svg
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="var(--stroke-icon)"
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

function ScreenshotText({ moment }: { moment: Moment }) {
	const [copyState, setCopyState] = useState("");
	const [expanded, setExpanded] = useState(false);
	const ready = moment.image_summary != null && moment.image_transcript != null;
	const state = moment.image_state;
	const copy = async () => {
		try {
			await navigator.clipboard.writeText(moment.image_transcript ?? "");
			setCopyState("已复制");
		} catch {
			setCopyState("复制未成功，请选择文字复制");
		}
	};
	return (
		<>
			{ready ? (
				<>
					<p className="moment-text image-summary">{moment.image_summary}</p>
					{moment.image_transcript?.trim() ? (
						<details
							className="moment-ocr"
							onToggle={(event) => setExpanded(event.currentTarget.open)}
						>
							<summary>完整转录</summary>
							<div className="moment-markdown">
								{expanded && (
									<Markdown
										remarkPlugins={[remarkGfm]}
										skipHtml
										components={{
											img: ({ alt }) => <span>{alt}</span>,
											a: ({ children }) => <span>{children}</span>,
										}}
									>
										{moment.image_transcript}
									</Markdown>
								)}
							</div>
							<Button type="button" tone="quiet" onClick={() => void copy()}>
								复制 Markdown
							</Button>
							<span className="moment-status" role="status">
								{copyState}
							</span>
						</details>
					) : (
						<p className="moment-status">图片中没有可转录的文字</p>
					)}
				</>
			) : (
				<>
					{state && (
						<p
							className={`moment-status ${state === "failed" ? "failed" : ""}`}
						>
							{state === "paused"
								? "图片理解已暂停，可在队列中继续"
								: state === "failed"
									? "图片理解未成功，可在队列中重试"
									: state === "processing"
										? "正在理解图片…"
										: state === "pending"
											? "等待图片理解…"
											: "暂无图片理解结果"}
						</p>
					)}
					{moment.ocr_text?.trim() ? (
						<details className="moment-ocr">
							<summary>{state ? "本地识别文字" : "识别文字"}</summary>
							<p className="moment-ocr-text">{moment.ocr_text}</p>
						</details>
					) : (
						!state && (
							<p
								className={`moment-status ${moment.ocr_state === "failed" ? "failed" : ""}`}
							>
								{moment.ocr_state === "failed"
									? "图片已保存，文字识别未成功"
									: moment.ocr_state === "complete"
										? "未识别到文字"
										: "暂无识别文字"}
							</p>
						)
					)}
				</>
			)}
		</>
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
	onScreenshot: (id: string, trigger: HTMLButtonElement) => void;
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
								onClick={(event) => {
									event.currentTarget.focus({ preventScroll: true });
									onScreenshot(moment.id, event.currentTarget);
								}}
							>
								<span>
									{loadingImage === moment.id ? "正在打开…" : "查看截图"}
								</span>
								<span aria-hidden="true">↗</span>
							</button>
							<ScreenshotText moment={moment} />
						</>
					)}
				</li>
			))}
		</ol>
	);
}
