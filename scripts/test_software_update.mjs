// Real React hook and components driven by mocked host snapshots and events.
import assert from 'node:assert/strict';
import test from 'node:test';
import { registerHooks } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { JSDOM } from 'jsdom';

const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost/' });
for (const name of ['window', 'document', 'HTMLElement', 'HTMLButtonElement', 'HTMLDialogElement', 'Event', 'MouseEvent', 'KeyboardEvent']) globalThis[name] = dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.isTauri = true;
window.HTMLDialogElement.prototype.showModal = function () { this.open = true; };
window.HTMLDialogElement.prototype.close = function () { this.open = false; };
registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('.') && context.parentURL?.includes('/src/')) for (const suffix of ['.ts', '.tsx']) {
      const url = new URL(specifier + suffix, context.parentURL);
      if (existsSync(url)) return next(url.href, context);
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (url.endsWith('.css')) return { format: 'module', source: '', shortCircuit: true };
    if (/\.tsx?$/.test(url)) return { format: 'module', shortCircuit: true, source: ts.transpileModule(readFileSync(fileURLToPath(url), 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, jsx: ts.JsxEmit.ReactJSX } }).outputText };
    return next(url, context);
  },
});
const { default: React, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { emit } = await import('@tauri-apps/api/event');
const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
const { useAppUpdate } = await import('../src/useAppUpdate.ts');
const { SoftwareUpdate, RestartUpdate } = await import('../src/SoftwareUpdate.tsx');
const h = React.createElement;

test('host snapshots reveal verified updates across window reloads and preserve the install action', async () => {
  const event = 'loppi-update-status';
  const available = { currentVersion: '1.0.0', version: '1.1.0' };
  const initial = { revision: 0, available: null, phase: 'idle', checked: false, error: '' };
  let status = initial;
  let pendingSnapshot, pendingInstall, readInitial = true;
  const calls = [];
  const listeners = new Map();
  async function publish(fields) {
    status = { ...status, ...fields, revision: status.revision + 1 };
    await emit(event, status);
  }
  mockIPC(async (command, args) => {
    if (command === 'plugin:event|listen') {
      assert.equal(args.event, event);
      listeners.set(args.handler, args.handler);
      return args.handler;
    }
    if (command === 'plugin:event|unlisten') {
      listeners.delete(args.eventId);
      return;
    }
    if (command === 'plugin:event|emit') {
      for (const [id, handler] of listeners) {
        window.__TAURI_INTERNALS__.runCallback(handler, { ...args, id });
      }
      return;
    }
    calls.push(command);
    if (command === 'plugin:app|version') return '1.0.0';
    if (command === 'update_status') {
      if (readInitial) {
        readInitial = false;
        return new Promise(resolve => { pendingSnapshot = () => resolve(initial); });
      }
      return status;
    }
    if (command === 'check_update') {
      await publish({ phase: 'checking', error: '' });
      await publish({ phase: 'idle', error: 'offline' });
      return;
    }
    if (command === 'install_update') {
      assert.equal(args.version, available.version);
      await publish({ phase: 'stopping', error: '' });
      return new Promise(resolve => {
        pendingInstall = async error => {
          await publish({ phase: 'idle', error });
          resolve();
        };
      });
    }
    throw Error(command);
  });

  function Harness() {
    const update = useAppUpdate();
    return h(React.Fragment, null, h('aside', { className: 'sidebar' }, h(RestartUpdate, { update })), h(SoftwareUpdate, { update }));
  }
  const root = createRoot(document.getElementById('root'));
  const render = () => root.render(h(React.StrictMode, null, h(Harness)));
  const settle = async fn => act(async () => { await fn(); await new Promise(resolve => setTimeout(resolve, 0)); });
  const restart = () => document.querySelector('.restart-update-button');
  const check = () => document.querySelector('.software-update-setting button');
  try {
    await settle(render);
    assert.equal(calls.includes('check_update'), false, 'renderer does not schedule checks');
    await settle(() => publish({ phase: 'downloading' }));
    await settle(() => pendingSnapshot());
    assert.equal(restart(), null, 'host has not verified the package');
    assert.match(document.body.textContent, /后台下载/);
    assert.equal(check().disabled, true, 'manual check joins host work');

    await settle(() => root.render(null));
    await settle(() => publish({ phase: 'idle', available, checked: true }));
    await settle(render);
    assert.match(restart().textContent, /重启更新/);
    assert.equal(calls.includes('install_update'), false, 'ready update waits for the user');
    assert.equal(calls.includes('check_update'), false, 'reopening only reads the cached host state');

    await settle(() => { restart().click(); restart().click(); });
    assert.equal(calls.filter(c => c === 'install_update').length, 1);
    assert.equal(calls.includes('check_update'), false, 'install uses the verified local package');
    assert.match(document.body.textContent, /等待后台退出/);
    assert.equal(document.querySelector('dialog'), null);
    assert.equal(restart().disabled, true);
    await settle(() => pendingInstall('后台未能正常退出'));
    assert.match(document.querySelector('.sidebar-update [role=alert]').textContent, /后台未能正常退出/);

    await settle(() => check().click());
    assert.equal(calls.filter(c => c === 'check_update').length, 1);
    assert.match(document.querySelector('[role=alert]').textContent, /offline/);
    assert.equal(restart().disabled, false, 'failed recheck retains a verified update');
    await settle(() => publish({ checked: true, available: null, error: '' }));
    assert.match(document.body.textContent, /当前已是最新版本/);
    assert.equal(restart(), null);
  } finally {
    await settle(() => root.unmount());
    clearMocks();
  }
});
