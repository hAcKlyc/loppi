import modelIcon from "./assets/analysis/model.png";
import speechIcon from "./assets/analysis/speech.png";
import strategyIcon from "./assets/analysis/strategy.png";
import { recognitionServices } from "./recognition-services";

export const analysisModules = {
	coarse: {
		title: "语音识别·粗转录",
		description: "使用本地模型完成语音识别",
		icon: speechIcon,
	},
	labels: { ...recognitionServices.llm, icon: modelIcon },
	precise: { ...recognitionServices.asr, icon: speechIcon },
	automation: {
		title: "自定义策略",
		description: "根据标签设置自定义的大模型分析策略",
		icon: strategyIcon,
	},
};

export type AnalysisSection = keyof typeof analysisModules;

export function AnalysisIcon({ module }: { module: AnalysisSection }) {
	return (
		<img className="analysis-icon" src={analysisModules[module].icon} alt="" />
	);
}

function AnalysisStep({ module }: { module: AnalysisSection }) {
	const { title, description } = analysisModules[module];
	return (
		<div className="analysis-step">
			<AnalysisIcon module={module} />
			<div>
				<strong>{title}</strong>
				<span>{description}</span>
			</div>
		</div>
	);
}

export function AnalysisOverview() {
	return (
		<figure className="analysis-flow" aria-label="智能分析流程">
			<AnalysisStep module="coarse" />
			<span className="analysis-flow-arrow" aria-hidden="true" />
			<AnalysisStep module="labels" />
			<span className="analysis-flow-connector">
				<span>事件确定后</span>
			</span>
			<div className="analysis-flow-outputs">
				<AnalysisStep module="precise" />
				<AnalysisStep module="automation" />
			</div>
		</figure>
	);
}
