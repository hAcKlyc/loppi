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
  workflow:{phase:'undetermined',tag_state:'tagged',labels:[{id:'work',name:'工作会议'}],manual:false} };
let current={...initial}, key=0, failOnce=false, failRunsOnce=false, staleTitleOnce=false, extraSuccess=false, longBody=false, refreshMarker=0, mutations=[], actions=[], retranscriptions=[], resultReads=[], titleEdits=[];
const strategyRun=(id,name)=>({run_id:id+'-run',strategy_id:id,name,input:'precise',
  state:'succeeded',result_version_id:id+'-run',created_ms:started,completed_ms:started,asr_task_ids:[]});
mockIPC(async(command,args)=>{
 const r=args.request;
 if(r.type==='event_detail')return {type:'event',event:current};
 if(r.type==='moments')return {type:'moments',moments:[]};
 if(r.type==='edit_event'){
  titleEdits.push(r);
  if(staleTitleOnce){
   staleTitleOnce=false;current={...current,title:'其他入口保存的标题',revision:current.revision+1};
   return {type:'error',code:'event_invalid',message:'event revision conflict'};
  }
  current={...current,title:r.title,revision:current.revision+1};
  return {type:'accepted',id:args.requestId};
 }
 if(r.type==='workflow'){
  if(r.action.op==='runs'&&failRunsOnce){failRunsOnce=false;throw Error('模拟策略读取失败');}
  if(r.action.op==='runs'){
   const minutes=strategyRun('minutes','会议纪要');
   const runs=[minutes,strategyRun('actions','行动清单')];
   if(extraSuccess)runs.unshift({...minutes,run_id:'earlier-body',result_version_id:'earlier-body',created_ms:started+1,completed_ms:started-1});
   return {type:'workflow',result:{ok:true,data:{runs}}};
  }
  if(r.action.op==='result'){
   resultReads.push(r.action.strategy_id);
   return {type:'workflow',result:{ok:true,data:{...strategyRun(r.action.strategy_id,'策略'),body_markdown:r.action.cursor?'\n\n## 续页保留':r.action.strategy_id==='minutes'?'# 合成纪要\n\n## 决策\n\n- 周四交付':'# 合成行动清单',next_cursor:longBody&&!r.action.cursor?'minutes-run:12000':null}}};
  }
 }

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
const render=()=>root.render(React.createElement(DetailDrawer,{key,item:current,people:[],refreshToken:refreshMarker,
 onClose(){actions.push('close')},onEdit(){actions.push('edit')},onTools(_item,mode){actions.push(mode)},onQueue(){actions.push('queue')},
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
 assert(button('会议纪要'));assert(button('行动清单'));assert(!button('生成内容'));
 assert.equal(resultReads.length,0,'strategy tabs load metadata without loading markdown');
 await settle(()=>button('会议纪要').click());
 assert.equal(q('.workflow-markdown h1').textContent,'合成纪要');
 assert.equal(q('.workflow-markdown h2').textContent,'决策');
 assert.doesNotMatch(q('.workflow-results').textContent,/已生成|基于精转录/);
 await settle(()=>button('行动清单').click());
 assert.equal(q('.workflow-markdown h1').textContent,'合成行动清单');
 assert.deepEqual(resultReads,['minutes','actions']);


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
 await reset();failRunsOnce=true;await open();await settle(()=>button('确认结束并开始处理').click());
 assert.match(q('.workflow-tab-error').textContent,/模拟策略读取失败/);
 assert(!button('会议纪要'));
 await settle(()=>button('重试读取策略').click());
 assert(button('会议纪要'));assert(!q('.workflow-tab-error'));
 await reset();
 assert.equal(q('.detail-meta .detail-tag').textContent,'工作会议');
 assert(q('.detail-meta .eyebrow')&&q('.detail-meta .detail-status'));
 assert(!button('编辑标签'));
 await settle(()=>q('.detail-title-trigger').click());
 assert.equal(document.activeElement,q('.detail-title-input'));
 const actionsBeforeIme=actions.length;
 await settle(()=>q('.detail-title-input').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,isComposing:true})));
 assert(q('.detail-title-input'));assert.equal(actions.length,actionsBeforeIme);assert.equal(titleEdits.length,0);
 await setValue(q('.detail-title-input'),'取消的标题');
 await settle(()=>q('.detail-title-input').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})));
 assert.equal(titleEdits.length,0);
 assert.equal(q('.detail-title-trigger').textContent,initial.title);
 await settle(()=>q('.detail-title-trigger').click());
 await setValue(q('.detail-title-input'),'  内联修改后的标题  ');
 await settle(()=>q('.detail-title-input').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true})));
 assert.deepEqual(titleEdits,[{type:'edit_event',event_id:initial.id,expected_revision:initial.revision,
  started_ms:initial.started_ms,ended_ms:initial.ended_ms,sources:initial.sources,title:'内联修改后的标题'}]);
 assert.equal(q('.detail-title-trigger').textContent,'内联修改后的标题');
 assert.equal(document.activeElement,q('.detail-title-trigger'));
 staleTitleOnce=true;
 await settle(()=>q('.detail-title-trigger').click());
 await setValue(q('.detail-title-input'),'过期的编辑');
 await settle(()=>q('.detail-title-input').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true})));
 assert.equal(titleEdits.length,2);assert(!q('.detail-title-input'));
 assert.equal(q('.detail-title-trigger').textContent,'其他入口保存的标题');
 assert.equal(document.activeElement,q('.detail-title-trigger'));

 await reset();extraSuccess=true;longBody=true;await open();await settle(()=>button('确认结束并开始处理').click());
 await settle(()=>button('会议纪要').click());
 await settle(()=>button('继续阅读').click());
 assert.match(q('.workflow-markdown').textContent,/续页保留/);
 extraSuccess=false;refreshMarker++;await settle(render);
 assert.match(q('.workflow-markdown').textContent,/续页保留/,'metadata refresh preserves already-loaded pages of the same version');
}finally{await settle(()=>root.unmount());clearMocks();dom.window.close();}
