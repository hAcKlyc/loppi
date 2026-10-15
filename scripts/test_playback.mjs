import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ContinuousPlayback } from '../src/playback.ts';
const settle = async () => { for (let n=0;n<12;n++) await Promise.resolve(); };
function audio() {
 const nodes=[];
 const context={currentTime:0,destination:{},resume:async()=>{},suspend:async()=>{},close:async()=>{},
 decodeAudioData:async()=>({duration:5,sampleRate:48000,numberOfChannels:1}),
 createBufferSource(){const node={buffer:null,onended:null,when:null,stopped:false,connect(){},disconnect(){},start(t){this.when=t;},stop(){this.stopped=true;}};nodes.push(node);return node;}};
 return {nodes,context,end(index){const n=nodes[index];context.currentTime=n.when+5;n.onended?.();}};
}
const chunk={wav_base64:'AA==',has_audio:true,next_audio_ms:null};
test('audio-clock continuation prefetches only one ahead, releases buffers, and completes',async()=>{
 const a=audio();const requests=[];const positions=[];let done=0;
 const player=new ContinuousPlayback(async(from,to)=>{requests.push([from,to]);return chunk;},p=>positions.push(p),()=>done++,e=>{throw e;},()=>a.context);
 try {
  player.start(1000,16000,'both');await settle();
  assert.deepEqual(requests,[[1000,6000],[6000,11000]]);
  assert.equal(a.nodes[1].when,a.nodes[0].when+5);
  a.end(0);await settle();assert.equal(requests.length,3);assert.equal(a.nodes[0].buffer,null);
  assert.equal(a.nodes[2].when,a.nodes[1].when+5);
  a.end(1);await settle();assert.equal(done,0);
  a.end(2);await settle();assert.equal(done,1);assert.equal(positions.at(-1),16000);
 } finally {player.dispose();}
});
test('seek/source/close discard late responses without concurrent requests or stale audio',async()=>{
 const a=audio();const requests=[];let resolve;
 const player=new ContinuousPlayback((from,to,source)=>{requests.push([from,to,source]);return new Promise(r=>{resolve=r;});},()=>{},()=>{},()=>{},()=>a.context);
 player.start(0,20000,'microphone');await settle();
 player.start(10000,20000,'system');await settle();assert.equal(requests.length,1);
 resolve(chunk);await settle();assert.equal(a.nodes.length,0);assert.deepEqual(requests[1],[10000,15000,'system']);
 resolve(chunk);await settle();assert.equal(a.nodes.length,1);
 const pending=resolve;player.dispose();pending(chunk);await settle();
 assert.equal(a.nodes.length,1);assert.equal(a.nodes[0].stopped,true);assert.equal(a.nodes[0].buffer,null);
});
test('empty ranges jump to real next media while preserving its absolute playhead',async()=>{
 const a=audio();const requested=[];const positions=[];
 const player=new ContinuousPlayback(async(from)=>{requested.push(from);return from===0?{...chunk,has_audio:false,next_audio_ms:10000}:chunk;},p=>positions.push(p),()=>{},e=>{throw e;},()=>a.context);
 try {player.start(0,15000,'system');await settle();assert.deepEqual(requested,[0,10000]);a.end(0);await settle();assert.equal(positions.at(-1),15000);}finally{player.dispose();}
});

test('prefetch failure preserves current audio and reports only at its boundary',async()=>{
 const a=audio();const errors=[];const requests=[];let done=0;
 const player=new ContinuousPlayback(async(from)=>{requests.push(from);if(from===5000)throw Error('missing next media');return chunk;},()=>{},()=>done++,e=>errors.push(e),()=>a.context);
 try {
  player.start(0,15000,'both');await settle();
  assert.deepEqual(requests,[0,5000]);assert.equal(a.nodes[0].stopped,false);assert.notEqual(a.nodes[0].buffer,null);
  assert.equal(done,0);assert.equal(errors.length,0);
  a.end(0);await settle();assert.equal(done,1);assert.equal(errors[0].message,'missing next media');assert.equal(requests.length,2);
 } finally {player.dispose();}
});
test('seek cancels deferred prefetch error and first-chunk failures still stop immediately',async()=>{
 const a=audio();const errors=[];let done=0;
 const player=new ContinuousPlayback(async(from)=>{if(from===5000||from===20000)throw Error('missing media');return chunk;},()=>{},()=>done++,e=>errors.push(e),()=>a.context);
 try {
  player.start(0,15000,'both');await settle();player.start(10000,15000,'system');await settle();
  a.end(1);await settle();assert.equal(done,1);assert.equal(errors.length,0);
  player.start(20000,25000,'both');await settle();assert.equal(done,2);assert.equal(errors.length,1);
 } finally {player.dispose();}
});
