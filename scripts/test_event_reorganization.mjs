// Real App and Tauri event bridge: overlapping refreshes and invalidation.
import assert from 'node:assert/strict';
import { registerHooks, createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
const requireTest = createRequire(import.meta.url);
const { JSDOM } = requireTest('jsdom');
const dom = new JSDOM('<div id="root"></div>', { url:'http://localhost/', pretendToBeVisual:true });
for (const name of ['window','document','HTMLElement','HTMLTextAreaElement','Event','MouseEvent','KeyboardEvent','MutationObserver']) globalThis[name]=dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT=true;
globalThis.ResizeObserver=class {observe(){} disconnect(){}};
window.isTauri=true; globalThis.isTauri=true;
globalThis.requestAnimationFrame=dom.window.requestAnimationFrame.bind(dom.window);
globalThis.cancelAnimationFrame=dom.window.cancelAnimationFrame.bind(dom.window);
registerHooks({
 resolve(specifier,context,next) {
  if(specifier.startsWith('.') && context.parentURL?.includes('/src/')) {
   for(const suffix of ['.ts','.tsx']) {
    const url=new URL(specifier+suffix,context.parentURL);
    if(existsSync(url)) return next(url.href,context);
   }
  }
  return next(specifier,context);
 },
 load(url,context,next) {
  if(url.endsWith('/Rabbit.tsx'))return {format:'module',source:'export function Rabbit(){return null}',shortCircuit:true};
  if(/\.(css|png|svg)$/.test(url)) return {format:'module',source:'export default "fixture-asset"',shortCircuit:true};
  if(/\.tsx?$/.test(url)) return {format:'module',shortCircuit:true,source:ts.transpileModule(readFileSync(fileURLToPath(url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext,jsx:ts.JsxEmit.ReactJSX}}).outputText};
  return next(url,context);
 }
});
const {default:React,act}=await import('react');
const {createRoot}=await import('react-dom/client');
const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');
const {emit}=await import('@tauri-apps/api/event');
const {default:App}=await import('../src/App.tsx');
const {dayBounds}=await import('../src/api.ts');
globalThis.HTMLDialogElement=window.HTMLDialogElement;
globalThis.HTMLButtonElement=window.HTMLButtonElement;
globalThis.HTMLInputElement=window.HTMLInputElement;
HTMLDialogElement.prototype.showModal=function(){this.open=true};
HTMLDialogElement.prototype.close=function(){this.open=false};
const today=dayBounds(new Date())[0];
const state={revision:1,enabled_intent:true,paused_intent:true,storage_paused:false,microphone_enabled:true,system_enabled:true,capture_state:'paused',microphone_phase:'idle',system_phase:'idle',microphone_recording:false,system_recording:false};
const calls=[]; let first=true;
mockIPC((command,args)=>{
 if(command==='onboarding_status')return {completed:true};
 if(command==='entry_snapshot')return {sequence:1,state,error:null,mode:'top',appearance:'sleeping',control:'resume',detail:null,notice:null};
 if(command!=='core_request')return null;
 const r=args.request;
 if(r.type==='reorganize_events') {
  calls.push(args);
  if(first){first=false;throw new Error('lost response');}
  return {type:'accepted',id:args.requestId,object_id:args.requestId};
 }
 if(r.type==='timeline')return {type:'timeline',gaps:[],items:[{id:'old',manual_range_id:null,title:'Existing meeting',summary:'Keep this while the model works.',started_ms:today+1000,ended_ms:today+5000,sources:'microphone',revision:1,processing_state:'transcribed',recording_sources:[],coarse_segments:[],precise_segments:[]}]};
 if(r.type==='calendar_days')return {type:r.type,days:[today]};
 if(r.type==='moments')return {type:r.type,moments:[]};
 if(r.type==='cloud_status')return {type:r.type,queues:[]};
 if(r.type==='people')return {type:r.type,people:[]};
 if(r.type==='models')return {type:r.type,state:{phase:'ready',speaker_phase:'ready',progress_percent:100}};
 return {type:r.type,state,items:[],tasks:[]};
},{shouldMockEvents:true});
const root=createRoot(document.getElementById('root'));
const settle=async(fn=()=>{})=>act(async()=>{await fn();await new Promise(r=>setTimeout(r,0));});
const button=pattern=>[...document.querySelectorAll('button')].find(b=>pattern.test(b.textContent));
try {
 await settle(()=>root.render(React.createElement(App)));
 await settle(()=>button(/\u65b0\u5efa\u4e8b\u4ef6/).click());
 const title=document.querySelector('dialog input');
 await settle(()=>{Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set.call(title,'Product meeting');title.dispatchEvent(new Event('input',{bubbles:true}));});
 await settle(()=>button(/^\u4fdd\u5b58$/).click());
 assert.equal(calls.length,1);
 assert(document.querySelector('dialog[open]'),'lost receipt keeps original form');
 await settle(()=>button(/^\u4fdd\u5b58$/).click());
 assert.equal(calls.length,2);
 assert.equal(calls[0].requestId,calls[1].requestId,'retry confirms original submission');
 assert.deepEqual(calls[0].request,calls[1].request);
 assert.equal(calls[1].request.type,'reorganize_events');
 assert.equal(calls[1].request.title,'Product meeting');
 assert.equal(calls[1].request.sources,'both');
 assert(!document.querySelector('dialog[open]'),'accepted task closes unchanged form');
 assert(document.body.textContent.includes('Existing meeting'),'accepted task retains existing cards');
 assert(document.body.textContent.includes('\u5df2\u63d0\u4ea4'),'receipt says submitted, not event completed');
 console.log('event reorganization: real App form, accepted task, stable retry ID and retained cards PASS');
} finally {await settle(()=>root.unmount());clearMocks();dom.window.close();}
const { renderToStaticMarkup } = await import('react-dom/server');
const { QueueView } = await import('../src/WorkspaceViews.tsx');
for (const [sources,label] of [['microphone','\u9ea6\u514b\u98ce'],['system','\u7535\u8111\u58f0\u97f3'],['both','\u53cc\u8def\u58f0\u97f3']]) {
 const html=renderToStaticMarkup(React.createElement(QueueView,{queues:[],reviews:[],busy:false,onRefresh(){},onMutate(){},tasks:[{id:sources,provider:'llm',target_kind:'reorganization',target_id:'request',source:null,sources,started_ms:today+1000,ended_ms:today+5000,state:'failed',error_code:'reorganization_revision_conflict',manual:true}]}));
 assert(html.includes(label),sources+' source label');
 assert(html.includes('\u91cd\u8bd5'),'existing failed-task retry control');
}
console.log('queue reorganization source labels: microphone/system/both PASS');
