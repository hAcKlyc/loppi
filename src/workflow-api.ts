import { useRef } from "react";
import { core } from "./api";

export type WorkflowInput = "coarse" | "precise";
export type ManualWorkflowInput = "configured" | "coarse" | "prepare_precise";
export type WorkflowLabel = { id: string; name: string };
export type WorkflowTag = WorkflowLabel & { criteria: string };
export type WorkflowStrategy = {
	id: string;
	name: string;
	prompt: string;
	input: WorkflowInput;
	tag_ids: string[];
};
export type WorkflowConfig = {
	mode: "single" | "multiple";
	tags: WorkflowTag[];
	auto_asr: { mode: "all" } | { mode: "tags"; tag_ids: string[] };
	strategies: WorkflowStrategy[];
};
export type WorkflowSettings = {
	revision: number;
	config: WorkflowConfig;
	other_tag: string;
};
export type EventWorkflow = {
	phase: "undetermined" | "judging" | "decided";
	tag_state: "tagged" | "unclassified" | "pending" | "failed";
	labels: WorkflowLabel[];
	manual: boolean;
};
export type WorkflowRun = {
	run_id: string;
	strategy_id: string;
	name: string;
	input: WorkflowInput;
	state: string;
	reason?: string;
	task_id?: string;
	asr_task_ids: string[];
	retry_asr_task_id?: string;
	result_version_id?: string;
	created_ms: number;
	completed_ms?: number;
};
export type WorkflowOutput = {
	run: WorkflowRun;
	output: { append: boolean; cursor: string; body_markdown: string } | null;
};

const errors: Record<string, string> = {
	asr_rule_conflict: "这些策略的触发标签需要加入自动精转",
	tag_rule_conflict: "请先调整仍引用该标签的规则",
	revision_conflict: "事件或设置已更新，请刷新后重试",
	plan_stale: "处理计划已更新，请核对后重新确认",
	tag_selection_stale: "标签设置已更新，请重新选择并确认",
	unknown_tag: "所选标签已不可用，请重新选择",
	invalid_tag_selection: "请选择符合单选或多选要求的标签",
	invalid_tag_definition: "请填写各不相同的标签名称和判断标准",
	invalid_strategy_definition: "请填写各不相同的策略名称和 Prompt",
	input_unavailable: "当前事件没有可用的转录输入",
	input_too_large: "当前事件的转录过长，无法一次处理",
	configuration_too_large: "标签或策略配置过长",
	event_not_determined: "请先确认事件结束并等待自动流程判断",
	event_range_conflict: "此范围已归入另一个事件，请重新选择",
	event_not_found: "此事件已不可用",
	strategy_not_found: "此策略已不可用，请重新选择",
	result_unavailable: "此结果已不可用",
	run_not_found: "此生成任务已不可用",
};

export class WorkflowError extends Error {
	constructor(
		readonly code: string,
		readonly details: unknown,
	) {
		super(errors[code] ?? "操作未完成，请刷新后重试");
	}
}

export async function workflow<T>(
	action: { op: string; [key: string]: unknown },
	requestId?: string,
): Promise<T> {
	const response = await core<{
		type: string;
		id: string;
		result: {
			ok: boolean;
			data: T;
			error?: { code: string; details?: unknown };
		};
	}>({ type: "workflow", action }, requestId);
	if (!response.result.ok) {
		const error = response.result.error;
		throw new WorkflowError(error?.code ?? "workflow_failed", error?.details);
	}
	return response.result.data;
}

export function inputLabel(input: WorkflowInput) {
	return input === "precise" ? "精转录" : "粗转录";
}

export function selectedIds(ids: string[], id: string, checked: boolean) {
	return checked ? [...ids, id] : ids.filter((item) => item !== id);
}

export function useWorkflowRequestId() {
	const last = useRef<{ key: string; id: string } | null>(null);
	return (action: object) => {
		const key = JSON.stringify(action);
		if (last.current?.key !== key) {
			last.current = { key, id: crypto.randomUUID() };
		}
		return last.current.id;
	};
}
