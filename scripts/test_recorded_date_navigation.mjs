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
for (const name of ['window', 'document', 'Node', 'HTMLElement', 'HTMLTextAreaElement', 'Event', 'MouseEvent', 'KeyboardEvent', 'MutationObserver']) globalThis[name] = dom.window[name];
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
  if (r.type === 'calendar_days') return { type: r.type, days: available.filter(at => at >= r.boundaries[0] && at < r.boundaries.at(-1)) };
  if (r.type === 'calendar_adjacent') {
    if (failAdjacent) return Promise.reject(new Error('synthetic availability failure'));
    const result = { type: r.type, newer_ms: available.find(at => at >= r.to_ms) ?? null, older_ms: available.findLast(at => at < r.from_ms) ?? null };
    if (malformed) return { ...result, newer_ms: fixedNow + 86400000, older_ms: r.from_ms };
    if (r.from_ms === holdDay) return new Promise(resolve => deferred.push(() => resolve(result)));
    return result;
  }
  if (r.type === 'timeline') return { type: r.type, gaps: [], items: !momentsOnly && available.includes(r.from_ms) ? [{ id: `fixture-${r.from_ms}`, manual_range_id: null, title: 'synthetic record', summary: null, started_ms: r.from_ms + 1000, ended_ms: r.from_ms + 5000, sources: 'microphone', revision: 1, processing_state: 'transcribed', recording_sources: [], coarse_segments: [], precise_segments: [] }] : [] };
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
  await settle(() => root.render(React.createElement(App)));
  await until(() => selected() === '2026-10-31' && !older().disabled);
  assert(newer().disabled, 'most recent recorded day is the newer boundary even before today');
  assert.equal(newer().getAttribute('data-tooltip'), null, 'disabled date buttons have no tooltip');
  assert.equal(older().getAttribute('data-tooltip'), '前一天');
  assert.equal(q('.date-step-controls button'), newer(), 'left arrow moves newer');
  await click(q('.date-today'));
  await until(() => selected() === '2026-11-01' && !older().disabled);
  assert(q('.list-empty'), 'today remains directly selectable without records');
  const dateTrigger = q('.date-choice');
  const previousMonth = () => q('.calendar-popover [aria-label="上个月"]');
  const nextMonth = () => q('.calendar-popover [aria-label="下个月"]');
  await click(dateTrigger);
  assert.equal(document.activeElement, previousMonth());
  await click(previousMonth());
  await click(nextMonth());
  assert(nextMonth().disabled);
  assert.equal(document.activeElement, previousMonth(), 'reaching the current month keeps focus on a usable month control');
  await settle(() => dateTrigger.focus());
  await settle(() => dateTrigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
  assert(!q('.calendar-popover'), 'Escape on the trigger dismisses the open calendar');
  assert.equal(document.activeElement, dateTrigger);
  await click(dateTrigger);
  const createEvent = q('.create-event');
  await settle(() => createEvent.focus());
  assert(!q('.calendar-popover'), 'keyboard focus leaving the date controls dismisses the calendar');
  assert.equal(document.activeElement, createEvent, 'outside dismissal does not move focus back');
  await click(dateTrigger);
  await settle(() => createEvent.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true })));
  assert(!q('.calendar-popover'), 'outside pointer dismissal still works');
  await click(older());
  await until(() => selected() === '2026-10-31' && !older().disabled);
  const anchor = q('.date-choice');
  holdDay = short;
  await click(older());
  await until(() => selected() === '2026-01-01' && deferred.length > 0);
  assert.equal(q('.date-choice'), anchor, 'date trigger keeps stable DOM identity');
  assert(older().disabled && newer().disabled, 'unknown neighbors cannot be guessed while loading');
  await click(q('.date-today'));
  await until(() => selected() === '2026-11-01' && !older().disabled);
  holdDay = null;
  await settle(() => { for (const resolve of deferred.splice(0)) resolve(); });
  assert.equal(selected(), '2026-11-01', 'late availability response cannot navigate or replace current neighbors');
  await click(older());
  await until(() => selected() === '2026-10-31' && !older().disabled);
  failAdjacent = true;
  await settle(() => { currentNow += 300; window.dispatchEvent(new Event('focus')); });
  await until(() => older().disabled && newer().disabled);
  assert.equal(selected(), '2026-10-31', 'failed availability read cannot move the date');
  failAdjacent = false;
  malformed = true;
  await settle(() => { currentNow += 300; window.dispatchEvent(new Event('focus')); });
  await until(() => !q('[aria-label="正在加载当天记录"]'));
  assert(older().disabled && newer().disabled, 'wrong-direction and future anchors are rejected');
  malformed = false;
  await settle(() => { currentNow += 300; window.dispatchEvent(new Event('focus')); });
  await until(() => !older().disabled);
  for (const expected of ['2026-01-01', '2025-12-31', '1900-01-01']) {
    await click(older());
    await until(() => selected() === expected && (expected === '1900-01-01' ? !newer().disabled : !older().disabled));
  }
  assert(older().disabled, 'earliest recorded day is disabled');
  assert.equal(older().getAttribute('data-tooltip'), null);
  assert.equal(newer().getAttribute('data-tooltip'), '后一天');
  await click(newer());
  await until(() => selected() === '2025-12-31' && !newer().disabled);
  assert(requests.filter(r => r.type === 'calendar_days').length < 25, 'navigation does not enumerate historical months');
  momentsOnly = true;
  await settle(() => root.render(React.createElement(App, { key: 'moments-only' })));
  await until(() => q('.day-map-moment'));
  assert.equal(selected(), '2026-11-01', 'a moment-only today does not jump away on first open');
  assert(older().disabled && newer().disabled);
  momentsOnly = false;
  empty = true;
  await settle(() => root.render(React.createElement(App, { key: 'empty' })));
  await until(() => q('.list-empty'));
  assert.equal(selected(), '2026-11-01');
  assert(older().disabled && newer().disabled, 'an empty history has no adjacent navigation');
  console.log('recorded date navigation: history, boundaries, DST, direct today, stable trigger, failed/malformed/stale replies and moment-only startup passed');
} finally {
  await settle(() => root.unmount());
  clearMocks();
  dom.window.close();
  globalThis.Date = NativeDate;
}
