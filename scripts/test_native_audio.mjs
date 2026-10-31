import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { NativeAudio } from '../src/native-audio.ts';
globalThis.window=globalThis;
const settle=()=>new Promise(resolve=>setImmediate(resolve));
function host() {
 const calls=[]; let current=0,epoch=0,next=0,buffer=0;const buffers=new Set();
 mockIPC(async(command,args)=>{
  assert.equal(command,'playback_output');const c=args.command;calls.push(c);
  if(c.type==='open') {
   for(const id of buffers)await emit('loppi-playback-ended',{context:current,epoch,buffer:id,status:-1});
   buffers.clear();current=++next;epoch=0;return {context:current};
  }
  if(c.context!==current||c.epoch<epoch)throw Error('playback_stale');
  epoch=c.epoch;
  if(c.type==='resume')return {clock:0};
  if(c.type==='load') {const id=++buffer;buffers.add(id);return {buffer:id,duration:5,sampleRate:48000,numberOfChannels:1,clock:0};}
  if(c.type==='schedule')return {when:c.when};
  if(c.type==='remove')buffers.delete(c.buffer);
  if(c.type==='suspend'||c.type==='close')buffers.clear();
  if(c.type==='close')current=0;
  return {};
 },{shouldMockEvents:true});
 return {calls,buffers,get context(){return current;},get epoch(){return epoch;}};
}
test('native output completion is scoped to context and epoch, and pause releases buffers',async()=>{
 const h=host();const audio=new NativeAudio();let ended=[];
 try {
  await audio.resume();const buffer=await audio.decodeWav('synthetic');
  const node=audio.createBufferSource();node.buffer=buffer;node.onended=(...args)=>ended.push(args);
  assert.equal(await node.start(.1),.1);
  await emit('loppi-playback-ended',{context:h.context,epoch:h.epoch-1,buffer:buffer.buffer,status:1});await settle();
  assert.equal(ended.length,0);
  await emit('loppi-playback-ended',{context:h.context,epoch:h.epoch,buffer:buffer.buffer,status:1});await settle();
  assert.deepEqual(ended,[[true,false]]);
  await audio.suspend();assert.equal(h.buffers.size,0);
 } finally {await audio.close();clearMocks();}
});
test('stop before an asynchronous renderer open never starts a silent output',async()=>{
 const h=host();const audio=new NativeAudio();
 try {const resume=audio.resume();await audio.suspend();await resume;assert.equal(h.calls.length,0);}finally{await audio.close();clearMocks();}
});
test('a new player cancels the old context and a late old close cannot stop it',async()=>{
 const h=host();const old=new NativeAudio();let cancelled=false;
 await old.resume();const buffer=await old.decodeWav('synthetic');const node=old.createBufferSource();node.buffer=buffer;node.onended=(_success,cancel)=>{cancelled=cancel;};await node.start(.1);
 const next=new NativeAudio();
 try {
  await next.resume();await settle();assert(cancelled);
  const active=h.context;
  await assert.rejects(old.close(),/playback_stale/);
  assert.equal(h.context,active);
 } finally {await next.close();clearMocks();}
});
