// Real DetailDrawer/React with mocked Core reads and IntersectionObserver delivery.
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { JSDOM } from 'jsdom';

const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost/', pretendToBeVisual: true });
for (const name of ['window', 'document', 'HTMLElement', 'HTMLDialogElement', 'HTMLButtonElement', 'Event', 'MouseEvent', 'KeyboardEvent']) globalThis[name] = dom.window[name];
HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.isTauri = true;
const observers = new Set();
globalThis.IntersectionObserver = class {
  constructor(callback, options) { this.callback = callback; this.options = options; }
  observe(target) { this.target = target; observers.add(this); }
  disconnect() { observers.delete(this); }
};
registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('.') && context.parentURL?.includes('/src/')) {
      for (const suffix of ['.ts', '.tsx']) {
        const url = new URL(specifier + suffix, context.parentURL);
        if (existsSync(url)) return next(url.href, context);
      }
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (/\.tsx?$/.test(url)) return { format: 'module', shortCircuit: true, source: ts.transpileModule(readFileSync(fileURLToPath(url), 'utf8'), {
      compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, jsx: ts.JsxEmit.ReactJSX },
    }).outputText };
    return next(url, context);
  },
});
const { default: React, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
const { DetailDrawer } = await import('../src/DetailDrawer.tsx');

const started = Date.now()-3600000+123;
const initial = { id:'completion-fixture', title:'结束确认回归', started_ms:started, ended_ms:started+1200000,
  sources:'microphone', revision:7, processing_state:'settling_asr', coarse_segments:[], precise_segments:[],
  workflow:{phase:'undetermined',tag_state:'tagged',labels:[],manual:false} };
let current={...initial}, key=0, failOnce=false, mutations=[], actions=[], retranscriptions=[];
mockIPC(async(command,args)=>{
 const r=args.request;
 if(r.type==='event_detail')return {type:'event',event:current};
 if(r.type==='moments')return {type:'moments',moments:[]};
 if(r.type==='retranscribe_event'){
  retranscriptions.push(r);
  current={...current,processing_state:'pending_asr'};
  return {type:'accepted',id:args.requestId,object_id:r.event_id};
 }
 if(r.type==='confirm_event_completion'){
  mutations.push(args);
  if(failOnce){failOnce=false;throw Error('lost response');}
  const changed=r.started_ms!==initial.started_ms || r.ended_ms!==initial.ended_ms;
  current={...current,completed_ms:changed?null:Date.now(),completion_state:changed?'pending':undefined,processing_state:changed?'settling_asr':'pending_asr',revision:changed?7:8,workflow:{...initial.workflow,phase:changed?'judging':'decided'}};
  return {type:'accepted',id:args.requestId,object_id:r.event_id};
 }
 throw Error('unexpected '+r.type);
});
const root=createRoot(document.getElementById('root'));
const settle=async(fn=()=>{})=>act(async()=>{await fn();await new Promise(r=>setTimeout(r,0));});
const q=s=>document.querySelector(s);
const button=text=>[...document.querySelectorAll('button')].find(el=>el.textContent===text);
const render=()=>root.render(React.createElement(DetailDrawer,{key,item:current,people:[],refreshToken:0,
 onClose(){},onEdit(){actions.push('edit')},onTools(_item,mode){actions.push(mode)},onQueue(){actions.push('queue')},
 onDataAction(kind){actions.push(kind)},onChanged(){},onConfirmedEvent(){},onMessage(){}}));
const reset=async()=>{current={...initial};key++;await settle(render);};
const open=async()=>settle(()=>q('.event-completion-trigger').click());
const setValue=async(input,value)=>settle(()=>{
 Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,value);
 input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));
});
globalThis.HTMLInputElement=window.HTMLInputElement;
try{
 await settle(render);
 assert(q('.detail-header .event-completion-trigger'));
 assert(!q('[aria-label="执行策略"]'),'strategy entry waits for event determination and rule evaluation');
 assert(!q('.detail-header-actions > .control-button:not([aria-label])'),'management actions are inside more');
 await open();
 let inputs=[...document.querySelectorAll('.modal input')];
 assert.equal(inputs.length,2);assert.equal(inputs[0].step,'1');
 await settle(()=>button('取消').click());assert.equal(mutations.length,0);
 assert.equal(document.activeElement,q('.event-completion-trigger'),'cancel restores trigger focus');
 await open();await settle(()=>button('确认结束并开始处理').click());
 assert.equal(mutations.length,1);assert.equal(mutations[0].request.started_ms,initial.started_ms);
 assert.equal(mutations[0].request.ended_ms,initial.ended_ms,'unchanged defaults preserve millisecond identity');
 assert.match(q('.detail-status').textContent,/已确认结束.*待精转/);
 assert(!q('.event-completion-trigger'));assert(!q('.modal'));
 assert(q('[aria-label="执行策略"]'),'strategy entry is available while queued ASR is still pending');
 assert(button('生成内容'));
 await reset();await open();
 inputs=[...document.querySelectorAll('.modal input')];
 await setValue(inputs[1],inputs[0].value);
 await settle(()=>button('确认结束并开始处理').click());
 assert.match(q('.dialog-feedback').textContent,/有效的起止时间/);assert.equal(mutations.length,1);
 await settle(()=>button('取消').click());
 await open();inputs=[...document.querySelectorAll('.modal input')];
 const shifted=new Date(initial.ended_ms+60000), pad=x=>String(x).padStart(2,'0');
 const value=`${shifted.getFullYear()}-${pad(shifted.getMonth()+1)}-${pad(shifted.getDate())}T${pad(shifted.getHours())}:${pad(shifted.getMinutes())}:${pad(shifted.getSeconds())}`;
 await setValue(inputs[1],value);failOnce=true;
 await settle(()=>button('确认结束并开始处理').click());
 assert(button('确认上次提交'));assert([...document.querySelectorAll('.modal input')].every(i=>i.disabled));
 const uncertain=mutations.at(-1);assert.notEqual(uncertain.request.ended_ms,initial.ended_ms);
 await settle(()=>button('确认上次提交').click());
 assert.equal(mutations.at(-1).requestId,uncertain.requestId,'uncertain retries reuse receipt identity');
 assert.deepEqual(mutations.at(-1).request,uncertain.request);
 assert.match(q('.detail-status').textContent,/重新整理中/);assert(!q('.event-completion-trigger'));
 await settle(()=>q('.detail-status-link').click());assert.equal(actions.at(-1),'queue');
 await reset();
 assert.deepEqual([...document.querySelectorAll('.detail-menu-items button')].map(el=>el.textContent),['编辑','调整分段','导出','清理数据']);
 for(const [label,action] of [['编辑','edit'],['调整分段','split'],['导出','export'],['清理数据','cleanup']]){
  await settle(()=>q('.detail-menu summary').click());assert(q('.detail-menu').open);
  await settle(()=>button(label).click());assert.equal(actions.at(-1),action);assert(!q('.detail-menu').open);
 }
 await settle(()=>q('.detail-menu summary').click());
 await settle(()=>q('.detail-menu summary').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})));
 assert(!q('.detail-menu').open);assert.equal(document.activeElement,q('.detail-menu summary'));
 current={...initial,completion_state:'failed'};await settle(render);
 assert.match(q('.detail-status').textContent,/重新整理失败/);
 await settle(()=>q('.detail-status-link').click());assert.equal(actions.at(-1),'queue');
 for(const processing_state of ['pending_asr','transcribing','asr_failed','partial_asr_failed']){
  current={...initial,processing_state,completed_ms:Date.now()};await settle(render);
  assert(!q('.event-completion-trigger'));
  await settle(()=>q('.detail-status-link').click());assert.equal(actions.at(-1),'queue');
 }
 current={...initial,processing_state:'transcribed',completed_ms:Date.now()};await settle(render);
 assert(!q('.detail-status-link'),'completed processing is a plain status');
 await settle(()=>button('音频信息').click());
 const rerun=button('重新识别录音');assert(rerun.closest('.information-panel'));
 await settle(()=>rerun.click());
 assert.match(q('.modal').textContent,/产生新的识别用量/);assert.equal(retranscriptions.length,0);
 await settle(()=>button('取消').click());assert.equal(retranscriptions.length,0);
 assert.equal(document.activeElement,rerun,'cancel returns to the audio information action');
 await settle(()=>rerun.click());await settle(()=>button('确认重新识别').click());
 assert.deepEqual(retranscriptions,[{type:'retranscribe_event',event_id:initial.id,expected_revision:initial.revision}]);
 assert(!q('.modal'));assert.match(q('.detail-status-link').textContent,/待精转/);
 console.log('PASS: four-action menu, completion and queue status navigation, audio-only rerun with usage confirmation, default precision, validation and stable uncertain receipt');
}finally{await settle(()=>root.unmount());clearMocks();dom.window.close();}
