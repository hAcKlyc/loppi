// Real App with synthetic IPC only: availability navigation and stale responses.
import assert from 'node:assert/strict';
import { registerHooks, createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import ts from 'typescript';

const requireTest = createRequire(import.meta.url);
const { JSDOM } = requireTest('jsdom');
const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost/', pretendToBeVisual: true });
for (const name of ['window', 'document', 'HTMLElement', 'HTMLTextAreaElement', 'Event', 'MouseEvent', 'KeyboardEvent', 'MutationObserver']) globalThis[name] = dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.ResizeObserver = class { observe() {} disconnect() {} };
window.isTauri = true;
globalThis.isTauri = true;
globalThis.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
globalThis.cancelAnimationFrame = dom.window.cancelAnimationFrame.bind(dom.window);
const NativeDate = Date;
const fixedNow = new NativeDate(2026, 10, 1, 12).getTime();
let currentNow = fixedNow;
globalThis.Date = window.Date = class extends NativeDate {
  constructor(...args) { super(...(args.length ? args : [currentNow])); }
  static now() { return currentNow; }
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
    if (url.endsWith('/Rabbit.tsx')) return { format: 'module', source: 'export function Rabbit(){return null}', shortCircuit: true };
    if (/\.(css|png|svg)$/.test(url)) return { format: 'module', source: 'export default "fixture-asset"', shortCircuit: true };
    if (/\.tsx?$/.test(url)) return { format: 'module', shortCircuit: true, source: ts.transpileModule(readFileSync(fileURLToPath(url), 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, jsx: ts.JsxEmit.ReactJSX } }).outputText };
    return next(url, context);
  },
});
const { default: React, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
const { default: App } = await import('../src/App.tsx');
const state = { revision: 1, enabled_intent: false, paused_intent: true, microphone_enabled: false, system_enabled: false, capture_state: 'paused', microphone_phase: 'idle', system_phase: 'idle' };
let failure = false, hold = false, pending = [];
const timers = [];
window.setInterval = fn => { timers.push(fn); return timers.length; };
window.clearInterval = () => {};
document.hasFocus = () => true;
mockIPC((command, args) => {
  if (command === 'onboarding_status') return {completed:true};
  if (command === 'entry_snapshot') return {sequence:1,state,error:null,mode:'top',appearance:'sleeping',control:'resume'};
  const kind = command === 'core_request' ? args.request.type : command;
  const values = {privacy_status:{microphone:1,screen_and_system_audio:1,tap_required:false,tap_ready:true},login_item_status:1,models:{type:'models',state:{phase:'ready',speaker_phase:'ready',progress_percent:100}},cloud_status:{type:'cloud_status',queues:[]},people:{type:'people',people:[]},timeline:{type:'timeline',items:[],gaps:[]},calendar_days:{type:'calendar_days',days:[]},calendar_adjacent:{type:'calendar_adjacent',newer_ms:null,older_ms:null},moments:{type:'moments',moments:[]},status:{type:'status',state}};
  if (['models','login_item_status'].includes(kind)) {
    const finish = () => failure ? Promise.reject(new Error('synthetic read failure')) : Promise.resolve(values[kind]);
    if (hold) return new Promise((resolve,reject)=>pending.push({kind,finish:()=>finish().then(resolve,reject)}));
    return finish();
  }
  return values[kind] ?? null;
});
const root = createRoot(document.getElementById('root'));
const flush = async () => { await act(async()=>{await new Promise(r=>setTimeout(r,25));}); };
const tick = async () => { await act(async()=>{for(const fn of timers)fn();}); await flush(); };
try {
  await act(async()=>root.render(React.createElement(App)));
  await flush();
  const settings = [...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='设置');
  assert.ok(settings);
  await act(async()=>settings.click()); await flush();
  const toggle = document.querySelector('[role="switch"][aria-label="登录时启动"]');
  assert.ok(toggle);
  failure=true; await tick();
  assert.match(document.body.textContent,/登录时启动读取失败/);
  assert.match(document.body.textContent,/本地组件读取失败/);
  const alert = [...document.querySelectorAll('[role="alert"]')].find(el=>el.textContent.includes('登录时启动读取失败'));
  hold=true; await tick();
  assert.ok(alert.isConnected,'error must stay mounted during refresh');
  assert.equal(document.querySelector('[role="switch"][aria-label="登录时启动"]'),toggle);
  await act(async()=>{for(const item of pending.splice(0))await item.finish();});
  assert.ok(alert.isConnected,'repeated failure must not remove the error');
  failure=false; await tick();
  const login = pending.find(x=>x.kind==='login_item_status');
  await act(async()=>login.finish()); await flush();
  assert.doesNotMatch(document.body.textContent,/登录时启动读取失败/);
  assert.match(document.body.textContent,/本地组件读取失败/,'one success cannot clear a different resource error');
  await act(async()=>{for(const item of pending.splice(0).filter(x=>x!==login))await item.finish();});
  assert.doesNotMatch(document.body.textContent,/本地组件读取失败/);
  console.log('PASS settings preserve controls/errors through pending refresh and repeated failure; clear per-resource on success');
} finally { await act(async()=>root.unmount()); clearMocks(); dom.window.close(); }
