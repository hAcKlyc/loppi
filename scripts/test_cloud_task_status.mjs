import assert from 'node:assert/strict';
import { test } from 'node:test';
import { taskStatus } from '../src/cloud-task-status.ts';

test('ASR mapping and response errors are distinguished from transport failures', () => {
 const reasons = {
  provider_unmapped_timing: '识别时间与原声不匹配',
  provider_unmapped_timing_word_in_separator: '识别时间与原声不匹配',
  provider_unmapped_timing_audio_coverage: '结果未保存，可重试获取',
  provider_invalid_timing: '识别时间超出录音范围',
  provider_invalid_utterance: '识别结果格式异常',
  invalid_fit_boundary: '整理结果时间范围异常',
  invalid_fit_boundary_overlap: '整理结果时间范围异常',
  invalid_fit_boundary_uncovered_speech: '整理结果遗漏了语音片段',
  provider_status_45000092: '云端未找到识别任务，可重试',
  provider_http_429: '自动重试已停止，请检查网络或服务后手动重试',
  provider_transport: '自动重试已停止，请检查网络或服务后手动重试',
  provider_http_503: '自动重试已停止，请检查网络或服务后手动重试',
  provider_http_402: '云端余额不足，请充值后手动重试',
  provider_invalid_utterance_text: '识别结果格式异常',
  credential_missing: '请检查云端配置',
  audio_unavailable: '原声不可用',
 };
 for (const [error_code, reason] of Object.entries(reasons)) {
  assert.equal(taskStatus({state:'failed',error_code}),`处理失败 · ${reason}`);
 }
});
test('a stale error cannot make an active or complete task look failed', () => {
 for (const [state,label] of Object.entries({processing:'处理中',complete:'已完成',no_transcript:'无可识别文字',superseded:'已由新范围替代'})) {
  assert.equal(taskStatus({state,error_code:'provider_unmapped_timing'}),label);
 }
 assert.equal(taskStatus({state:'failed',error_code:null}),'处理失败');
});
test('queued retries expose credential and transport failures instead of looking idle', () => {
 for (const state of ['pending','submitted','failed']) {
  assert.match(taskStatus({state,error_code:'keychain_read_-25293'}), /无法读取钥匙串密钥/);
  assert.match(taskStatus({state,error_code:'keychain_read_-25308'}), /重新打开 loppi/);
 }
 assert.equal(taskStatus({state:'pending',error_code:'provider_transport'}),'等待重试 · 云端服务异常');
 assert.equal(taskStatus({state:'pending',error_code:'provider_http_429'}),'等待重试 · 服务繁忙，请稍后重试');
 assert.equal(taskStatus({state:'pending',error_code:null}),'等待处理');
 assert.equal(taskStatus({state:'submitted',error_code:null}),'等待识别结果');
});

test('only rejected ASR results offer an explicit paid recognition', async () => {
 const { taskNeedsRecognition } = await import('../src/cloud-task-status.ts');
 const invalid=['provider_unmapped_timing','provider_unmapped_timing_word_in_separator','provider_invalid_timing','provider_invalid_utterance','provider_invalid_utterance_text','provider_missing_utterances'];
 for(const error_code of invalid) {
  assert.equal(taskNeedsRecognition({provider:'asr',state:'failed',error_code}),true);
  assert.equal(taskNeedsRecognition({provider:'llm',state:'failed',error_code}),false);
  assert.equal(taskNeedsRecognition({provider:'asr',state:'submitted',error_code}),false);
 }
 for(const error_code of ['provider_unmapped_timing_audio_coverage','provider_transport','provider_deadline','provider_read_failed','keychain_read_-25308',null]) {
  assert.equal(taskNeedsRecognition({provider:'asr',state:'failed',error_code}),false);
 }
});

test('queue renders paid reruns separately from request retries and disables busy actions', async () => {
 const { createServer } = await import('vite');
 const { createElement } = await import('react');
 const { renderToStaticMarkup } = await import('react-dom/server');
 const server=await createServer({server:{middlewareMode:true},appType:'custom'});
 try {
  const { QueueView }=await server.ssrLoadModule('/src/WorkspaceViews.tsx');
  const task={id:'failed',provider:'asr',target_kind:'event',target_id:'event',source:'microphone',started_ms:1000,ended_ms:6000,state:'failed',manual:false};
  const render=(error_code,busy=false)=>renderToStaticMarkup(createElement(QueueView,{queues:[],tasks:[{...task,error_code}],reviews:[],busy,onRefresh:async()=>{},onMutate:async()=>true}));
  const rejected=render('provider_invalid_utterance');
  assert.match(rejected,/>重新识别<\/button>/);
  assert.match(rejected,/产生新的识别用量/);
  const uncertain=render('provider_transport');
  assert.match(uncertain,/>重试<\/button>/);
  assert.doesNotMatch(uncertain,/产生新的识别用量/);
  const recoverable=render('provider_unmapped_timing_audio_coverage');
  assert.match(recoverable,/>重试<\/button>/);
  assert.match(recoverable,/结果未保存，可重试获取/);
  assert.doesNotMatch(recoverable,/重新识别|产生新的识别用量/);
  assert.match(render('provider_unmapped_timing_word_in_separator',true),/disabled=""[^>]*>重新识别<\/button>/);
 } finally { await server.close(); }
});

test('completed partial results distinguish empty audio and cropped text, with legacy fallback', async () => {
 const {createServer}=await import('vite');
 const server=await createServer({server:{middlewareMode:true},appType:'custom'});
 try {
  const {processingLabel}=await server.ssrLoadModule('/src/api.ts');
  assert.equal(processingLabel('partial_transcript'),'精转完成 · 部分文字不可用');
  assert.equal(processingLabel('no_transcript'),'精转完成 · 无可识别文字');
  assert.equal(processingLabel('no_transcript',{total:2,complete:2,empty:2,boundary_clipped:false}),'精转完成 · 2 段无文字');
  assert.equal(processingLabel('partial_transcript',{empty:0,boundary_clipped:true}),'精转完成 · 边界处文字待核对');
  assert.equal(processingLabel('partial_transcript',{empty:2,boundary_clipped:true}),'精转完成 · 2 段无文字 · 边界处文字待核对');
  assert.equal(processingLabel('transcribing',{empty:2,boundary_clipped:false}),'精转中');
  assert.equal(processingLabel('partial_asr_failed'),'部分精转失败 · 已完成文字保留');
 } finally {await server.close();}
});
