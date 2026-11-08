import type { CloudProvider } from "./api";

export const recognitionServices: Record<
	CloudProvider,
	{ title: string; description: string }
> = {
	llm: {
		title: "事件分析",
		description: "使用云端大模型对本地转录文本进行事件分析识别",
	},
	asr: {
		title: "语音识别·精转录",
		description: "使用云端大模型对确认的事件进行二次精准语音识别",
	},
};
