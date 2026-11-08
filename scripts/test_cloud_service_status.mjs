import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

test('cloud settings show queue facts without changing cloud configuration or processing', async (t) => {
  const server = await createServer({ server: { middlewareMode: true }, appType: 'custom' });
  try {
    const { CloudServiceSettings } = await server.ssrLoadModule('/src/CloudServiceSettings.tsx');
    const queue = { provider: 'llm', configured: true, paused: false, blocked_reason: null, pending: 0, processing: 0, failed: 0 };
    let refreshes = 0;
    const refresh = async () => { refreshes += 1; };
    const render = (value, provider = 'llm') => renderToStaticMarkup(createElement(CloudServiceSettings, { provider, queue: value, refresh }));
    const status = html => html.match(/role="status">([^<]+)</)?.[1];

    await t.test('unread and unconfigured queues do not claim to be running', () => {
      const unread = render(undefined);
      assert.equal(status(unread), '正在读取配置…');
      assert.match(unread, /disabled=""/);
      const unconfigured = render({ ...queue, configured: false, paused: true, processing: 1 });
      assert.equal(status(unconfigured), '未配置');
      assert.match(unconfigured, />配置<\/button>/);
      assert.doesNotMatch(unconfigured, /已配置|处理中|在智能分析中继续/);
    });

    await t.test('configured but paused queues name the pause and where to continue', () => {
      for (const provider of ['llm', 'asr']) {
        const html = render({ ...queue, provider, paused: true, pending: 72, processing: 1 }, provider);
        assert.equal(status(html), '已配置 · 已暂停');
        assert.match(html, /录音与云端处理分别控制，请在智能分析中继续。/);
        assert.match(html, />修改密钥<\/button>/);
        assert.doesNotMatch(html, /运行中|>继续<\/button>/);
      }
    });

    await t.test('account blocks take precedence over ordinary progress and pause', () => {
      const blocked = render({ ...queue, blocked_reason: 'provider_http_402', paused: true, pending: 8, processing: 1 });
      assert.equal(status(blocked), '已配置 · 余额不足');
      assert.match(blocked, /充值后，在智能分析中继续；失败项需手动重试。/);
      assert.doesNotMatch(status(blocked), /已暂停|处理中/);
      const other = render({ ...queue, blocked_reason: 'future_block', processing: 1 });
      assert.equal(status(other), '已配置 · 处理受阻');
      assert.match(other, /请在智能分析中查看详情。/);
    });

    await t.test('live work shows actual counts while an empty queue waits for work', () => {
      assert.equal(status(render({ ...queue, pending: 68, processing: 1 })), '已配置 · 1 项处理中');
      assert.equal(status(render({ ...queue, pending: 68 })), '已配置 · 68 项待处理');
      assert.equal(status(render(queue)), '已配置 · 等待任务');
      const failed = render({ ...queue, failed: 3 });
      assert.match(failed, /3 项处理失败，请在智能分析中查看。/);
      assert.doesNotMatch(failed, /已完成|运行中/);
    });

    await t.test('SettingsView passes each existing queue snapshot through to its service row', async () => {
      const { SettingsView } = await server.ssrLoadModule('/src/WorkspaceViews.tsx');
      const html = renderToStaticMarkup(createElement(SettingsView, {
        date: new Date(2026, 10, 1), state: null,
        queues: [{ ...queue, paused: true, pending: 72 }, { ...queue, provider: 'asr', processing: 2 }],
        selection: null, privacy: null, loginStatus: null, models: null, busy: false,
        onStatus: refresh, onContext: refresh, onTimeline: refresh,
        onMutate: async () => { assert.fail('render must not mutate'); }, onMessage: () => {},
      }));
      assert.match(html, /事件分析/);
      assert.match(html, /语音识别/);
      assert.match(html, /已配置 · 已暂停/);
      assert.match(html, /已配置 · 2 项处理中/);
    });

    await t.test('an uncertain configuration keeps its existing confirmation action and sends no render request', async () => {
      const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
      const setup = await server.ssrLoadModule('/src/setup.ts');
      const oldWindow = globalThis.window;
      const oldInternals = globalThis.__TAURI_INTERNALS__;
      const oldIsTauri = globalThis.isTauri;
      globalThis.window = globalThis;
      globalThis.__TAURI_INTERNALS__ = {};
      globalThis.isTauri = true;
      const calls = [];
      try {
        mockIPC((command, args) => {
          assert.equal(command, 'core_request');
          calls.push(args.request.type);
          throw new Error('synthetic lost reply');
        });
        await assert.rejects(setup.saveCloud('llm', 'synthetic-test-key'), /synthetic lost reply/);
        const html = render({ ...queue, paused: true, blocked_reason: 'provider_http_402' });
        assert.equal(status(html), '配置结果待确认');
        assert.match(html, />确认配置<\/button>/);
        assert.doesNotMatch(html, /已配置|充值后|在智能分析中继续/);
        assert.deepEqual(calls, ['configure_cloud']);
      } finally {
        mockIPC(() => ({ type: 'cloud_configuration_receipt', id: 'synthetic', pending: false, response: null }));
        await setup.confirmCloud('llm');
        clearMocks();
        if (oldWindow === undefined) delete globalThis.window;
        else globalThis.window = oldWindow;
        if (oldInternals === undefined) delete globalThis.__TAURI_INTERNALS__;
        else globalThis.__TAURI_INTERNALS__ = oldInternals;
        if (oldIsTauri === undefined) delete globalThis.isTauri;
        else globalThis.isTauri = oldIsTauri;
      }
    });
    assert.equal(refreshes, 0, 'SSR must only project the supplied snapshots');
  } finally {
    await server.close();
  }
});
