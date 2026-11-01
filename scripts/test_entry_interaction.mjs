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
for (const name of ['window', 'document', 'HTMLElement', 'HTMLTextAreaElement', 'HTMLInputElement', 'CompositionEvent', 'Event', 'MouseEvent', 'KeyboardEvent']) globalThis[name] = dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
// JSDOM exposes custom properties rather than resolving CSS lengths. Resolve
// only the numeric endpoint checked here; native/browser checks cover rendering.
const tokenSource=readFileSync(new URL('../src/tokens.css',import.meta.url),'utf8');
const visualTokens=Object.fromEntries([...tokenSource.matchAll(/--([\w-]+):\s*([^;]+);/g)].map(m=>[m[1],m[2].trim()]));
function computedLeft(element){
 let value=window.getComputedStyle(element).left;
 for(let i=0;i<20&&value.includes('var(');i++)value=value.replace(/var\(--([\w-]+)\)/g,(_,name)=>visualTokens[name]??'INVALID');
 value=value.replaceAll('calc','').replaceAll('px','');
 assert.match(value,/^[\d\s()+*\-/.]+$/,'Only resolved numeric geometry is evaluated');
 return Function('return ('+value+')')()+'px';
}
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
    if (url.endsWith('.svg')) return { format: 'module', source: `export default ${JSON.stringify(url)}`, shortCircuit: true };
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
    return { width: args.view === 'collapsed' ? 44 : 412, height: args.view === 'collapsed' ? 32 : 176, band: 32, top_band: 32, top_shift: args.feedback ? 178 : 296, rabbit_x: 8, rabbit_y: 2, menu_y: 8 };
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
  const editor = () => document.querySelector('input[aria-label="留下评论"]');
  const enter = (options={}) => settle(() => editor().dispatchEvent(new KeyboardEvent('keydown', {key:'Enter', keyCode:13, bubbles:true, cancelable:true, ...options})));
  const type = value => settle(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(editor(), value);
    editor().dispatchEvent(new Event('input', {bubbles:true}));
  });
  assert.equal(document.activeElement, editor(), 'hover focuses persistent one-line editor after completion and activation');
  assert.equal(document.querySelectorAll('.notch-actions button').length, 2);
  assert(document.querySelector('.notch-wordmark').getAttribute('src').includes('wordmark-reversed.svg'));
  await click(button('重点'));
  assert(!panel(), 'action immediately collapses');
  assert.equal(document.querySelector('.entry-top .entry-rabbit'), persistentRabbit, 'rabbit keeps DOM identity');
  assert.match(document.querySelector('[role=status]').textContent, /已标记/);
  await click(document.querySelector('.entry-rabbit'));
  assert(panel(), 'explicit click overrides inhibit');
  const count = layouts.length;
  const acquisitions = focus.filter(call => call.acquire).length;
  await native(1, 1);
  assert.equal(layouts.length, count, 'repeated open is idempotent');
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 3300)); });
  assert(panel(), 'feedback expiry leaves panel open');
  assert.equal(focus.filter(call => call.acquire).length, acquisitions, 'feedback layout never reacquires keyboard');
  await escape();
  await native(1, 1);
  assert(!panel(), 'residual hover cannot undo intentional dismissal');
  await native(7, 0);
  confirmFocus = false;
  await native(1, 1);
  const firstIntent = focus.at(-1).generation;
  assert.notEqual(document.activeElement, editor(), 'unconfirmed native activation cannot focus editor');
  await settle(() => emit('loppi-entry-native', {kind:9,value:1,generation:firstIntent}));
  assert.equal(document.activeElement, editor(), 'late actual activation focuses editor');
  await type('保留草稿');
  const beforeIME = actions.length;
  await settle(() => editor().dispatchEvent(new CompositionEvent('compositionstart', {bubbles:true})));
  await enter({isComposing:true});
  await enter({keyCode:229});
  assert.equal(actions.length,beforeIME,'IME confirmation never submits');
  await settle(() => editor().dispatchEvent(new CompositionEvent('compositionend', {bubbles:true})));
  await escape();
  await click(document.querySelector('.entry-rabbit'));
  await settle(() => emit('loppi-entry-native', {kind:9,value:1,generation:firstIntent}));
  assert.notEqual(document.activeElement, editor(), 'old focus cannot affect new panel');
  await settle(() => emit('loppi-entry-native', {kind:9,value:1,generation:focus.at(-1).generation}));
  assert.equal(document.activeElement,editor());
  assert.equal(editor().value,'保留草稿','Escape preserves draft');
  confirmFocus = true;
  outcome = 'rejected';
  await enter();
  assert(!panel(),'comment send collapses');
  const firstComment=actions.at(-1);
  assert.equal(firstComment.text,'保留草稿');
  await click(document.querySelector('.entry-rabbit'));
  assert.equal(editor().value,'保留草稿','failure retains draft');
  outcome='uncertain';
  await enter();
  const uncertainComment=structuredClone(actions.at(-1));
  await click(document.querySelector('.entry-rabbit'));
  assert(editor().readOnly,'unconfirmed request cannot be edited');
  assert.equal(editor().value,'保留草稿');
  outcome='confirmed';
  await click(document.querySelector('.comment-send'));
  assert.deepEqual(actions.at(-1),uncertainComment,'uncertain retry uses identical request');
  assert.equal(actions.at(-1).anchor_ms,firstComment.anchor_ms,'anchor survives failure/retry');
  await click(document.querySelector('.entry-rabbit'));
  assert.equal(editor().value,'','confirmed save clears draft');
  await type('外部收起保留');
  await native(2,0);
  assert(!panel());
  await click(document.querySelector('.entry-rabbit'));
  assert.equal(editor().value,'外部收起保留','outside dismissal retains draft');
  await escape();
  await settle(() => document.querySelector('.entry-rabbit').focus());
  assert(panel(),'keyboard trigger reopens');
  assert.equal(document.activeElement,editor(),'keyboard opening also focuses editor');
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
  const platformDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { platform: 'Win32' } });
  try {
    const windowsMarks = actions.filter(a => a.kind === 'mark').length;
    await settle(() => rabbit.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 })));
    assert.equal(drags.length, 1, 'Windows WebView pointer starts the existing native move loop');
    await settle(() => rabbit.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 })));
    assert.equal(actions.filter(a => a.kind === 'mark').length, windowsMarks, 'physical click waits for native release ownership');
    await native(10, 1);
    assert.equal(actions.filter(a => a.kind === 'mark').length, windowsMarks + 1, 'one native release creates one mark');
    await click(rabbit);
    assert.equal(actions.filter(a => a.kind === 'mark').length, windowsMarks + 2, 'Windows keyboard activation still marks');
  } finally {
    if (platformDescriptor) Object.defineProperty(globalThis, 'navigator', platformDescriptor);
    else delete globalThis.navigator;
  }
  console.log('PASS: hover/click, suppression, idempotence, comment focus/cancel/failure/retry, floating click/drag');
} finally {
  await settle(() => root.unmount());
  clearMocks();
  dom.window.close();
}
