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
const today=dayBounds(new Date())[0];
const state={revision:1,enabled_intent:true,paused_intent:true,storage_paused:false,microphone_enabled:true,system_enabled:true,capture_state:'paused',microphone_phase:'idle',system_phase:'idle',microphone_recording:false,system_recording:false};
const pending=[]; const requests=[];
let active=0,maxActive=0;
mockIPC((command,args)=>{
 if(command==='onboarding_status')return {completed:true};
 if(command==='entry_snapshot')return {sequence:1,state,error:null,mode:'top',appearance:'sleeping',control:'resume',detail:null,notice:null};
 if(command==='privacy_status')return {microphone:1,screen_and_system_audio:1,tap_required:false,tap_ready:true};
 if(command==='login_item_status')return 1;
 if(command!=='core_request')return null;
 const r=args.request;
 if(r.type==='timeline') {
  requests.push(r); active++; maxActive=Math.max(maxActive,active);
  return new Promise(resolve=>pending.push(title=>{active--;resolve({type:'timeline',gaps:[],items:[{id:'fixture',manual_range_id:null,title,summary:null,started_ms:r.from_ms+1000,ended_ms:r.from_ms+5000,sources:'microphone',revision:1,processing_state:'transcribed',recording_sources:[],coarse_segments:[],precise_segments:[]}]});}));
 }
 if(r.type==='calendar_days')return {type:r.type,days:[today]};
 if(r.type==='moments')return {type:r.type,moments:[]};
 if(r.type==='cloud_status')return {type:r.type,queues:[]};
 if(r.type==='people')return {type:r.type,people:[]};
 if(r.type==='models')return {type:r.type,state:{phase:'ready',speaker_phase:'ready',progress_percent:100}};
 return {type:r.type,state,items:[],tasks:[]};
},{shouldMockEvents:true});
const root=createRoot(document.getElementById('root'));
const settle=async(fn=()=>{})=>act(async()=>{await fn();await new Promise(r=>setTimeout(r,0));});
try {
 await settle(()=>root.render(React.createElement(App)));
 assert.equal(requests.length,1,'initial status refresh joins the in-flight date read');
 await settle(()=>window.dispatchEvent(new Event('focus')));
 assert.equal(requests.length,1,'focus does not duplicate a same-day read');
 await settle(()=>pending.shift()('discarded-initial-snapshot'));
 if(pending.length) await settle(()=>pending.shift()('current-snapshot'));
 assert.equal(maxActive,1);
 assert(document.body.textContent.includes('current-snapshot'),'post-status snapshot becomes visible');
 let staleWasShown=false;
 const observer=new MutationObserver(()=>{staleWasShown ||= document.body.textContent.includes('stale-after-mutation');});
 observer.observe(document.getElementById('root'),{subtree:true,childList:true,characterData:true});
 await settle(()=>emit('loppi-moment-changed'));
 const before=requests.length;
 await settle(()=>emit('loppi-moment-changed'));
 assert.equal(requests.length,before,'second mutation queues one trailing read');
 await settle(()=>pending.shift()('stale-after-mutation'));
 assert.equal(requests.length,before+1,'mutation obtains a fresh snapshot after current query');
 assert(!staleWasShown,'response captured before mutation never reaches the view');
 await settle(()=>pending.shift()('fresh-after-mutation'));
 assert(document.body.textContent.includes('fresh-after-mutation'));
 assert.equal(maxActive,1);
 observer.disconnect();
 console.log('timeline refresh: same-day coalescing, immediate stale-response invalidation and serial fresh read passed');
} finally {
 await settle(()=>root.unmount());
 clearMocks(); dom.window.close();
}
