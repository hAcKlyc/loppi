import modelIcon from "./assets/analysis/model.png";
import speechIcon from "./assets/analysis/speech.png";
import strategyIcon from "./assets/analysis/strategy.png";

const analysisModules = {
	coarse: {
		title: "语音识别·粗转录",
		description: "使用本地模型完成语音识别",
		icon: speechIcon,
	},
	event: {
		title: "事件分析",
		description: "使用云端大模型事件分析与智能标签",
		icon: modelIcon,
	},
	precise: {
		title: "语音识别·精转录",
		description: "使用云端模型针对事件/标签二次识别",
		icon: speechIcon,
	},
	strategy: {
		title: "自定义策略",
		description: "根据标签设置自定义的大模型分析策略",
		icon: strategyIcon,
	},
};

function AnalysisStep({ module }: { module: keyof typeof analysisModules }) {
	const { title, description, icon } = analysisModules[module];
	return (
		<div className="analysis-step">
			<img className="analysis-icon" src={icon} alt="" />
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
			<AnalysisStep module="event" />
			<span className="analysis-flow-connector" aria-hidden="true" />
			<div className="analysis-flow-outputs">
				<AnalysisStep module="precise" />
				<AnalysisStep module="strategy" />
			</div>
		</figure>
	);
}
