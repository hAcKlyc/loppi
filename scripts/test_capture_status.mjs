import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { captureStatusView } from '../src/capture-status.ts';
import { EntryRequests, entryAppearance } from '../src/entry-api.ts';

globalThis.window = globalThis;
test('main, notch and floating share the host appearance, including unknown/error observations', () => {
 for (const appearance of ['listening','sleeping','waiting','partial','interrupted']) {
  assert.equal(entryAppearance({appearance,error:null}),appearance);
  assert.equal(entryAppearance({appearance,error:'core unavailable'}),'waiting');
 }
 assert.equal(entryAppearance(null),'waiting');
});
const snapshot = (appearance = 'listening', fields = {}, extra = {}) => ({
 sequence: 10, mode: 'top', error: null, detail: null, notice: null,
 appearance, control: 'pause',
 state: {
  enabled_intent: true, paused_intent: false, storage_paused: false,
  microphone_enabled: true, system_enabled: true,
  microphone_recording: true, system_recording: true,
  microphone_phase: 'recording', system_phase: 'recording', capture_blocker: 'none',
  ...fields,
 }, ...extra,
});
test('healthy copy names only actual recording sources, and partial never claims healthy capture', () => {
 assert.equal(captureStatusView(snapshot()).subtitle, '麦克风 · 电脑声音');
 assert.equal(captureStatusView(snapshot('listening', {system_enabled:false, system_recording:false, system_phase:'disabled'})).subtitle, '麦克风');
 const partial = captureStatusView(snapshot('partial', {system_recording:false, system_phase:'unavailable'}, {detail:'电脑声音暂不可用'}));
 assert.equal(partial.label, '部分记录');
 assert.equal(partial.tone, 'warning');
 assert.equal(partial.action, 'pause');
 assert.equal(partial.detail, '电脑声音暂不可用');
});
test('paused, never started, and query-only are distinct from real capture', () => {
 const stopped = {microphone_recording:false, system_recording:false, microphone_phase:'idle', system_phase:'idle'};
 const paused = captureStatusView(snapshot('sleeping', {...stopped, paused_intent:true}, {control:'resume'}));
 assert.equal(paused.label,'已暂停'); assert.equal(paused.action,'resume');
 const first = captureStatusView(snapshot('sleeping', {...stopped, enabled_intent:false}, {control:'start'}));
 assert.equal(first.label,'未开始'); assert.equal(first.action,'start');
 const query = captureStatusView(snapshot('waiting', {...stopped, capture_blocker:'query_only'}, {control:'resume'}));
 assert.equal(query.label,'尚未开始'); assert.equal(query.action,'resume');
});
test('unknown or failed observations disable blind controls even with stale healthy data', () => {
 assert.equal(captureStatusView(null).action,'retry');
 const failed=captureStatusView(snapshot('listening',{}, {error:'IPC lost'}));
 assert.equal(failed.label,'状态待确认'); assert.equal(failed.action,'retry'); assert.equal(failed.tone,'waiting');
});
test('stopping, source startup and maintenance do not claim stopped or recording', () => {
 const stopping=captureStatusView(snapshot('waiting',{microphone_phase:'stopping'}));
 assert.equal(stopping.label,'正在暂停'); assert.equal(stopping.action,null);
 const starting=captureStatusView(snapshot('waiting',{system_phase:'starting',system_recording:false}));
 assert.equal(starting.tone,'waiting'); assert.equal(starting.action,'pause');
 const maintenance=captureStatusView(snapshot('waiting',{capture_blocker:'maintenance'}));
 assert.equal(maintenance.label,'正在整理'); assert.equal(maintenance.action,null);
});
test('capacity and setup blockers lead to existing settings rather than a futile blind resume', () => {
 const full=captureStatusView(snapshot('interrupted',{storage_paused:true,microphone_recording:false,system_recording:false}, {control:'resume'}));
 assert.equal(full.label,'空间不足'); assert.equal(full.action,'settings');
 for(const capture_blocker of ['model_not_ready','setup_incomplete','sources_disabled']) {
  const view=captureStatusView(snapshot('waiting',{capture_blocker}));
  assert.equal(view.label,'尚未就绪'); assert.equal(view.action,'settings');
 }
 assert.equal(captureStatusView(snapshot('interrupted',{capture_blocker:'source_unavailable'})).label,'记录中断');
});
test('a control click is single-flight; a lost outcome confirms the original operation with the same ID', async () => {
 const calls=[]; let finish;
 mockIPC((command,args) => {
  assert.equal(command,'entry_action'); calls.push(structuredClone(args.action));
  return calls.length===1 ? new Promise(resolve=>{finish=resolve;}) : {outcome:'confirmed'};
 });
 try {
  const requests=new EntryRequests(); const pending=requests.send('control',100,null);
  await assert.rejects(requests.send('control',101,null),/操作正在处理/);
  assert.equal(calls.length,1);
  finish({outcome:'uncertain'}); await pending;
  await requests.send('control',999,null);
  assert.deepEqual(calls[0],calls[1]); assert.equal(requests.isUncertain('control'),false);
 } finally { clearMocks(); }
});
test('a competing control refused before dispatch is not retained as an uncertain confirmation', async () => {
 let hostPending=true; const sent=[];
 mockIPC((_command,args) => {
  if(hostPending) return {outcome:'rejected',message:'action_pending: 请先确认上一次操作的结果'};
  sent.push(structuredClone(args.action)); return {outcome:'confirmed'};
 });
 try {
  const main=new EntryRequests();
  const rejected=await main.send('control',100,null);
  assert.equal(rejected.outcome,'rejected');
  assert.equal(main.isUncertain('control'),false);
  assert.equal(sent.length,0);
  // Once the rabbit operation resolves, only a new explicit action is sent.
  hostPending=false;
  await main.send('control',200,null);
  assert.equal(sent.length,1); assert.equal(sent[0].anchor_ms,200);
 } finally { clearMocks(); }
});
test('confirming an accepted control still in flight retains its ID until its receipt is known', async () => {
 const calls=[];
 mockIPC((_command,args) => {
  calls.push(structuredClone(args.action));
  if(calls.length===1) throw new Error('bridge reply lost after host accepted');
  return {outcome:calls.length===2?'uncertain':'confirmed',message:'操作仍在处理'};
 });
 try {
  const main=new EntryRequests();
  await assert.rejects(main.send('control',100,null),/bridge reply lost/);
  assert.equal(main.isUncertain('control'),true);
  await main.send('control',200,null);
  assert.equal(main.isUncertain('control'),true);
  await main.send('control',300,null);
  assert.deepEqual(calls[0],calls[1]); assert.deepEqual(calls[1],calls[2]);
  assert.equal(main.isUncertain('control'),false);
 } finally { clearMocks(); }
});
test('recovered capacity keeps recording paused and offers the host resume control', () => {
 const view=captureStatusView(snapshot('sleeping', {storage_paused:true,storage_recoverable:true,microphone_recording:false,system_recording:false}, {control:'resume'}));
 assert.equal(view.label,'已暂停'); assert.equal(view.subtitle,'空间已恢复，可继续记录'); assert.equal(view.action,'resume');
});
