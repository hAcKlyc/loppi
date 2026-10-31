// npm ci && node scripts/test_entry_interaction.mjs
// Real React/widget/EntryRequests, mocked Tauri IPC; native policy is tested in test_native_entry.m.
import assert from 'node:assert/strict';
import { registerHooks, createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const requireTest = createRequire(import.meta.url);
const { JSDOM } = requireTest('jsdom');
const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost/' });
for (const name of ['window', 'document', 'HTMLElement', 'HTMLTextAreaElement', 'Event', 'MouseEvent', 'KeyboardEvent']) globalThis[name] = dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const style = document.createElement('style');
style.textContent = readFileSync(new URL('../src/entry.css', import.meta.url), 'utf8');
document.head.append(style);
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
    if (url.endsWith('.css')) return { format: 'module', source: '', shortCircuit: true };
    if (url.endsWith('/Rabbit.tsx')) return { format: 'module', source: 'export function Rabbit(){return null}', shortCircuit: true };
    if (/\.tsx?$/.test(url)) return {
      format: 'module', shortCircuit: true,
      source: ts.transpileModule(readFileSync(fileURLToPath(url), 'utf8'), {
        compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, jsx: ts.JsxEmit.ReactJSX },
      }).outputText,
    };
    return next(url, context);
  },
});
const { default: React, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { mockIPC, mockWindows, clearMocks } = await import('@tauri-apps/api/mocks');
const { emit } = await import('@tauri-apps/api/event');
const { FloatingWidget } = await import('../src/FloatingWidget.tsx');
const layouts = [], actions = [], focus = [], drags = [];
let outcome = 'confirmed';
let mode = 'top';
let finishLayout = true, confirmFocus = true;
mockWindows('float');
mockIPC(async (command, args) => {
  if (command === 'entry_snapshot') return { sequence: 1, state: null, error: null, mode: 'top', appearance: 'sleeping', control: 'resume', detail: null, notice: null };
  if (command === 'entry_layout') {
    layouts.push(args);
    if (mode === 'floating') return { width: 68, height: 68, band: 0, top_band: 0, top_shift: 0, rabbit_x: 8, rabbit_y: 8, menu_y: 8 };
    if (finishLayout && args.view === 'panel')
      await emit('loppi-entry-native', { kind: 8, value: 1, generation: args.generation });
    return { width: args.view === 'collapsed' ? 44 : 412, height: args.view === 'collapsed' ? 32 : 160, band: 32, top_band: 32, top_shift: args.feedback ? 178 : 296, rabbit_x: 8, rabbit_y: 2, menu_y: 8 };
  }
  if (command === 'entry_focus') { focus.push(args); return confirmFocus; }
  if (command === 'entry_action') { actions.push(args.action); return { outcome, message: 'simulated failure' }; }
  if (command === 'privacy_status') return { screen_and_system_audio: 1 };
  if (command === 'entry_drag' || command === 'plugin:window|start_dragging') { drags.push({ command, args }); return; }
}, { shouldMockEvents: true });
const root = createRoot(document.getElementById('root'));
const settle = async (fn = () => {}) => act(async () => { await fn(); await new Promise(resolve => setTimeout(resolve, 0)); });
const native = (kind, value) => settle(() => emit('loppi-entry-native', { kind, value }));
const button = text => [...document.querySelectorAll('button')].find(b => b.textContent === text);
const click = element => { assert(element, 'target exists'); return settle(() => element.click()); };
const panel = () => document.querySelector('.notch-surface:not([data-view=collapsed])');
const escape = () => settle(() => document.querySelector('.entry-root').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
try {
  await settle(() => root.render(React.createElement(FloatingWidget)));
  const persistentRabbit = document.querySelector(".entry-top .entry-rabbit");
  finishLayout = false;
  await native(1, 1);
  assert(panel(), 'native hover opens');
  assert.equal(document.querySelector('.entry-top .entry-rabbit'), persistentRabbit, 'rabbit keeps its DOM identity while opening');
  assert.equal(panel().dataset.ready, 'false', 'content waits for actual native completion');
  const oldLayout = layouts.at(-1).generation;
  await escape();
  assert.equal(document.querySelector('.entry-top .entry-rabbit'), persistentRabbit);
  await click(document.querySelector('.entry-rabbit'));
  await settle(() => emit('loppi-entry-native', { kind: 8, value: 1, generation: oldLayout }));
  assert.equal(panel().dataset.ready, 'false', 'canceled completion cannot reveal a newer panel');
  await settle(() => emit('loppi-entry-native', { kind: 8, value: 1, generation: layouts.at(-1).generation }));
  assert.equal(panel().dataset.ready, 'true', 'matching native completion reveals content');
  finishLayout = true;
  assert(!focus.some(call => call.acquire), 'hover never acquires app focus');
  await click(button('重点'));
  assert(!panel(), 'action immediately collapses');
  assert.equal(document.querySelector('.entry-top .entry-rabbit'), persistentRabbit, 'rabbit keeps its DOM identity while closing');
  assert(document.querySelector('[role=status]').textContent.includes('已标记'), 'confirmed feedback');
  assert.equal(window.getComputedStyle(document.querySelector('[role=status]')).left, '8px', 'collapsed feedback retains the full-width endpoint');
  // No kind 7 release: a direct/keyboard-generated click must override the guard.
  await click(document.querySelector('.entry-rabbit'));
  assert(panel(), 'explicit click reopens while inhibited');
  assert.equal(window.getComputedStyle(document.querySelector('[role=status]')).left, '110px', 'expanded feedback retains its separate endpoint');
  assert.equal(layouts.at(-1).suppress, false, 'native open is also unsuppressed');
  const count = layouts.length;
  await native(1, 1);
  assert.equal(layouts.length, count, 'repeated open is idempotent');
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 3300)); });
  assert(!document.querySelector('[role=status]'), 'feedback expires');
  assert(panel(), 'feedback expiry does not close the reopened menu');
  await escape();
  await native(1, 1);
  assert(!panel(), 'residual hover cannot undo intentional dismissal');
  await native(7, 0);
  await native(1, 1);
  assert(panel(), 'fresh native entry reopens');
  confirmFocus = false;
  await click(button('评论'));
  const firstIntent = focus.at(-1).generation;
  assert.notEqual(document.activeElement, document.querySelector('textarea'), 'unconfirmed activation does not focus editor');
  await settle(() => emit('loppi-entry-native', { kind: 9, value: 1, generation: firstIntent }));
  const text = document.querySelector('textarea');
  assert.equal(document.activeElement, text, 'late actual activation focuses its editor');
  await settle(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(text, '保留草稿');
    text.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await click(button('取消'));
  await click(document.querySelector('.entry-rabbit'));
  await click(button('评论'));
  await settle(() => emit('loppi-entry-native', { kind: 9, value: 1, generation: firstIntent }));
  assert.notEqual(document.activeElement, document.querySelector('textarea'), 'old focus confirmation cannot affect a new comment');
  await settle(() => emit('loppi-entry-native', { kind: 9, value: 1, generation: focus.at(-1).generation }));
  assert.equal(document.activeElement, document.querySelector('textarea'));
  confirmFocus = true;
  assert.equal(document.querySelector('textarea').value, '保留草稿', 'cancel preserves draft');
  outcome = 'rejected';
  await click(button('保存'));
  assert(!panel(), 'failed action remains collapsed');
  await click(document.querySelector('.entry-rabbit'));
  await click(button('评论'));
  assert.equal(document.querySelector('textarea').value, '保留草稿', 'failure preserves draft');
  const firstComment = actions.find(a => a.kind === 'comment');
  outcome = 'confirmed';
  await click(button('保存'));
  assert.equal(actions.at(-1).anchor_ms, firstComment.anchor_ms, 'retry preserves comment anchor');
  await click(document.querySelector('.entry-rabbit'));
  await click(button('评论'));
  assert.equal(document.querySelector('textarea').value, '', 'confirmed save clears draft');
  await click(button('取消'));
  await settle(() => document.querySelector('.entry-rabbit').focus());
  assert(panel(), 'explicit keyboard focus reopens after dismissal');
  assert(document.activeElement.matches('.notch-rabbit-control'), 'focus moves off hidden trigger');
  mode = 'floating';
  await settle(() => emit('loppi-entry-snapshot', { sequence: 2, state: null, error: null, mode, appearance: 'sleeping', control: 'resume', detail: null, notice: null }));
  const rabbit = document.querySelector('.entry-rabbit');
  const marks = actions.filter(a => a.kind === 'mark').length;
  await click(rabbit);
  assert.equal(actions.filter(a => a.kind === 'mark').length, marks + 1, 'floating click still marks');
  await native(7, 0);
  await native(1, 1);
  assert(document.querySelector('.rabbit-menu'));
  await settle(() => emit('loppi-entry-native', { kind: 5, value: 1, geometry: { width: 68, height: 68, rabbit_x: 8, rabbit_y: 8, menu_y: 8, band: 0, top_band: 0, top_shift: 0 } }));
  assert(!document.querySelector('.rabbit-menu'), 'native drag collapses the menu');
  assert.equal(document.querySelector('.entry-floating').style.getPropertyValue('--entry-rabbit-y'), '8px', 'drag-start geometry keeps the rabbit visible even with feedback');
  await native(1, 1);
  assert(!document.querySelector('.rabbit-menu'), 'hover cannot reopen during drag');
  await click(rabbit);
  assert.equal(actions.filter(a => a.kind === 'mark').length, marks + 1, 'drag does not mark');
  assert.equal(drags.length, 0, 'pointer moves do not start asynchronous window dragging or write preferences');
  await settle(() => emit('loppi-entry-native', { kind: 5, value: 0, geometry: { width: 132, height: 230, rabbit_x: 8, rabbit_y: 8, menu_y: 55, band: 0, top_band: 0, top_shift: 0 } }));
  assert.equal(drags.length, 0, 'native completion saves position without a WebView round trip');
  assert.equal(document.querySelector('.entry-floating').dataset.menuDown, 'true');
  assert.equal(document.querySelector('.entry-floating').style.getPropertyValue('--entry-rabbit-y'), '8px');
  await native(10, 0);
  assert.equal(actions.filter(a => a.kind === 'mark').length, marks + 2, 'native physical click uses the existing mark action');
  await settle(() => emit('loppi-entry-native', { kind: 5, value: 0, error: 'position save failed' }));
  assert.match(document.querySelector('[role=status]').textContent, /position save failed/, 'position save errors remain visible');
  console.log('PASS: hover/click, suppression, idempotence, comment focus/cancel/failure/retry, floating click/drag');
} finally {
  await settle(() => root.unmount());
  clearMocks();
  dom.window.close();
}
