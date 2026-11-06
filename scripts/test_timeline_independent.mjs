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
const { adjacentRecordedDay } = await import('../src/timeline-view.ts');
const day = (y, m, d) => new Date(y, m - 1, d).getTime();
const today = day(2026, 11, 1), recent = day(2026, 10, 31), short = day(2026, 1, 1), long = day(2025, 12, 31), ancient = day(1900, 1, 1);
for (const value of [null, undefined, NaN, Infinity, 'bad', today, today + 1]) {
  assert.equal(adjacentRecordedDay(value, today, day(2026, 11, 2), 'older', today), null);
}
assert.equal(adjacentRecordedDay(recent + 123, today, day(2026, 11, 2), 'older', today), recent);
assert.equal(adjacentRecordedDay(today, recent, today, 'newer', today), today);
assert.equal(adjacentRecordedDay(today + 86400000, recent, today, 'newer', today), null);
const dst = spawnSync(process.execPath, ['--input-type=module', '-e', `
  import assert from 'node:assert/strict';
  import {adjacentRecordedDay} from './src/timeline-view.ts';
  for(const [year,month,date,hours] of [[2026,2,8,23],[2026,10,1,25]]) {
    const from=new Date(year,month,date).getTime(),to=new Date(year,month,date+1).getTime();
    assert.equal(to-from,hours*3600000);
    const previous=new Date(year,month,date-1).getTime();
    assert.equal(adjacentRecordedDay(from-1,from,to,'older',to),previous);
    assert.equal(adjacentRecordedDay(to,from,to,'newer',to),to);
  }
`], { cwd: fileURLToPath(new URL('..', import.meta.url)), env: { ...process.env, TZ: 'America/New_York' }, encoding: 'utf8' });
assert.equal(dst.status, 0, dst.stderr);

const { default: React, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
const { default: App } = await import('../src/App.tsx');
const days = [ancient, long, short, recent];
const state = { revision: 1, enabled_intent: true, paused_intent: true, storage_paused: false, microphone_enabled: true, system_enabled: true, capture_state: 'paused', microphone_phase: 'idle', system_phase: 'idle', microphone_recording: false, system_recording: false };
const requests = [];
let blockCalendar, blockMoments, failMoments = false;
let holdDay = null, deferred = [], failAdjacent = false, malformed = false, momentsOnly = false, empty = false;
mockIPC((command, args) => {
  if (command === 'onboarding_status') return { completed: true };
  if (command === 'entry_snapshot') return { sequence: 1, state, error: null, mode: 'top', appearance: 'sleeping', control: 'resume', detail: null, notice: null };
  if (command === 'privacy_status') return { microphone: 1, screen_and_system_audio: 1, tap_required: false, tap_ready: true };
  if (command === 'login_item_status') return 1;
  if (command !== 'core_request') return null;
  const r = args.request;
  requests.push(r);
  const available = empty ? [] : momentsOnly ? [today] : days;
  if (r.type === 'calendar_days' && blockCalendar) return new Promise(resolve => { blockCalendar.resolve=()=>resolve({type:r.type,days:[]}); });
  if (r.type === 'calendar_days') return { type: r.type, days: available.filter(at => at >= r.boundaries[0] && at < r.boundaries.at(-1)) };
  if (r.type === 'calendar_adjacent') {
    if (failAdjacent) return Promise.reject(new Error('synthetic availability failure'));
    const result = { type: r.type, newer_ms: available.find(at => at >= r.to_ms) ?? null, older_ms: available.findLast(at => at < r.from_ms) ?? null };
    if (malformed) return { ...result, newer_ms: fixedNow + 86400000, older_ms: r.from_ms };
    if (r.from_ms === holdDay) return new Promise(resolve => deferred.push(() => resolve(result)));
    return result;
  }
  if (r.type === 'timeline') return { type: r.type, gaps: [], items: !momentsOnly && available.includes(r.from_ms) ? [{ id: `fixture-${r.from_ms}`, manual_range_id: null, title: 'synthetic record', summary: null, started_ms: r.from_ms + 1000, ended_ms: r.from_ms + 5000, sources: 'microphone', revision: 1, processing_state: 'transcribed', recording_sources: [], coarse_segments: [], precise_segments: [] }] : [] };
  if (r.type === 'moments' && failMoments) throw Error('synthetic moments failure');
  if (r.type === 'moments' && blockMoments) return new Promise(resolve => {blockMoments.resolve=(moments=[])=>resolve({type:r.type,moments});});
  if (r.type === 'moments') return { type: r.type, moments: momentsOnly && r.from_ms === today ? [{ id: 'point', kind: 'mark', anchor_ms: today + 1000 }] : [] };
  if (r.type === 'cloud_status') return { type: r.type, queues: [] };
  if (r.type === 'people') return { type: r.type, people: [] };
  if (r.type === 'models') return { type: r.type, state: { phase: 'ready', speaker_phase: 'ready', progress_percent: 100 } };
  return { type: r.type, state, items: [], tasks: [] };
}, { shouldMockEvents: true });
const root = createRoot(document.getElementById('root'));
const settle = async (fn = () => {}) => act(async () => { await fn(); await new Promise(resolve => setTimeout(resolve, 0)); });
const q = selector => document.querySelector(selector);
const older = () => q('[aria-label="上一个有记录的日期"]');
const newer = () => q('[aria-label="下一个有记录的日期"]');
const selected = () => q('.date-choice')?.dataset.date;
const until = async check => { for (let i = 0; i < 100; i++) { if (check()) return; await settle(); } throw Error('synthetic navigation did not settle'); };
const click = async button => { assert(button && !button.disabled); await settle(() => button.click()); };
try {
 days.push(today);blockCalendar={};blockMoments={};
 await settle(()=>root.render(React.createElement(App)));
 await until(()=>q('.event-card'));
 assert(blockCalendar.resolve,'calendar is still pending');
 await settle(()=>{blockCalendar.resolve();blockCalendar=null;});
 await until(()=>blockMoments.resolve);
 assert(q('.event-card'),'pending moments cannot hide timeline');
 await settle(()=>{blockMoments.resolve();blockMoments=null;});
 failMoments=false;blockMoments={};
 currentNow+=300;
 await settle(()=>window.dispatchEvent(new Event('focus')));
 await until(()=>blockMoments.resolve);
 const releaseOld=blockMoments.resolve;blockMoments=null;
 // Calendar's older-day control remains usable while old moments are pending.
 await click(older());await until(()=>selected()==='2026-10-31');
 requests.length=0;
 await settle(()=>releaseOld(Array.from({length:200},(_,i)=>({id:`old-${i}`,kind:'mark',anchor_ms:today+1000+i}))));
 assert(!requests.some(r=>r.type==='moments'&&r.from_ms===today&&r.offset===200),'stale moments must not fetch another page');
 currentNow+=300;
 failMoments=true;
 await settle(()=>window.dispatchEvent(new Event('focus')));
 await until(()=>document.body.textContent.includes('现场记录读取失败'));
 assert(q('.event-card'),'failed moments cannot hide successful timeline');
 console.log('PASS: stale moments stop paging; timeline renders before calendar/moments and survives auxiliary failure');
} finally {await settle(()=>root.unmount());clearMocks();dom.window.close();}
