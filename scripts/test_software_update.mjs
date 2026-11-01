// Real React hook and components; only desktop IPC/clock and dialog API are mocked.
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
const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
const { useAppUpdate } = await import('../src/useAppUpdate.ts');
const { SoftwareUpdate, RestartUpdate } = await import('../src/SoftwareUpdate.tsx');
const h = React.createElement;

test('startup silently downloads once; only a ready update shows the sidebar restart button', async () => {
  const timers = new Map(); let timerId = 0;
  const nativeSet = window.setTimeout, nativeClear = window.clearTimeout;
  window.setTimeout = (callback, delay) => { assert.equal(delay, 10_000); timers.set(++timerId, callback); return timerId; };
  window.clearTimeout = id => timers.delete(id);
  const calls = []; let failCheck = false, pendingCheck, pendingInstall;
  const update = { currentVersion: '1.0.0', version: '1.1.0' };
  let latest = update;
  mockIPC(async (command, args) => {
    calls.push(command);
    if (command === 'plugin:app|version') return '1.0.0';
    if (command === 'check_update') {
      if (failCheck) throw 'offline';
      args.progress.onmessage({ phase: 'downloading', downloaded: 50, total: 100 });
      return new Promise(resolve => { pendingCheck = () => resolve(latest); });
    }
    if (command === 'install_update') { assert.equal(args.version, '1.1.0'); return new Promise((resolve, reject) => { pendingInstall = { resolve, reject, channel: args.progress }; }); }
    throw Error(command);
  });
  let controller;
  function Harness() { controller = useAppUpdate(); return h(React.Fragment, null, h('aside', { className: 'sidebar' }, h(RestartUpdate, { update: controller })), h(SoftwareUpdate, { update: controller })); }
  const root = createRoot(document.getElementById('root'));
  const settle = async fn => act(async () => { await fn?.(); await new Promise(resolve => setTimeout(resolve, 0)); });
  const restart = () => document.querySelector('.restart-update-button');
  try {
    await settle(() => root.render(h(React.StrictMode, null, h(Harness))));
    assert.equal(timers.size, 1, 'StrictMode cleanup leaves only one startup timer');
    assert.equal(calls.filter(c => c === 'check_update').length, 0);
    await settle(() => { for (const callback of timers.values()) callback(); });
    assert.equal(calls.filter(c => c === 'check_update').length, 1);
    assert.equal(restart(), null, 'download has not finished yet');
    assert.equal(document.querySelector('dialog'), null);
    assert.match(document.body.textContent, /后台下载/);
    await settle(() => { void controller.check(); });
    assert.equal(calls.filter(c => c === 'check_update').length, 1, 'busy download cannot start a second operation');
    await settle(() => pendingCheck());
    assert.equal(restart().textContent.includes('重启更新'), true);
    assert.equal(calls.includes('install_update'), false, 'ready update waits for the user');
    await settle(() => { restart().click(); restart().click(); });
    assert.equal(calls.filter(c => c === 'install_update').length, 1, 'double click starts one install');
    assert.equal(calls.filter(c => c === 'check_update').length, 1, 'restart makes no new check or download request');
    assert.equal(document.querySelector('dialog'), null, 'click directly installs without a popup');
    assert.equal(restart().disabled, true);
    await settle(() => pendingInstall.channel.onmessage({ phase: 'stopping', downloaded: 100, total: 100 }));
    assert.match(document.body.textContent, /等待后台退出/);
    await settle(() => pendingInstall.reject('后台未能正常退出'));
    assert.match(document.querySelector('.sidebar-update [role=alert]').textContent, /后台未能正常退出/);
    assert.equal(restart().disabled, false);
    await settle(() => { restart().click(); });
    assert.equal(calls.filter(c => c === 'install_update').length, 2);
    await settle(() => pendingInstall.reject('安装失败'));
    failCheck = true; await settle(() => controller.check());
    assert.match(document.querySelector('[role=alert]').textContent, /offline/);
    assert(restart(), 'a failed recheck does not discard a verified cached update');
    failCheck = false; latest = null;
    await settle(() => { void controller.check(); });
    await settle(() => pendingCheck());
    assert.match(document.body.textContent, /当前已是最新版本/);
    assert.equal(restart(), null);
  } finally {
    await settle(() => root.unmount()); clearMocks(); window.setTimeout = nativeSet; window.clearTimeout = nativeClear;
  }
});
