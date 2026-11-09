// node --test scripts/test_speaker_picker.mjs
// Real drawer/picker/IPC adapter, with WebKit's non-focusable mouse-button default.
import assert from 'node:assert/strict';
import { registerHooks, createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const { JSDOM } = createRequire(import.meta.url)('jsdom');
const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost/' });
for (const name of ['window', 'document', 'Node', 'Element', 'HTMLElement', 'HTMLButtonElement', 'HTMLInputElement', 'HTMLLabelElement', 'HTMLDialogElement', 'Event', 'MouseEvent', 'KeyboardEvent']) globalThis[name] = dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.ResizeObserver = class { observe() {} disconnect() {} };
HTMLDialogElement.prototype.showModal = function () { this.open = true; };
HTMLDialogElement.prototype.close = function () { this.open = false; };
HTMLElement.prototype.scrollIntoView = function () {};
HTMLElement.prototype.scrollTo = function () {};
globalThis.isTauri = true;
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
const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
const { DetailDrawer } = await import('../src/DetailDrawer.tsx');
const people = [{ id: 'person-a', display_name: '测试人物', sample_count: 0, revision: 1 }];
let assigned = null, failure = null, blocked = null, release;
const requests = [], messages = [];
const item = () => ({
  id: 'event', title: '指派回归', started_ms: 1000, ended_ms: 20000,
  sources: 'system', revision: 1, processing_state: 'transcribed', coarse_segments: [],
  precise_segments: [0, 1].map(i => ({
    id: `sentence-${i}`, started_ms: 1000 + i * 8000, ended_ms: 7000 + i * 8000,
    source: 'system', text: '测试发言', speaker: 'group-a', revision: 7, speaker_revision: 33,
    person_id: assigned, person_name: people.find(p => p.id === assigned)?.display_name ?? null,
    person_origin: assigned ? 'manual' : null, corrected: false, original_text: null,
  })),
});
mockIPC(async (command, args) => {
  assert.equal(command, 'core_request');
  const r = args.request;
  if (r.type === 'event_detail') return { type: 'event', id: 'read', event: item() };
  if (r.type === 'session_speakers') return { type: 'session_speakers', id: 'read', speakers: [{ group_id: 'group-a', segment_id: 'sentence-0', name: '说话人 1', anonymous: !assigned, revision: 33 }] };
  if (r.type === 'moments') return { type: 'moments', id: 'read', moments: [] };
  requests.push(r);
  if (blocked) await blocked;
  if (failure) { const result = failure; failure = null; return result; }
  if (r.type === 'bind_speaker') assigned = r.person_id;
  if (r.type === 'create_and_bind_speaker') {
    assigned = 'created'; people.push({ id: assigned, display_name: r.display_name, sample_count: 0 });
    return { type: 'person_changed', id: 'mutation', person_id: assigned, sample_count: 0 };
  }
  return { type: 'text_changed', id: 'mutation', segment_id: r.segment_id, revision: 34 };
});
const root = createRoot(document.getElementById('root'));
const settle = async (fn = () => {}) => act(async () => { await fn(); await new Promise(resolve => setTimeout(resolve, 0)); });
const picker = () => document.querySelector('.speaker-picker');
const chips = () => [...document.querySelectorAll('button.speaker-chip')];
const button = text => [...picker().querySelectorAll('button')].find(b => b.textContent.trim() === text);
const option = text => [...picker().querySelectorAll('.speaker-picker-option')].find(b => b.textContent.includes(text));
// WebKit focuses a nearby focusable ancestor on uncanceled mouse down, before click.
const pointer = async target => {
  assert(target && !target.disabled, 'enabled pointer target');
  await settle(() => {
    target.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
    if (target.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }))) {
      const fallback = target.closest('.speaker-picker[tabindex], .transcript-list, dialog');
      if (fallback?.matches('dialog')) fallback.tabIndex = -1;
      fallback?.focus();
    }
  });
  if (target.isConnected) await settle(() => target.click());
};
const open = () => pointer(chips()[0]);
const search = value => settle(() => {
  const input = picker().querySelector('input[type=search]');
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
});
try {
  await settle(() => root.render(React.createElement(DetailDrawer, {
    item: item(), people, refreshToken: 0, onClose: () => {}, onEdit: () => {},
    onTools: () => {}, onQueue: () => {}, onDataAction: () => {}, onChanged: () => {},
    onMessage: message => messages.push(message),
  })));
  await settle();
  assert(chips().length, document.body.textContent);
  await open();
  await pointer(picker().querySelector('h3'));
  assert(picker()?.contains(document.activeElement), 'pressing the picker heading keeps focus inside');
  await pointer(chips()[0]);
  assert.equal(picker(), null, 'clicking the same speaker chip closes without reopening');
  assert.equal(document.activeElement, chips()[0]);
  await open();
  await pointer(option('测试人物').querySelector('.speaker-picker-option-name'));
  assert(picker(), 'WebKit option click must not dismiss before confirming');
  assert.equal(requests.length, 0, 'choice only drafts an assignment');
  await pointer(button('确认'));
  assert.equal(requests[0].type, 'bind_speaker');
  assert.equal(requests[0].expected_revision, 33);
  assert.equal(requests[0].entire_local_speaker, true);
  assert.equal(picker(), null);
  assert(chips().every(chip => chip.textContent.includes('测试人物')), 'read-back updates labels');

  await open(); await search('新人物');
  await pointer(option('新建').querySelector('.speaker-picker-option-name'));
  assert(picker(), 'WebKit create click must retain the draft');
  assert.equal(requests.length, 1);
  await pointer([...picker().querySelectorAll('label')][0].querySelector('span'));
  assert(picker().querySelector('input[type=radio]').checked, 'radio label retains native selection');
  await pointer(button('确认'));
  assert.equal(requests[1].type, 'create_and_bind_speaker');
  assert.equal(requests[1].display_name, '新人物');
  assert.equal(requests[1].expected_revision, 7);
  assert.equal(requests[1].entire_local_speaker, false);
  assert(chips()[0].textContent.includes('新人物'));
  assert(messages.includes('人物已保存 · 待补充声纹'));

  await open(); await pointer(option('测试人物'));
  failure = { type: 'error', id: 'failed', code: 'speaker_binding_invalid', message: '测试写入失败' };
  blocked = new Promise(resolve => { release = resolve; });
  await pointer(button('确认'));
  assert(picker().getAttribute('aria-busy') === 'true');
  assert(chips().every(chip => chip.disabled));
  await settle(() => picker().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
  assert(picker(), 'pending IPC cannot be dismissed');
  await settle(() => { release(); blocked = null; });
  assert(picker().querySelector('[role=alert]').textContent.includes('测试写入失败'), 'IPC errors remain visible');
  assert(chips()[0].textContent.includes('新人物'), 'failed write cannot update labels');
  await pointer(button('确认'));
  assert.equal(picker(), null, 'explicit retry closes only after success');

  await open();
  await settle(() => chips()[0].focus());
  assert(picker(), 'returning focus to the anchor is allowed');
  await settle(() => picker().querySelector('input').focus());
  await settle(() => document.querySelector('[aria-label="关闭详情"]').focus());
  assert.equal(picker(), null, 'keyboard focus leaving still dismisses');
  await open();
  await settle(() => picker().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
  assert.equal(picker(), null, 'Escape still dismisses');
  await open();
  await settle(() => document.body.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true })));
  assert.equal(picker(), null, 'outside pointer still dismisses');
  console.log('speaker picker: WebKit pointer choices/scopes, IPC completion/errors and dismissal passed');
} finally {
  await settle(() => root.unmount()); clearMocks(); dom.window.close();
}
