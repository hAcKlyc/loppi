import { useEffect, useRef } from "react";
import { dayMapPoint } from "./timeline-view";

/** Pointer decoration only: never intercept range selection or re-render the list. */
export function DayMapCursor({ from, to }: { from: number; to: number }) {
	const trackRef = useRef<HTMLDivElement>(null);
	const lineRef = useRef<HTMLSpanElement>(null);
	const labelRef = useRef<HTMLSpanElement>(null);
	useEffect(() => {
		const track = trackRef.current;
		const plot = track?.parentElement;
		const line = lineRef.current;
		const label = labelRef.current;
		if (!track || !plot || !line || !label) return;
		let frame = 0;
		let clientX = 0;
		const hide = () => {
			cancelAnimationFrame(frame);
			frame = 0;
			delete track.dataset.visible;
		};
		const draw = () => {
			frame = 0;
			const bounds = track.getBoundingClientRect();
			if (clientX < bounds.left || clientX > bounds.right) {
				hide();
				return;
			}
			const point = dayMapPoint(clientX, bounds.left, bounds.width, from, to);
			if (!point) return hide();
			line.style.transform = `translateX(${point.x}px)`;
			label.textContent = point.label;
			const labelX = Math.max(
				0,
				Math.min(
					bounds.width - label.offsetWidth,
					point.x - label.offsetWidth / 2,
				),
			);
			label.style.transform = `translateX(${labelX}px)`;
			track.dataset.visible = "true";
		};
		const move = (event: PointerEvent) => {
			if (event.pointerType === "touch") return hide();
			clientX = event.clientX;
			if (!frame) frame = requestAnimationFrame(draw);
		};
		plot.addEventListener("pointermove", move, { passive: true });
		plot.addEventListener("pointerleave", hide);
		plot.addEventListener("pointercancel", hide);
		window.addEventListener("blur", hide);
		window.addEventListener("scroll", hide, true);
		const resize = new ResizeObserver(hide);
		resize.observe(track);
		return () => {
			hide();
			resize.disconnect();
			plot.removeEventListener("pointermove", move);
			plot.removeEventListener("pointerleave", hide);
			plot.removeEventListener("pointercancel", hide);
			window.removeEventListener("blur", hide);
			window.removeEventListener("scroll", hide, true);
		};
	}, [from, to]);
	return (
		<div ref={trackRef} className="day-map-cursor-track" aria-hidden="true">
			<span ref={lineRef} className="day-map-cursor-line" />
			<span ref={labelRef} className="day-map-cursor-label" />
		</div>
	);
}
