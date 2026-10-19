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
let finishLayout = true, confirmFocus = true;
mockWindows('float');
mockIPC(async (command, args) => {
  if (command === 'entry_snapshot') return { sequence: 1, state: null, error: null, mode: 'top', appearance: 'sleeping', control: 'resume', detail: null, notice: null };
  if (command === 'entry_layout') {
    layouts.push(args);
    if (finishLayout && args.view === 'panel')
      await emit('hear-entry-native', { kind: 8, value: 1, generation: args.generation });
    return { width: args.view === 'collapsed' ? 44 : 412, height: args.view === 'collapsed' ? 32 : 160, band: 32, rabbit_x: 8, rabbit_y: 2 };
  }
  if (command === 'entry_focus') { focus.push(args); return confirmFocus; }
  if (command === 'entry_action') { actions.push(args.action); return { outcome, message: 'simulated failure' }; }
  if (command === 'privacy_status') return { screen_and_system_audio: 1 };
  if (command === 'entry_drag' || command === 'plugin:window|start_dragging') { drags.push({ command, args }); return; }
}, { shouldMockEvents: true });
const root = createRoot(document.getElementById('root'));
const settle = async (fn = () => {}) => act(async () => { await fn(); await new Promise(resolve => setTimeout(resolve, 0)); });
const native = (kind, value) => settle(() => emit('hear-entry-native', { kind, value }));
const button = text => [...document.querySelectorAll('button')].find(b => b.textContent === text);
const click = element => { assert(element, 'target exists'); return settle(() => element.click()); };
const panel = () => document.querySelector('.notch-surface');
const escape = () => settle(() => document.querySelector('.entry-root').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
try {
  await settle(() => root.render(React.createElement(FloatingWidget)));
  finishLayout = false;
  await native(1, 1);
  assert(panel(), 'native hover opens');
  assert.equal(panel().dataset.ready, 'false', 'content waits for actual native completion');
  const oldLayout = layouts.at(-1).generation;
  await escape();
  await click(document.querySelector('.entry-rabbit'));
  await settle(() => emit('hear-entry-native', { kind: 8, value: 1, generation: oldLayout }));
  assert.equal(panel().dataset.ready, 'false', 'canceled completion cannot reveal a newer panel');
  await settle(() => emit('hear-entry-native', { kind: 8, value: 1, generation: layouts.at(-1).generation }));
  assert.equal(panel().dataset.ready, 'true', 'matching native completion reveals content');
  finishLayout = true;
  assert(!focus.some(call => call.acquire), 'hover never acquires app focus');
  await click(button('重点'));
  assert(!panel(), 'action immediately collapses');
  assert(document.querySelector('[role=status]').textContent.includes('已标记'), 'confirmed feedback');
  // No kind 7 release: a direct/keyboard-generated click must override the guard.
  await click(document.querySelector('.entry-rabbit'));
  assert(panel(), 'explicit click reopens while inhibited');
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
  await settle(() => emit('hear-entry-native', { kind: 9, value: 1, generation: firstIntent }));
  const text = document.querySelector('textarea');
  assert.equal(document.activeElement, text, 'late actual activation focuses its editor');
  await settle(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(text, '保留草稿');
    text.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await click(button('取消'));
  await click(document.querySelector('.entry-rabbit'));
  await click(button('评论'));
  await settle(() => emit('hear-entry-native', { kind: 9, value: 1, generation: firstIntent }));
  assert.notEqual(document.activeElement, document.querySelector('textarea'), 'old focus confirmation cannot affect a new comment');
  await settle(() => emit('hear-entry-native', { kind: 9, value: 1, generation: focus.at(-1).generation }));
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
  await settle(() => emit('hear-entry-snapshot', { sequence: 2, state: null, error: null, mode: 'floating', appearance: 'sleeping', control: 'resume', detail: null, notice: null }));
  const rabbit = document.querySelector('.entry-rabbit');
  const marks = actions.filter(a => a.kind === 'mark').length;
  await click(rabbit);
  assert.equal(actions.filter(a => a.kind === 'mark').length, marks + 1, 'floating click still marks');
  await settle(() => {
    rabbit.dispatchEvent(new MouseEvent('pointerdown', { screenX: 100, screenY: 100, bubbles: true }));
    rabbit.dispatchEvent(new MouseEvent('pointermove', { screenX: 110, screenY: 100, bubbles: true }));
  });
  await click(rabbit);
  assert.equal(actions.filter(a => a.kind === 'mark').length, marks + 1, 'drag does not mark');
  assert(drags.some(d => d.command === 'entry_drag' && d.args.dragging), 'native drag starts');
  console.log('PASS: hover/click, suppression, idempotence, comment focus/cancel/failure/retry, floating click/drag');
} finally {
  await settle(() => root.unmount());
  clearMocks();
  dom.window.close();
}
