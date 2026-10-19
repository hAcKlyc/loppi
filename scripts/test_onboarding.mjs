import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { setupBlocker, saveCloud, confirmCloud, cloudOperation } from '../src/setup.ts';
globalThis.window = globalThis;
globalThis.isTauri = true;
window.__TAURI_INTERNALS__ = {};
const state={disk_free_bytes:3_000_000_000,audio_used_bytes:0,audio_limit_bytes:1000,microphone_enabled:true,system_enabled:true,microphone_phase:'idle',system_phase:'idle'};
const models={phase:'ready',speaker_phase:'failed'};
const privacy={microphone:3,microphone_available:true,tap_required:true,tap_ready:true,screen_and_system_audio:0};
const queues=[{provider:'llm',configured:true},{provider:'asr',configured:true}];
test('completion checks selected sources, both providers and necessary model, not optional speaker',()=>{
 assert.equal(setupBlocker(state,models,privacy,queues),'');
 assert.match(setupBlocker({...state,disk_free_bytes:1_073_741_824},models,privacy,queues),/空间不足/);
 assert.match(setupBlocker({...state,audio_used_bytes:1000},models,privacy,queues),/空间不足/);
 assert.match(setupBlocker(state,{...models,phase:'loading'},privacy,queues),/本地/);
 assert.match(setupBlocker(state,models,privacy,queues.slice(0,1)),/两项/);
 assert.match(setupBlocker(state,models,{...privacy,tap_ready:false},queues),/电脑/);
 assert.equal(setupBlocker({...state,system_enabled:false},models,{...privacy,tap_ready:false},queues),'');
 assert.match(setupBlocker({...state,system_enabled:false,microphone_enabled:false},models,privacy,queues),/至少/);
 assert.match(setupBlocker(state,models,{...privacy,microphone_available:false},queues),/麦克风/);
 assert.match(setupBlocker({...state,system_phase:'unavailable'},models,privacy,queues),/电脑/);
 assert.match(setupBlocker(state,models,privacy,queues,'offline'),/读取/);
 assert.match(setupBlocker(null,null,null,[]),/正在读取/);
});
test('lost verification reply is queried using original ID, never resubmitted; providers remain independent',async()=>{
 const calls=[];let original;
 mockIPC((command,args)=>{
  assert.equal(command,'core_request');calls.push(args.request.type);
  if(args.request.type==='configure_cloud') {
   if(args.request.provider==='asr') return {type:'cloud_status',id:args.requestId};
   original=args.requestId;throw new Error('transport lost');
  }
  assert.equal(args.request.request_id,original);
  return {type:'cloud_configuration_receipt',id:'query',pending:false,response:{type:'cloud_status',id:original}};
 });
 await assert.rejects(saveCloud('llm','synthetic-key'),/transport/);
 assert.equal(cloudOperation('llm'),original);
 await assert.rejects(saveCloud('llm','synthetic-key'),/先确认/);
 await saveCloud('asr','synthetic-key');
 assert.equal(await confirmCloud('llm'),'saved');
 assert.equal(cloudOperation('llm'),undefined);
 assert.deepEqual(calls,['configure_cloud','configure_cloud','cloud_configuration_receipt']);clearMocks();
});
test('pending and unknown receipts never claim success; definite failures allow explicit retry',async()=>{
 mockIPC(()=>{throw new Error('lost');});await assert.rejects(saveCloud('asr','test'));
 mockIPC(()=>({type:'cloud_configuration_receipt',id:'q',pending:true,response:null}));
 assert.equal(await confirmCloud('asr'),'pending');assert.ok(cloudOperation('asr'));
 mockIPC(()=>({type:'cloud_configuration_receipt',id:'q',pending:false,response:null}));
 assert.equal(await confirmCloud('asr'),'unknown');assert.equal(cloudOperation('asr'),undefined);
 mockIPC(()=>({type:'error',id:'e',code:'provider_http_401',message:'rejected'}));
 await assert.rejects(saveCloud('asr','test'),/401/);assert.equal(cloudOperation('asr'),undefined);clearMocks();
});
