// Real DetailDrawer/React with mocked Core reads and IntersectionObserver delivery.
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { JSDOM } from 'jsdom';

const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost/', pretendToBeVisual: true });
for (const name of ['window', 'document', 'HTMLElement', 'HTMLDialogElement', 'HTMLButtonElement', 'Event', 'MouseEvent', 'KeyboardEvent', 'Node', 'Element', 'HTMLInputElement', 'HTMLLabelElement']) globalThis[name] = dom.window[name];
HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.ResizeObserver=class{observe(){}disconnect(){}};
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
    if (url.endsWith('/playback.ts')) return {format:'module',shortCircuit:true,source:`export class ContinuousPlayback { constructor(_read, tick){globalThis.playbackTick=tick} start(){} stop(){} dispose(){} }`};
    if (url.endsWith('/detail-transcript.ts')) return {format:'module',shortCircuit:true,source:ts.transpileModule(readFileSync(fileURLToPath(url),'utf8').replace('const precise = detail.precise_segments;', 'const precise = detail.precise_segments;'), {compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace('export function bestSegments(detail, more) {','export function bestSegments(detail, more) { globalThis.transcriptComputations = (globalThis.transcriptComputations || 0) + 1;')};

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

const initial = {id:'performance-fixture',title:'Long synthetic event',started_ms:1000,ended_ms:200000,
 sources:'microphone',revision:1,processing_state:'partial_asr_failed',coarse_segments:[],precise_segments:[]};
let version='run:0:1', refresh=0, requests=[], holdSpeakers=false, speakerReplies=[];
const rows=Array.from({length:90},(_,i)=>({id:`s${i}`,source:'microphone',started_ms:1000+i*1000,ended_ms:1900+i*1000,text:`original ${i}`,speaker:null,revision:1,speaker_revision:1,person_name:null,person_id:null,corrected:false}));
mockIPC(async(command,args)=>{
 const r=args.request; requests.push(r);
 if(r.type==='event_detail')return {type:'event',snapshot_version:version,event:{...initial,precise_segments:rows.slice(r.offset,r.offset+r.limit)}};
 if(r.type==='moments')return {type:'moments',moments:[]};
 if(r.type==='session_speakers' && holdSpeakers)return new Promise(resolve=>speakerReplies.push(()=>resolve({type:'session_speakers',speakers:[]})));
 if(r.type==='session_speakers')return {type:'session_speakers',speakers:[]};
 throw Error('unexpected '+r.type);
});
const root=createRoot(document.getElementById('root'));
const settle=async(fn=()=>{})=>act(async()=>{await fn();await new Promise(r=>setTimeout(r,0));});
const noop=()=>{};
const render=()=>root.render(React.createElement(DetailDrawer,{item:initial,people:[],refreshToken:refresh,onClose:noop,onEdit:noop,onTools:noop,onQueue:noop,onDataAction:noop,onChanged:noop,onMessage:noop}));
const advance=()=>{for(const observer of [...observers])observer.callback([{isIntersecting:true}]);};
const count=()=>document.querySelectorAll('.transcript-row').length;
try {
 holdSpeakers=true;
 await settle(render);assert.equal(count(),29);assert.equal(speakerReplies.length,1);
 refresh++;await settle(render);assert.equal(speakerReplies.length,2,'refresh must not discard the unfinished identity list');
 await settle(()=>speakerReplies.shift()());
 assert.equal(requests.filter(r=>r.type==='session_speakers').length,2);
 holdSpeakers=false;await settle(()=>speakerReplies.shift()());
 await settle(()=>document.querySelector('.speaker-chip').click());
 assert(document.querySelector('.speaker-picker input') && !document.querySelector('.speaker-picker input').disabled,'identity picker becomes usable');
 await settle(()=>document.querySelector('.speaker-picker input').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})));

 await settle(advance);assert.equal(count(),59);
 await settle(advance);assert.equal(count(),89);
 await settle(advance);assert.equal(count(),90);
 const computed=globalThis.transcriptComputations;
 const content=document.querySelector('.transcript-content');
 for(let i=0;i<10;i++)await settle(()=>globalThis.playbackTick(1000+i*100));
 assert.equal(globalThis.transcriptComputations,computed,'playback ticks do not recompute transcript');
 assert.equal(document.querySelector('.transcript-content'),content);
 requests=[];refresh++;await settle(render);
 assert.deepEqual(requests.filter(r=>r.type==='event_detail').map(r=>r.offset),[0]);
 assert.equal(count(),90,'same snapshot retains all loaded pages');
 rows[75]={...rows[75],text:'remote correction',revision:2};version='run:0:2';
 requests=[];refresh++;await settle(render);
 assert.deepEqual(requests.filter(r=>r.type==='event_detail').map(r=>r.offset),[0,30,60,90]);
 assert(document.body.textContent.includes('remote correction'),'same event revision must still see corrected later pages');
 version=undefined;requests=[];refresh++;await settle(render);
 assert.deepEqual(requests.filter(r=>r.type==='event_detail').map(r=>r.offset),[0,30,60,90]);
 console.log('PASS: playback keeps static transcript, unchanged snapshot skips deep reload, remote corrections and absent versions reload');
} finally {await settle(()=>root.unmount());clearMocks();dom.window.close();}
