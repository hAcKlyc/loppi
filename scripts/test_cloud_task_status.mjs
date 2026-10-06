import assert from 'node:assert/strict';
import { test } from 'node:test';
import { taskStatus } from '../src/cloud-task-status.ts';

test('ASR mapping and response errors are distinguished from transport failures', () => {
 const reasons = {
  provider_unmapped_timing: '识别时间与原声不匹配',
  provider_invalid_timing: '识别时间超出录音范围',
  provider_invalid_utterance: '识别结果格式异常',
  invalid_fit_boundary: '整理结果时间范围异常',
  provider_http_429: '服务繁忙，请稍后重试',
  provider_transport: '云端服务异常',
  provider_http_503: '云端服务异常',
  credential_missing: '请检查云端配置',
  audio_unavailable: '原声不可用',
 };
 for (const [error_code, reason] of Object.entries(reasons)) {
  assert.equal(taskStatus({state:'failed',error_code}),`处理失败 · ${reason}`);
 }
});
test('a stale error cannot make an active or complete task look failed', () => {
 for (const [state,label] of Object.entries({pending:'等待处理',processing:'处理中',submitted:'等待识别结果',complete:'已完成',no_transcript:'无可识别文字',superseded:'已由新范围替代'})) {
  assert.equal(taskStatus({state,error_code:'provider_unmapped_timing'}),label);
 }
 assert.equal(taskStatus({state:'failed',error_code:null}),'处理失败');
});
