import { useEffect, useRef, useState } from "react";
import manifest from "./assets/rabbit/manifest.json";
import listening from "./assets/rabbit/poses/listening.png";
import partial from "./assets/rabbit/poses/partial.png";
import sleeping from "./assets/rabbit/poses/sleeping.png";
import waiting from "./assets/rabbit/poses/waiting.png";
import atlas from "./assets/rabbit/sprite-atlas.png";
import type { RabbitState } from "./entry-api";

type ClipName = keyof typeof manifest.clips;
type FrameName = keyof typeof manifest.frames;
let loaded: Promise<HTMLImageElement> | null = null;
function loadAtlas() {
	loaded ??= new Promise<HTMLImageElement>((resolve, reject) => {
		const image = new Image();
		image.onload = () => resolve(image);
		image.onerror = reject;
		image.src = atlas;
	});
	return loaded;
}
const pose = (state: RabbitState) =>
	({ listening, sleeping, waiting, partial, interrupted: waiting })[state];
export function Rabbit({
	state,
	action,
	visible = true,
}: {
	state: RabbitState;
	action: { name: "curious" | "nod"; id: number } | null;
	visible?: boolean;
}) {
	const canvas = useRef<HTMLCanvasElement>(null);
	const [backing, setBacking] = useState({ width: 48, height: 48 });
	const previous = useRef<RabbitState | null>(null);
	const lastAction = useRef<number | null>(null);
	const [failed, setFailed] = useState(false);
	const [reduced, setReduced] = useState(
		() => matchMedia("(prefers-reduced-motion: reduce)").matches,
	);
	const [documentVisible, setDocumentVisible] = useState(
		document.visibilityState === "visible",
	);
	useEffect(() => {
		const element = canvas.current;
		if (!element) return;
		const resize = () => {
			const rect = element.getBoundingClientRect();
			const width = Math.max(1, Math.round(rect.width * devicePixelRatio));
			const height = Math.max(1, Math.round(rect.height * devicePixelRatio));
			setBacking((old) =>
				old.width === width && old.height === height ? old : { width, height },
			);
		};
		const observer = new ResizeObserver(resize);
		observer.observe(element);
		window.addEventListener("resize", resize);
		resize();
		return () => {
			observer.disconnect();
			window.removeEventListener("resize", resize);
		};
	}, []);
	useEffect(() => {
		const media = matchMedia("(prefers-reduced-motion: reduce)");
		const changed = () => setReduced(media.matches);
		const visibility = () =>
			setDocumentVisible(document.visibilityState === "visible");
		media.addEventListener("change", changed);
		document.addEventListener("visibilitychange", visibility);
		return () => {
			media.removeEventListener("change", changed);
			document.removeEventListener("visibilitychange", visibility);
		};
	}, []);
	useEffect(() => {
		let disposed = false;
		let timer: number | undefined;
		const freshAction = action && action.id !== lastAction.current;
		if (action) lastAction.current = action.id;
		const old = previous.current;
		if (state === "listening" || state === "sleeping") previous.current = state;
		let first: ClipName = state;
		if (!reduced && visible && documentVisible) {
			if (old === "listening" && state === "sleeping") first = "pause";
			else if (old === "sleeping" && state === "listening") first = "stretch";
			else if (
				freshAction &&
				action &&
				(state === "listening" || state === "sleeping")
			)
				first = state === "sleeping" ? "sleeping-response" : action.name;
		}
		loadAtlas()
			.then((image) => {
				if (disposed) return;
				const context = canvas.current?.getContext("2d");
				if (!context) {
					setFailed(true);
					return;
				}
				context.imageSmoothingEnabled = false;
				const { width, height } = manifest.cell;
				const scale = Math.min(backing.width / width, backing.height / height);
				const draw = (frame: string) => {
					const f = manifest.frames[frame as FrameName];
					context.setTransform(1, 0, 0, 1, 0, 0);
					context.clearRect(0, 0, backing.width, backing.height);
					context.setTransform(
						scale,
						0,
						0,
						scale,
						(backing.width - width * scale) / 2,
						(backing.height - height * scale) / 2,
					);
					context.drawImage(
						image,
						f.x,
						f.y,
						f.width,
						f.height,
						0,
						0,
						width,
						height,
					);
				};
				const play = (name: ClipName, index = 0) => {
					if (disposed) return;
					const clip = manifest.clips[name];
					const [frame, delay] = clip.frames[index];
					draw(String(frame));
					if (reduced || !visible || !documentVisible) return;
					const next = index + 1;
					if (next < clip.frames.length)
						timer = window.setTimeout(() => play(name, next), Number(delay));
					else if (clip.loop)
						timer = window.setTimeout(() => play(name), Number(delay));
					else if (name !== state)
						timer = window.setTimeout(() => play(state), Number(delay));
				};
				if (reduced || !visible || !documentVisible)
					draw(manifest.clips[state].frames[0][0] as string);
				else play(first);
			})
			.catch(() => {
				if (!disposed) setFailed(true);
			});
		return () => {
			disposed = true;
			window.clearTimeout(timer);
		};
	}, [state, action, reduced, visible, documentVisible, backing]);
	return (
		<span className="rabbit-art">
			{failed ? (
				<span className="rabbit-fallback">
					<img src={pose(state)} alt="" />
				</span>
			) : (
				<canvas
					ref={canvas}
					width={backing.width}
					height={backing.height}
					className="rabbit-pixels"
				/>
			)}
			{state === "interrupted" && <span className="rabbit-warning">!</span>}
		</span>
	);
}
