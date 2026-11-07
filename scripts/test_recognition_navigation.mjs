import assert from 'node:assert/strict';
import { test } from 'node:test';
import { registerHooks, createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const { JSDOM } = createRequire(import.meta.url)('jsdom');
const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost/', pretendToBeVisual: true });
for (const name of ['window', 'document', 'HTMLElement', 'HTMLButtonElement', 'HTMLInputElement', 'HTMLDialogElement', 'Event', 'MouseEvent', 'MutationObserver']) globalThis[name] = dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.ResizeObserver = class { observe() {} disconnect() {} };
globalThis.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
globalThis.cancelAnimationFrame = dom.window.cancelAnimationFrame.bind(dom.window);
window.isTauri = true;
globalThis.isTauri = true;
const scrolled = [];
HTMLElement.prototype.scrollIntoView = function (options) { scrolled.push({ element: this, options }); };
HTMLDialogElement.prototype.showModal = function () { this.open = true; };
HTMLDialogElement.prototype.close = function () { this.open = false; };
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
    if (/\.tsx?$/.test(url)) return {
      format: 'module', shortCircuit: true,
      source: ts.transpileModule(readFileSync(fileURLToPath(url), 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, jsx: ts.JsxEmit.ReactJSX } }).outputText,
    };
    return next(url, context);
  },
});
const { default: React, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
const { default: App } = await import('../src/App.tsx');
const { QueueView, SettingsView } = await import('../src/WorkspaceViews.tsx');
const { recognitionServices } = await import('../src/recognition-services.ts');
const h = React.createElement;
const settle = async (fn = () => {}) => act(async () => { await fn(); await new Promise(resolve => setTimeout(resolve, 0)); });
const select = label => document.querySelector(`button[aria-label="${label}"]`);
const click = async element => { assert(element && !element.disabled, 'enabled button exists'); await settle(() => element.click()); };
const baseQueue = { provider: 'llm', configured: true, paused: false, blocked_reason: null, pending: 72, processing: 2, failed: 3 };
const queues = [baseQueue, { ...baseQueue, provider: 'asr', paused: true }];
const state = { revision: 1, enabled_intent: true, paused_intent: true, microphone_enabled: true, system_enabled: true, microphone_phase: 'idle', system_phase: 'idle', storage_paused: false };
const series = { requests: 2, count: 2, input_tokens: 1200, output_tokens: 80, cached_tokens: 600, cost_nano: 100000, incomplete: 0 };
const precise = { id: 'new-segment', source: 'microphone', started_ms: 1000, ended_ms: 5000, text: '合成候选句段' };
const requests = [];
let cloudReadMode = 'ready';
let cloudReleases = [];
let partialTasks = [], partialReviews = [];
const workflowConfig = {
  mode: 'single',
  tags: [
    { id: 'work', name: '工作会议', criteria: '讨论项目进展' },
    { id: 'client', name: '客户回访', criteria: '客户反馈' },
    { id: 'distinct', name: '\uFEFF工作会议', criteria: '\uFEFF' },
  ],
  auto_asr: { mode: 'tags', tag_ids: ['work'] },
  strategies: [
    { id: 'minutes', name: '会议纪要', input: 'precise', prompt: '列出会议决策', tag_ids: ['work'] },
    { id: 'asr-name', name: '自动精转', input: 'coarse', prompt: '列出待办', tag_ids: ['work'] },
  ],
};
mockIPC((command, args) => {
  if (command === 'onboarding_status') return { completed: true };
  if (command === 'entry_snapshot') return { sequence: 1, state, error: null, mode: 'top', appearance: 'sleeping', control: 'resume', detail: null, notice: null };
  if (command === 'privacy_status') return { microphone: 3, screen_and_system_audio: 1, tap_required: false, tap_ready: true };
  if (command === 'login_item_status') return 1;
  if (command !== 'core_request') return null;
  const r = args.request;
  requests.push(r);
  if (r.type === 'workflow') {
    const { op, config, add_required_asr_tags } = r.action;
    if (op === 'config_show') return { result: { ok: true, data: { revision: 3, config: workflowConfig, other_tag: 'other' } } };
    assert.equal(op, 'config_set');
    if (!add_required_asr_tags) return { result: { ok: false, error: { code: 'asr_rule_conflict', details: { affected: [{ strategy_id: 'minutes', name: '会议纪要', tag_ids: ['client'] }] } } } };
    return { result: { ok: true, data: { revision: 4, config: { ...config, auto_asr: { mode: 'tags', tag_ids: ['work', 'client'] } }, other_tag: 'other' } } };
  }
  if (r.type === 'cloud_status') {
    if (cloudReadMode === 'failure') throw Error('合成队列读取失败');
    if (cloudReadMode === 'loading') return new Promise(resolve => cloudReleases.push(() => resolve({ type: r.type, queues })));
    return { type: r.type, queues: cloudReadMode === 'empty' ? [] : queues };
  }
  if (r.type === 'cloud_tasks') return { type: r.type, tasks: partialTasks.filter(task => task.provider === r.provider) };
  if (r.type === 'review_items') return { type: r.type, items: partialReviews };
  if (r.type === 'calendar_days') return { type: r.type, days: [] };
  if (r.type === 'calendar_adjacent') return { type: r.type, newer_ms: null, older_ms: null };
  if (r.type === 'moments') return { type: r.type, moments: [] };
  if (r.type === 'people') return { type: r.type, people: [] };
  if (r.type === 'models') return { type: r.type, state: { phase: 'ready', speaker_phase: 'ready', progress_percent: 100 } };
  if (r.type === 'timeline') return { type: r.type, items: [], gaps: [] };
  if (r.type === 'range_detail') return { type: 'event', event: { precise_segments: [precise], coarse_segments: [] } };
  if (r.type === 'usage_statistics') return { type: r.type, statistics: { started_ms: r.boundaries[0], currency: 'CNY', days: r.boundaries.slice(0, -1).map(started_ms => ({ started_ms, covered: true, partial: false, asr: series, llm: series })) } };
  return { type: r.type, tasks: [], items: [] };
}, { shouldMockEvents: true });
const root = createRoot(document.getElementById('root'));
const settingsProps = {
  date: new Date(2026, 10, 1), state: null, queues, selection: null, privacy: null, loginStatus: null, models: null, busy: false,
  onStatus: async () => {}, onContext: async () => {}, onTimeline: async () => {}, onMutate: async () => { assert.fail('navigation must not mutate'); }, onMessage: () => {},
};

test('recognition settings navigation uses existing pages, focuses the service row and preserves queue controls', async t => {
  try {
    await t.test('real App card buttons reach both named rows repeatedly without opening or verifying keys', async () => {
      await settle(() => root.render(h(App)));
      assert.equal(select('智能识别').getAttribute('title'), '智能识别');
      for (const provider of ['llm', 'asr', 'llm']) {
        await click(select('智能识别'));
        const service = recognitionServices[provider];
        assert.equal(document.querySelector('h1').textContent, '智能识别');
        const card = [...document.querySelectorAll('.recognition-card')].find(element => element.querySelector('h2').textContent === service.title);
        assert.equal(card.querySelector('p').textContent, service.description);
        assert.doesNotMatch(card.textContent, /运行中|已暂停/);
        const settingsButton = select(`${service.title}设置`);
        assert.equal(settingsButton.tagName, 'BUTTON');
        assert.equal(settingsButton.tabIndex, 0, 'native button remains reachable by keyboard');
        assert.equal(settingsButton.previousElementSibling.getAttribute('role'), 'switch', 'switch precedes settings');
        assert.equal(settingsButton.textContent, '', 'settings uses an icon without a visible text label');
        assert.ok(settingsButton.querySelector('svg'), 'settings icon is present');
        const before = scrolled.length;
        await click(settingsButton);
        const row = document.querySelector(`[data-cloud-provider="${provider}"]`);
        assert.equal(document.activeElement, row);
        assert.equal(row.tabIndex, -1, 'focus location does not add a redundant tab stop');
        assert.equal(document.getElementById(row.getAttribute('aria-labelledby')).textContent, service.title);
        assert.equal(document.getElementById(row.getAttribute('aria-describedby')).textContent, service.description);
        assert.equal(scrolled.length, before + 1);
        assert.equal(scrolled.at(-1).element, row);
        assert.deepEqual(scrolled.at(-1).options, { block: 'center' });
        assert(!document.querySelector('dialog[open], input[type=password]'));
      }
      assert(!requests.some(r => ['configure_cloud', 'cloud_configuration_receipt', 'set_cloud_paused'].includes(r.type)));
      await click(select('时间线'));
      const before = scrolled.length;
      await click(select('设置'));
      assert.equal(scrolled.length, before, 'ordinary settings entry clears previous service location');
      assert(!document.activeElement?.dataset.cloudProvider);
    });

    await t.test('App pages own independent scroll containers while same-page refresh preserves the container and focus', async () => {
      await click(select('智能识别'));
      const queueMain = document.querySelector('main');
      queueMain.scrollTop = 420;
      const refresh = queueMain.querySelector('.page-heading button');
      const before = scrolled.length;
      await click(refresh);
      assert.equal(document.querySelector('main'), queueMain);
      assert.equal(queueMain.scrollTop, 420);
      assert.equal(document.activeElement, refresh);
      assert.equal(scrolled.length, before, 'queue refresh must not refocus or scroll the page');
      await click(select('事件分析设置'));
      const settingsMain = document.querySelector('main');
      assert.notEqual(settingsMain, queueMain);
      assert.equal(queueMain.isConnected, false);
      assert.equal(document.activeElement.dataset.cloudProvider, 'llm');
      settingsMain.scrollTop = 800;
      await click(select('智能识别'));
      const returnedMain = document.querySelector('main');
      assert.notEqual(returnedMain, settingsMain);
      assert.equal(returnedMain.scrollTop, 0);
      assert.equal(document.activeElement, document.querySelector('h1'));
    });

    await t.test('unread settings retain the target until its row mounts, then avoid repeated focus during refresh', async () => {
      await settle(() => root.render(h(SettingsView, { ...settingsProps, queuesReady: false, cloudProvider: 'asr' })));
      assert(!document.querySelector('[data-cloud-provider]'));
      const before = scrolled.length;
      await settle(() => root.render(h(SettingsView, { ...settingsProps, queuesReady: true, cloudProvider: 'asr' })));
      assert.equal(document.activeElement.dataset.cloudProvider, 'asr');
      assert.equal(scrolled.length, before + 1);
      await settle(() => root.render(h(SettingsView, { ...settingsProps, queues: queues.map(q => ({ ...q, pending: 90 })), cloudProvider: 'asr' })));
      assert.equal(scrolled.length, before + 1, 'snapshot refresh must not steal focus');
      await settle(() => root.render(h(SettingsView, { ...settingsProps, cloudProvider: 'llm' })));
      assert.equal(document.activeElement.dataset.cloudProvider, 'llm');
    });

    await t.test('necessary warnings, counts and original pause requests survive every card state', async () => {
      for (const provider of ['llm', 'asr']) {
        for (const [changes, warning] of [
          [{}, null], [{ paused: true }, null], [{ configured: false }, '尚未配置'],
          [{ blocked_reason: 'provider_http_402', paused: true }, '余额不足'], [{ blocked_reason: 'future_block' }, '处理受阻'],
        ]) {
          const queue = { ...baseQueue, provider, ...changes };
          const mutations = [], locations = [];
          await settle(() => root.render(h(QueueView, {
            queues: [queue], tasks: [], reviews: [], busy: false, onRefresh: async () => {},
            onSettings: value => locations.push(value), onMutate: async value => { mutations.push(value); return true; },
          })));
          const card = [...document.querySelectorAll('.recognition-card')].find(element => element.querySelector('h2').textContent === recognitionServices[provider].title);
          assert.doesNotMatch(card.textContent, /运行中|已暂停/);
          assert.equal(card.querySelector('.queue-blocked')?.textContent.includes(warning) ?? null, warning ? true : null);
          assert.deepEqual([...card.querySelectorAll('.queue-counts span')].map(element => element.firstChild.textContent), ['72', '2', '3']);
          const control = card.querySelector('[role=switch]');
          assert.equal(control.getAttribute('aria-checked'), String(queue.configured && !queue.paused && !queue.blocked_reason));
          assert.equal(control.disabled, !queue.configured);
          await click(select(`${recognitionServices[provider].title}设置`));
          assert.deepEqual(locations, [provider]);
          assert.equal(mutations.length, 0);
          if (queue.configured) {
            await click(control);
            assert.deepEqual(mutations, [{ type: 'set_cloud_paused', provider, paused: !queue.paused && !queue.blocked_reason }]);
          }
        }
      }
    });

    await t.test('provider task pages preserve statistics state, filter tasks and retain retry and review actions', async () => {
      const mutations = [];
      let refreshes = 0;
      const task = { target_kind: 'event', target_id: 'synthetic', source: 'microphone', started_ms: 1000, ended_ms: 5000, state: 'failed', manual: false };
      const review = { id: 'text-review', kind: 'text', original_segment_id: 'old-segment', source: 'microphone', started_ms: 1000, ended_ms: 5000, text: '合成文字修正', person_id: null, person_name: null, revision: 4 };
      const props = {
        queues, tasks: [{ ...task, id: 'llm-task', provider: 'llm', error_code: 'provider_transport' }, { ...task, id: 'asr-task', provider: 'asr', error_code: 'provider_invalid_utterance' }],
        reviews: [review, { ...review, id: 'speaker-review', kind: 'speaker', text: null, person_id: 'synthetic-person', person_name: '合成人物' }],
        busy: false, onSettings() {}, onRefresh: async () => { refreshes++; }, onMutate: async r => { mutations.push(r); return true; },
      };
      await settle(() => root.render(h(QueueView, props)));
      assert.equal(document.querySelector('[role=tab][aria-selected=true]').textContent, '处理队列');
      assert(!document.querySelector('.task-row'));
      assert.equal(document.querySelectorAll('.usage-chart').length, 2);
      assert.match(select('语音识别任务明细').parentElement.textContent, /2 条待核对/);
      const statistics = document.querySelector('.usage-statistics');
      const overview = document.querySelector('.recognition-statistics').parentElement;
      const range = [...statistics.querySelectorAll('.usage-range button')].find(button => button.textContent === '近 7 天');
      await click(range);
      const legend = statistics.querySelector('.usage-legend button');
      await click(legend);
      assert.equal(legend.getAttribute('aria-pressed'), 'false');
      const reads = requests.filter(r => r.type === 'usage_statistics').length;
      await click(select('事件分析任务明细'));
      assert(!document.querySelector('[role=tablist]'));
      assert.equal(document.querySelector('h1').textContent, '事件分析任务明细');
      assert.equal(document.activeElement, document.querySelector('h1'));
      assert.equal(scrolled.at(-1).element, document.querySelector('.recognition-page'));
      assert.deepEqual(scrolled.at(-1).options, { block: 'start' });
      assert.equal(overview.hidden, true);
      assert.equal(overview.hasAttribute('inert'), true, 'hidden primary controls must be excluded from focus and accessibility');
      assert.equal(document.querySelector('.usage-statistics'), statistics);
      assert.equal(document.querySelectorAll('.task-row').length, 1);
      assert.match(document.querySelector('.task-row').textContent, /事件分析/);
      assert(!document.querySelector('.review-row'));
      await click(document.querySelector('.task-row button'));
      assert.deepEqual(mutations.pop(), { type: 'retry_cloud_task', task_id: 'llm-task' });
      await click(document.querySelector('.recognition-back'));
      assert.equal(document.querySelector('h1').textContent, '智能识别');
      assert.equal(document.activeElement, document.querySelector('h1'));
      assert.equal(overview.hidden, false);
      assert.equal(overview.hasAttribute('inert'), false);
      assert.equal(document.querySelector('.usage-statistics'), statistics);
      assert.equal(range.getAttribute('aria-pressed'), 'true');
      assert.equal(legend.getAttribute('aria-pressed'), 'false');
      assert.equal(requests.filter(r => r.type === 'usage_statistics').length, reads, 'task navigation retains the statistics instance and request owner');
      await click(select('语音识别任务明细'));
      assert.equal(document.querySelectorAll('.task-row').length, 1);
      assert.match(document.querySelector('.task-row').textContent, /语音识别/);
      assert.match(document.querySelector('.task-row').textContent, /产生新的识别用量/);
      assert.equal(document.querySelector('.task-row button').textContent, '重新识别');
      await click(document.querySelector('.task-row button'));
      assert.deepEqual(mutations.pop(), { type: 'retry_cloud_task', task_id: 'asr-task' });
      assert.equal(document.querySelectorAll('.review-row').length, 2);
      await click(document.querySelector('.review-row button'));
      const candidate = [...document.querySelectorAll('.review-candidates button')].find(button => button.textContent.includes('合成候选句段'));
      await click(candidate);
      assert.deepEqual(mutations.pop(), { type: 'resolve_review', item_id: 'text-review', target_segment_id: 'new-segment', expected_revision: 4 });
      await click(document.querySelectorAll('.review-row')[1].querySelector('button'));
      await click(document.querySelector('.review-dismiss'));
      assert.deepEqual(mutations.pop(), { type: 'resolve_review', item_id: 'speaker-review', target_segment_id: null, expected_revision: 4 });
      assert.equal(refreshes, 2, 'successful review actions retain their existing refresh');
      await settle(() => root.render(h(QueueView, { ...props, busy: true })));
      assert.equal(document.querySelector('.task-row button').disabled, true);
      assert.equal(document.querySelector('.recognition-back').disabled, false);
      await settle(() => root.render(h(QueueView, { ...props, tasksReady: false, reviewsReady: false })));
      assert(document.querySelector('[aria-label="正在加载任务"]'));
      assert(document.querySelector('[aria-label="正在加载待核对记录"]'));
      assert(!document.querySelector('.task-row, .review-row, .list-empty'));
      await settle(() => root.render(h(QueueView, { ...props, tasksReady: false, tasksError: '合成任务错误', reviewsReady: false, reviewsError: '合成核对错误' })));
      assert.equal(document.querySelectorAll('.detail-load-error').length, 2);
      assert(!document.querySelector('.task-row, .review-row, .list-empty'));
    });

    await t.test('workflow subpages share one draft and resolve precise input dependencies through Core', async () => {
      await settle(() => root.render(h(SettingsView, settingsProps)));
      assert.doesNotMatch(document.querySelector('.page-container').textContent, /标签库|生成策略/);
      const reads = () => requests.filter(r => r.action?.op === 'config_show').length;
      const readCount = reads();
      await settle(() => root.render(h(QueueView, {
        queues, tasks: [], reviews: [], busy: false, onRefresh: async () => {}, onSettings() {}, onMutate: async () => true,
      })));
      const tab = name => [...document.querySelectorAll('[role=tab]')].find(button => button.textContent === name);
      const button = (name, scope = document) => [...scope.querySelectorAll('button')].find(item => (item.querySelector('.button-labels > span:not([aria-hidden=true])')?.textContent ?? item.textContent) === name);
      assert.equal(reads(), readCount);
      for (const item of document.querySelectorAll('[role=tab]')) assert(document.getElementById(item.getAttribute('aria-controls')));
      await click(tab('智能标签'));
      assert.equal(reads(), readCount + 1);
      assert.match(document.querySelector('table[aria-label="标签库"]').textContent, /判断标准.*系统兜底/s);
      await click(document.querySelector('input[value="multiple"]'));
      assert.match(document.querySelector('[role=status]').textContent, /新标注方式适用于后续事件/);
      await click(select('编辑标签 · 工作会议'));
      const labelEditor = document.querySelector('dialog');
      assert.equal(labelEditor.querySelector('textarea').value, '讨论项目进展');
      await click(button('去自动流程调整', labelEditor));
      await settle(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(document.activeElement, select('编辑自动精转'));
      await click(tab('智能标签'));
      await click(select('编辑标签 · \uFEFF工作会议'));
      await click(button('应用到草稿', document.querySelector('dialog')));
      await click(tab('处理队列'));
      await click(tab('自动流程'));
      assert.equal(reads(), readCount + 1);
      assert.match(document.querySelector('[role=status]').textContent, /未保存的改动/);
      const flows = document.querySelector('table[aria-label="自动流程"]');
      const asrRow = flows.querySelector('tbody tr:first-child');
      assert.match(asrRow.textContent, /自动精转\s*官方.*事件音频.*工作会议/s);
      assert(!flows.querySelector('input'), 'workflow list only shows summaries');
      await click(select('编辑自动精转'));
      const autoAsr = document.querySelector('dialog');
      assert.equal(autoAsr.querySelector('input[value="tags"]').checked, true);
      assert.equal(document.activeElement, autoAsr.querySelector('input[value="tags"]'));
      assert.doesNotMatch(autoAsr.textContent, /移除/);
      await click(autoAsr.querySelector('input[value="all"]'));
      await click(button('取消', autoAsr));
      assert.match(asrRow.textContent, /工作会议/);
      await click(select('编辑自动精转'));
      const asrEditor = document.querySelector('dialog');
      await click(asrEditor.querySelector('input[value="all"]'));
      await click(button('应用到草稿', asrEditor));
      assert.match(asrRow.textContent, /全部有效事件/);
      assert.doesNotMatch(asrRow.textContent, /工作会议/);
      await click(select('编辑自动精转'));
      const tagsEditor = document.querySelector('dialog');
      await click(tagsEditor.querySelector('input[value="tags"]'));
      assert.equal(tagsEditor.querySelectorAll('input[type=checkbox]:checked').length, 0);
      await click([...tagsEditor.querySelectorAll('label')].find(item => item.textContent === '工作会议').querySelector('input'));
      await click(button('应用到草稿', tagsEditor));
      await click(select('编辑策略 · 会议纪要'));
      const strategyEditor = document.querySelector('dialog');
      assert.equal(strategyEditor.querySelector('select').value, 'precise');
      await click([...strategyEditor.querySelectorAll('label')].find(item => item.textContent === '客户回访').querySelector('input'));
      await click(button('应用到草稿', strategyEditor));
      await click(button('保存配置'));
      assert.match(document.querySelector('dialog').textContent, /会议纪要 · 客户回访/);
      await click(button('去自动流程调整', document.querySelector('dialog')));
      await settle(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(document.activeElement, select('编辑策略 · 会议纪要'));
      await click(button('保存配置'));
      await click(button('加入并保存', document.querySelector('dialog')));
      assert.match(document.querySelector('[role=status]').textContent, /已保存/);
      const writes = requests.filter(r => r.action?.op === 'config_set').map(r => r.action);
      assert.deepEqual(writes.map(w => [w.expected_revision, w.config.mode, w.add_required_asr_tags]), [[3, 'multiple', false], [3, 'multiple', false], [3, 'multiple', true]]);
      assert.deepEqual(writes.at(-1).config.strategies[0].tag_ids, ['work', 'client']);
      assert.deepEqual(writes.at(-1).config.tags, workflowConfig.tags);
      assert.deepEqual([...asrRow.querySelectorAll('.workflow-chip')].map(tag => tag.textContent), ['工作会议', '客户回访']);
      await click(select('编辑自动精转'));
      assert.deepEqual([...document.querySelectorAll('dialog input[type=checkbox]:checked')].map(input => input.parentElement.textContent), ['工作会议', '客户回访']);
      await click(button('取消', document.querySelector('dialog')));
    });

    await t.test('QueueView task and review actions remain reachable before queue status succeeds or when it returns no providers', async () => {
      const task = { target_kind: 'event', target_id: 'synthetic', source: 'microphone', started_ms: 1000, ended_ms: 5000, state: 'failed', error_code: 'provider_transport', manual: false };
      const review = { id: 'independent-review', kind: 'text', original_segment_id: 'old-segment', source: 'microphone', started_ms: 1000, ended_ms: 5000, text: '独立读取的文字修正', person_id: null, person_name: null, revision: 8 };
      for (const mode of ['loading', 'failure', 'empty']) {
        const mutations = [], locations = [];
        await settle(() => root.render(h(QueueView, {
          key: mode, queues: [], queuesReady: mode === 'empty', queuesError: mode === 'failure' ? '合成队列错误' : undefined,
          tasks: [{ ...task, provider: 'llm', id: 'independent-llm' }, { ...task, provider: 'asr', id: 'independent-asr' }], tasksReady: true,
          reviews: [review], reviewsReady: true, busy: false, onRefresh: async () => {},
          onSettings: provider => locations.push(provider), onMutate: async r => { mutations.push(r); return true; },
        })));
        assert.equal(document.querySelectorAll('.recognition-card').length, 2);
        assert.equal(document.querySelectorAll('.recognition-task-link button').length, 2);
        assert(!document.querySelector('[role=switch], .queue-counts > span, .queue-blocked'), 'unknown status cannot claim OFF, zero counts or unconfigured');
        if (mode === 'loading') assert(document.querySelector('[aria-label="正在加载队列状态"]'));
        else assert.equal(document.querySelectorAll('.recognition-card [role=alert]').length, 2);
        await click(select('事件分析设置'));
        assert.deepEqual(locations, ['llm']);
        await click(select('事件分析任务明细'));
        assert.equal(document.querySelectorAll('.task-row').length, 1);
        assert.match(document.querySelector('.task-row').textContent, /事件分析/);
        await click(document.querySelector('.task-row button'));
        assert.deepEqual(mutations.pop(), { type: 'retry_cloud_task', task_id: 'independent-llm' });
        await click(document.querySelector('.recognition-back'));
        await click(select('语音识别任务明细'));
        assert.equal(document.querySelectorAll('.task-row').length, 1);
        assert.match(document.querySelector('.task-row').textContent, /语音识别/);
        await click(document.querySelector('.review-row button'));
        await click([...document.querySelectorAll('.review-candidates button')].find(button => button.textContent.includes(precise.text)));
        assert.deepEqual(mutations.pop(), { type: 'resolve_review', item_id: review.id, target_segment_id: precise.id, expected_revision: 8 });
      }
    });

    await t.test('real App exposes independently loaded task retry and review resolution during an initial queue failure or held read', async () => {
      const task = { target_kind: 'event', target_id: 'synthetic', source: 'microphone', started_ms: 1000, ended_ms: 5000, state: 'failed', error_code: 'provider_transport', manual: false };
      partialTasks = [{ ...task, id: 'app-partial-llm', provider: 'llm' }, { ...task, id: 'app-partial-asr', provider: 'asr' }];
      partialReviews = [{ id: 'app-partial-review', kind: 'text', original_segment_id: 'old-segment', source: 'microphone', started_ms: 1000, ended_ms: 5000, text: '合成独立核对记录', person_id: null, person_name: null, revision: 5 }];
      try {
        for (const mode of ['failure', 'loading']) {
          cloudReadMode = mode;
          const before = requests.length;
          await settle(() => root.render(h(App, { key: `partial-${mode}` })));
          await click(select('智能识别'));
          assert.equal(document.querySelectorAll('.recognition-task-link button').length, 2);
          assert(!document.querySelector('[role=switch], .queue-counts > span, .queue-blocked'));
          if (mode === 'failure') assert.equal(document.querySelectorAll('.recognition-card [role=alert]').length, 2);
          else assert(document.querySelector('[aria-label="正在加载队列状态"]'));
          assert.match(select('语音识别任务明细').parentElement.textContent, /1 条待核对/);
          await click(select('事件分析任务明细'));
          assert.equal(document.querySelectorAll('.task-row').length, 1);
          assert.match(document.querySelector('.task-row').textContent, /事件分析/);
          await click(document.querySelector('.task-row button'));
          assert(requests.slice(before).some(r => r.type === 'retry_cloud_task' && r.task_id === 'app-partial-llm'));
          await click(document.querySelector('.recognition-back'));
          await click(select('语音识别任务明细'));
          assert.equal(document.querySelectorAll('.task-row').length, 1);
          assert.match(document.querySelector('.task-row').textContent, /语音识别/);
          await click(document.querySelector('.review-row button'));
          await click([...document.querySelectorAll('.review-candidates button')].find(button => button.textContent.includes(precise.text)));
          assert(requests.slice(before).some(r => r.type === 'resolve_review' && r.item_id === 'app-partial-review' && r.target_segment_id === precise.id && r.expected_revision === 5));
          assert(!requests.slice(before).some(r => ['set_cloud_paused', 'configure_cloud'].includes(r.type)));
          cloudReadMode = 'ready';
          await settle(() => { for (const release of cloudReleases.splice(0)) release(); });
        }
      } finally {
        cloudReadMode = 'ready';
        partialTasks = []; partialReviews = [];
        await settle(() => { for (const release of cloudReleases.splice(0)) release(); });
      }
    });
  } finally {
    await settle(() => root.unmount());
    clearMocks();
    dom.window.close();
  }
});
