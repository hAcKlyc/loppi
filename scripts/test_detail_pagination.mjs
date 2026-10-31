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
const pageSize = 30;
const from = 1_000_000;
const row = (prefix, index, width) => ({ id: `${prefix}-${index}`, source: 'microphone', started_ms: from + index * width, ended_ms: from + (index + 1) * width, text: `${prefix} ${index}` });
let coarse = Array.from({ length: 120 }, (_, i) => row('coarse', i, 20_000));
let precise = Array.from({ length: 465 }, (_, i) => ({ ...row('precise', i, 5_000), revision: 0 }));
const base = { id: 'fixture', title: '分页回归', started_ms: from, ended_ms: from + 2_400_000, sources: 'microphone', processing_state: 'partial_transcript', coarse_segments: [], precise_segments: [] };
let version = 0, key = 0, initialAt, failOffset = null, holdOffset = null, hold, release, calls = [];
mockIPC(async (command, { request }) => {
  if (request.type === 'event_detail') {
    assert.equal(request.limit, pageSize, 'preserve bounded IPC pages');
    calls.push(request.offset);
    if (request.offset === holdOffset) await hold;
    if (request.offset === failOffset) throw Error('simulated page failure');
    return { type: 'event', event: { ...base, coarse_segments: coarse.slice(request.offset, request.offset + request.limit), precise_segments: precise.slice(request.offset, request.offset + request.limit) } };
  }
  if (request.type === 'session_speakers') return { type: 'session_speakers', speakers: [] };
  if (request.type === 'moments') return { type: 'moments', moments: [] };
  throw Error(`unexpected request ${request.type}`);
});
const root = createRoot(document.getElementById('root'));
const settle = async (fn = () => {}) => act(async () => { await fn(); await new Promise(r => setTimeout(r, 0)); });
const render = () => root.render(React.createElement(DetailDrawer, { key, item: base, initialAt, people: [], refreshToken: version,
  onClose: () => root.render(null), onEdit() {}, onTools() {}, onQueue() {}, onDataAction() {}, onChanged() {}, onMessage() {} }));
const q = selector => document.querySelector(selector);
const rows = () => [...document.querySelectorAll('.transcript-row')];
const button = text => [...document.querySelectorAll('button')].find(b => b.textContent === text);
const intersect = async (times = 1) => settle(() => {
  assert.equal(observers.size, 1, 'exactly one active transcript observer');
  const observer = [...observers][0];
  assert.equal(observer.options.root, q('.transcript-list'));
  for (let i = 0; i < times; i++) observer.callback([{ isIntersecting: true, target: observer.target }]);
});
const block = offset => { holdOffset = offset; hold = new Promise(r => { release = r; }); };
const unblock = async () => settle(() => { holdOffset = null; release(); });

try {
  await settle(render);
  assert.deepEqual(calls, [0]);
  assert(rows().length > 0 && rows().every(r => !r.textContent.includes('粗转')), 'first page must not invent a coarse suffix');
  assert(!button('显示更多文字'));
  const list = q('.transcript-list'); list.scrollTop = 123;
  block(pageSize);
  await intersect(4);
  assert.deepEqual(calls, [0, pageSize], 'repeated intersections start only one batch');
  assert.match(q('.transcript-pagination').textContent, /正在加载/);
  assert(rows().length > 0, 'in-flight pagination keeps existing text');
  await unblock();
  assert.equal(list.scrollTop, 123, 'pagination preserves reading position');
  assert(rows().some(r => r.textContent.includes('precise 45')));
  assert(rows().every(r => !r.textContent.includes('粗转')));

  failOffset = 2 * pageSize;
  const previousText = rows().map(r => r.textContent);
  await intersect();
  assert(q('.transcript-pagination .detail-load-error'));
  assert.deepEqual(rows().map(r => r.textContent), previousText);
  assert.equal(observers.size, 0, 'a failed page does not enter an automatic retry loop');
  const failedCount = calls.length;
  await settle(); assert.equal(calls.length, failedCount);
  failOffset = null;
  await settle(() => q('.transcript-pagination .detail-load-error button').click());
  assert(rows().some(r => r.textContent.includes('precise 75')), 'retry requests the failed next page');
  await settle(() => button('音频信息').click());
  assert.equal(observers.size, 0, 'hidden transcript never pages');
  await settle(() => button('文本转录').click());
  assert.equal(observers.size, 1);
  await settle(() => button('查看本地粗转').click());
  assert(rows().every(r => r.textContent.includes('粗转')));
  await settle(() => button('返回精转').click());
  while (q('.transcript-pagination')) await intersect();
  assert(!q('.transcript-pagination'), 'the final short page stops loading');
  assert.equal(rows().filter(r => r.textContent.includes('precise ')).length, 465);
  assert(rows().some(r => r.textContent.includes('粗转')), 'the genuine uncovered tail remains coarse');
  version++; list.scrollTop = 321; await settle(render);
  assert.equal(list.scrollTop, 321, 'background refresh preserves position');

  // Full pages need the final empty response to establish EOF, including ties.
  coarse = []; precise = precise.slice(0, pageSize); key++; calls = [];
  await settle(render); await intersect();
  assert.equal(rows().length, pageSize); assert(!q('.transcript-pagination'));

  // Opening at a time held behind an unresolved coarse sentence must fetch it.
  coarse = [row('coarse', 0, 1_000_000)];
  precise = Array.from({ length: 250 }, (_, i) => ({ ...row('precise', i, 5_000), revision: 0, ended_ms: from + i * 5_000 + 1_000 }));
  initialAt = from + 400_000; key++; calls = [];
  await settle(render);
  assert(calls.includes(180), 'initial positioning resolves the straddling sentence before selecting text');
  assert(q('.transcript-row.is-current'));

  // Refresh supersedes pagination; the old result cannot overwrite newer pages.
  initialAt = undefined; coarse = []; key++; calls = [];
  await settle(render); block(pageSize); await intersect();
  version++; await settle(render); await unblock();
  assert(!rows().some(r => r.textContent.includes('precise 45')));
  assert.equal(observers.size, 1, 'the newest load releases the paging lock');

  block(pageSize); await intersect();
  await settle(() => root.render(null));
  const atClose = calls.length; await unblock();
  assert(!q('.detail-drawer')); assert.equal(observers.size, 0);
  assert.equal(calls.length, atClose, 'closed detail does not request further pages');
  console.log('PASS: precise/coarse boundary, scroll paging, concurrency, errors/retry, EOF, views, seek, refresh and close');
} finally {
  await settle(() => root.unmount()); clearMocks(); dom.window.close();
}
