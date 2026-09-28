import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { FloatingWidget } from "./FloatingWidget";

const floating = isTauri() && getCurrentWindow().label === "float";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
	<React.StrictMode>
		{floating ? <FloatingWidget /> : <App />}
	</React.StrictMode>,
);
