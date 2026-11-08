import type { CloudProvider } from "./api";

export const recognitionServices: Record<
	CloudProvider,
	{ title: string; description: string }
> = {
	llm: {
		title: "事件分析",
		description: "使用云端大模型事件分析与智能标签",
	},
	asr: {
		title: "语音识别·精转录",
		description: "使用云端模型针对事件/标签二次识别",
	},
};
