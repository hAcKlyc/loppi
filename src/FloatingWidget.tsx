import { invoke } from "@tauri-apps/api/core";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { core, type ServiceState } from "./api";
import "./App.css";

export function FloatingWidget() {
	const [state, setState] = useState<ServiceState | null>(null);
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	const [expanded, setExpanded] = useState(false);
	const pointer = useRef<{ x: number; y: number } | null>(null);
	const dragged = useRef(false);
	const desiredExpanded = useRef(false);
	const windowExpanded = useRef(false);
	const resizeQueue = useRef(Promise.resolve());

	const changeMenu = (next: boolean) => {
		if (desiredExpanded.current === next) return;
		desiredExpanded.current = next;
		setExpanded(next);
		resizeQueue.current = resizeQueue.current
			.then(async () => {
				if (windowExpanded.current === next) return;
				const float = getCurrentWindow();
				const [position, scale] = await Promise.all([
					float.outerPosition(),
					float.scaleFactor(),
				]);
				const dx = Math.round((132 - 68) * scale);
				const dy = Math.round((246 - 68) * scale);
				if (next) {
					await float.setSize(new LogicalSize(132, 246));
					await float.setPosition(
						new PhysicalPosition(position.x - dx, position.y - dy),
					);
				} else {
					await float.setPosition(
						new PhysicalPosition(position.x + dx, position.y + dy),
					);
					await float.setSize(new LogicalSize(68, 68));
				}
				windowExpanded.current = next;
			})
			.catch((error) => {
				setExpanded(windowExpanded.current);
				setMessage(String(error));
			});
	};

	useEffect(() => {
		document.body.classList.add("floating-body");
		const refresh = async () => {
			try {
				const response = await core<{
					type: "status";
					id: string;
					state: ServiceState;
				}>({ type: "status" });
				setState(response.state);
			} catch (error) {
				setMessage(String(error));
			}
		};
		void refresh();
		const timer = window.setInterval(() => void refresh(), 5000);
		return () => {
			window.clearInterval(timer);
			document.body.classList.remove("floating-body");
		};
	}, []);

	const perform = async (kind: "mark" | "screenshot" | "recording") => {
		if (busy) return;
		setBusy(true);
		try {
			if (kind === "recording") {
				await core({
					type:
						state?.capture_state === "recording"
							? "pause"
							: state?.enabled_intent
								? "resume"
								: "start",
					expected_revision: state?.revision,
				});
				const response = await core<{
					type: "status";
					id: string;
					state: ServiceState;
				}>({ type: "status" });
				setState(response.state);
				setMessage(response.state.paused_intent ? "已暂停" : "正在记录");
			} else {
				const anchor = Date.now();
				await core(
					kind === "mark"
						? {
								type: "add_moment",
								kind: "mark",
								anchor_ms: anchor,
								text: null,
							}
						: { type: "capture_screenshot" },
				);
				await emit("hear-moment-changed");
				setMessage(kind === "mark" ? "已标记" : "截屏已保存");
			}
		} catch (error) {
			setMessage(String(error));
		} finally {
			setBusy(false);
			window.setTimeout(() => setMessage(""), 4500);
		}
	};

	const openComment = async () => {
		const anchor_ms = Date.now();
		try {
			await invoke("show_main");
			await emit("hear-open-comment", { anchor_ms });
		} catch (error) {
			setMessage(String(error));
		}
	};

	const recording = state?.capture_state === "recording";
	return (
		<div
			className="floating-shell"
			role="toolbar"
			aria-label="快速操作"
			onMouseEnter={() => changeMenu(true)}
			onMouseLeave={() => changeMenu(false)}
			onFocusCapture={() => changeMenu(true)}
			onBlurCapture={(event) => {
				if (!event.currentTarget.contains(event.relatedTarget)) {
					changeMenu(false);
				}
			}}
		>
			{message && (
				<div className="floating-feedback" role="status">
					{message}
				</div>
			)}
			<div className={`float-wrap ${expanded ? "expanded" : ""}`}>
				<div className="float-menu">
					<button
						type="button"
						disabled={busy}
						onClick={() => void perform("recording")}
					>
						{recording ? "暂停" : state?.enabled_intent ? "继续" : "开始"}
					</button>
					<button type="button" onClick={() => void openComment()}>
						评论
					</button>
					<button
						type="button"
						disabled={busy}
						onClick={() => void perform("screenshot")}
					>
						截屏
					</button>
				</div>
				<button
					type="button"
					className={`float-main ${recording ? "recording" : ""}`}
					aria-label="标记重点；拖动以移动悬浮球"
					title="重点"
					onPointerDown={(event) => {
						pointer.current = { x: event.screenX, y: event.screenY };
						dragged.current = false;
					}}
					onPointerMove={(event) => {
						if (!pointer.current || dragged.current) return;
						if (
							Math.hypot(
								event.screenX - pointer.current.x,
								event.screenY - pointer.current.y,
							) >= 7
						) {
							dragged.current = true;
							void getCurrentWindow().startDragging();
						}
					}}
					onPointerUp={() => {
						pointer.current = null;
					}}
					onClick={() => {
						if (dragged.current) {
							dragged.current = false;
							return;
						}
						void perform("mark");
					}}
				>
					◉
				</button>
			</div>
		</div>
	);
}
