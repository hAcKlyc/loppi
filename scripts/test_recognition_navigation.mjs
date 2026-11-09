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
let timelineItems = [];
let workflowConfig = {
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
let workflowRevision = 3;
let workflowFailure = null;
let holdWorkflowWrite = false;
let releaseWorkflowWrite;
let loseWorkflowReply = false;
const workflowWrites = [];
const workflowReceipts = new Map();
const workflowError = (code, details) => ({ result: { ok: false, error: { code, details } } });
const workflowSnapshot = () => ({ result: { ok: true, data: { revision: workflowRevision, config: workflowConfig, other_tag: 'other' } } });
mockIPC((command, args) => {
  if (command === 'onboarding_status') return { completed: true };
  if (command === 'entry_snapshot') return { sequence: 1, state, error: null, mode: 'top', appearance: 'sleeping', control: 'resume', detail: null, notice: null };
  if (command === 'privacy_status') return { microphone: 3, screen_and_system_audio: 1, tap_required: false, tap_ready: true };
  if (command === 'login_item_status') return 1;
  if (command !== 'core_request') return null;
  const r = args.request;
  requests.push(r);
  if (r.type === 'workflow') {
    const { op, config, expected_revision, add_required_asr_tags } = r.action;
    if (op === 'config_show') return workflowSnapshot();
    if (op === 'strategies') return { result: { ok: true, data: { revision: 3, strategies: [] } } };
    if (op === 'runs') return { result: { ok: true, data: { runs: [] } } };
    assert.equal(op, 'config_set');
    const { requestId } = args;
    workflowWrites.push({ action: r.action, requestId });
    const finish = () => {
      if (workflowReceipts.has(requestId)) return workflowReceipts.get(requestId);
      if (expected_revision !== workflowRevision) return workflowError('revision_conflict');
      if (workflowFailure) {
        const error = workflowFailure;
        workflowFailure = null;
        return error;
      }
      workflowRevision++;
      const withId = item => item.id.startsWith('00000000-') ? { ...item, id: 'created-' + workflowRevision } : item;
      workflowConfig = { ...config, tags: config.tags.map(withId), strategies: config.strategies.map(withId) };
      if (add_required_asr_tags) workflowConfig.auto_asr = { mode: 'tags', tag_ids: ['work', 'client'] };
      const result = workflowSnapshot();
      workflowReceipts.set(requestId, result);
      if (loseWorkflowReply) {
        loseWorkflowReply = false;
        throw Error('合成设置响应丢失');
      }
      return result;
    };
    if (holdWorkflowWrite) return new Promise(resolve => { releaseWorkflowWrite = () => resolve(finish()); });
    return finish();
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
  if (r.type === 'timeline') return { type: r.type, items: timelineItems, gaps: [] };
  if (r.type === 'event_detail') return { type: 'event', event: timelineItems.find(item => item.id === r.event_id) };
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
    await t.test('event dialogs manage strategies and scene labels directly without saving', async () => {
      const start = new Date().setHours(0, 0, 0, 0);
      timelineItems = [{
        id: 'management-fixture', title: '合成管理入口', started_ms: start, ended_ms: start + 1000,
        sources: 'microphone', revision: 1, fit_state: 'complete', completed_ms: start + 1000,
        processing_state: 'transcribed', recording_sources: [], coarse_segments: [], precise_segments: [],
        workflow: { phase: 'decided', tag_state: 'unclassified', labels: [], manual: false },
      }];
      try {
        await settle(() => root.render(h(App, { key: 'workflow-management' })));
        for (const [opener, title, destination] of [
          ['执行策略', '执行策略', '流程与策略'],
          ['编辑标签 · 添加标签', '编辑事件标签', '事件分析·场景标签'],
        ]) {
          const before = requests.length;
          await click(document.querySelector('.event-card'));
          await click(select(opener));
          const dialog = document.querySelector('dialog.modal[open]');
          assert.equal(dialog.querySelector('.modal-title h2').textContent, title);
          const manage = dialog.querySelector('.modal-header-action button');
          assert.equal(manage.textContent, '管理');
          if (title === '执行策略') assert.match(dialog.textContent, /添加策略/);
          await click(manage);
          assert.equal(document.querySelector('.recognition-tabs [aria-selected=true]').textContent, destination);
          assert(!document.querySelector('dialog[open]'));
          assert(document.activeElement === document.querySelector('h1'), 'destination heading receives focus, got ' + document.activeElement?.tagName);
          assert(requests.slice(before).filter(r => r.type === 'workflow').every(r => ['runs', 'strategies', 'config_show'].includes(r.action.op)));
          await click(select('时间线'));
        }
        await click(select('智能分析'));
        assert.equal(document.querySelector('.recognition-tabs [aria-selected=true]').textContent, '处理队列');
      } finally {
        timelineItems = [];
      }
    });

    await t.test('real App card buttons reach both named rows repeatedly without opening or verifying keys', async () => {
      await settle(() => root.render(h(App)));
      assert.equal(select('智能分析').getAttribute('title'), '智能分析');
      for (const provider of ['llm', 'asr', 'llm']) {
        await click(select('智能分析'));
        const service = recognitionServices[provider];
        assert.equal(document.querySelector('h1').textContent, '智能分析');
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

    await t.test('returning to the queue reloads live data without replacing the page or stealing focus', async () => {
      const beforeEntry = requests.length;
      await click(select('智能分析'));
      const queueMain = document.querySelector('main');
      assert(requests.slice(beforeEntry).some(r => r.type === 'cloud_status'));
      assert(!queueMain.querySelector('.page-heading button'));
      const [queueTab, labelsTab, automationTab] = queueMain.querySelectorAll('[role=tab]');
      assert.deepEqual([queueTab, labelsTab, automationTab].map(item => item.textContent), ['处理队列', '事件分析·场景标签', '流程与策略']);
      const statistics = queueMain.querySelector('.usage-statistics');
      const range = [...statistics.querySelectorAll('.usage-range button')].find(button => button.textContent === '近 7 天');
      await click(range);
      await click(labelsTab);
      queueMain.scrollTop = 420;
      const beforeRefresh = requests.length;
      const before = scrolled.length;
      await settle(() => queueTab.focus());
      await click(queueTab);
      const loaded = requests.slice(beforeRefresh).map(r => r.type);
      for (const type of ['cloud_status', 'cloud_tasks', 'review_items', 'usage_statistics']) assert(loaded.includes(type));
      assert.equal(queueMain.querySelector('.usage-statistics'), statistics);
      assert.equal(range.getAttribute('aria-pressed'), 'true');
      assert.equal(document.querySelector('main'), queueMain);
      assert.equal(queueMain.scrollTop, 420);
      assert.equal(document.activeElement, queueTab);
      assert.equal(scrolled.length, before, 'queue refresh must not refocus or scroll the page');
      const refreshed = requests.length;
      await click(queueTab);
      assert.equal(requests.length, refreshed, 'reselecting the active tab does not issue duplicate reads');
      await click(select('事件分析设置'));
      const settingsMain = document.querySelector('main');
      assert.notEqual(settingsMain, queueMain);
      assert.equal(queueMain.isConnected, false);
      assert.equal(document.activeElement.dataset.cloudProvider, 'llm');
      settingsMain.scrollTop = 800;
      await click(select('智能分析'));
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
      assert.match(select('语音识别·精转录任务明细').parentElement.textContent, /2 条待核对/);
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
      assert.equal(document.querySelector('h1').textContent, '智能分析');
      assert.equal(document.activeElement, document.querySelector('h1'));
      assert.equal(overview.hidden, false);
      assert.equal(overview.hasAttribute('inert'), false);
      assert.equal(document.querySelector('.usage-statistics'), statistics);
      assert.equal(range.getAttribute('aria-pressed'), 'true');
      assert.equal(legend.getAttribute('aria-pressed'), 'false');
      assert.equal(requests.filter(r => r.type === 'usage_statistics').length, reads, 'task navigation retains the statistics instance and request owner');
      await click(select('语音识别·精转录任务明细'));
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
      assert.equal(refreshes, 4, 'opening two task views and resolving two reviews refreshes each time');
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

    await t.test('workflow settings apply once, keep invalid changes in the editor and recover lost replies safely', async () => {
      await settle(() => root.render(h(SettingsView, settingsProps)));
      assert.doesNotMatch(document.querySelector('.page-container').textContent, /场景标签管理|生成策略/);
      const reads = () => requests.filter(r => r.action?.op === 'config_show').length;
      const readCount = reads();
      const props = { queues, tasks: [], reviews: [], busy: false, onRefresh: async () => {}, onSettings() {}, onMutate: async () => true };
      await settle(() => root.render(h(QueueView, props)));
      const tab = name => [...document.querySelectorAll('[role=tab]')].find(item => item.textContent === name);
      const button = (name, scope = document) => [...scope.querySelectorAll('button')].find(item => (item.querySelector('.button-labels > span:not([aria-hidden=true])')?.textContent ?? item.textContent) === name);
      const dialog = () => document.querySelector('dialog[open]');
      const save = () => click(button('保存', dialog()));
      const tag = name => [...dialog().querySelectorAll('label')].find(item => item.textContent === name).querySelector('input');
      const setValue = async (field, value) => settle(() => {
        Object.getOwnPropertyDescriptor(Object.getPrototypeOf(field), 'value').set.call(field, value);
        field.dispatchEvent(new Event(field.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
      });
      const assertApplied = () => {
        assert(!dialog());
        assert.equal(document.querySelector('.workflow-config-feedback [role=status]').textContent, '设置已生效');
        assert.doesNotMatch(document.querySelector('.workflow-settings').textContent, /未保存的改动|保存配置|确认设置/);
      };
      assert.equal(reads(), readCount);
      for (const item of document.querySelectorAll('[role=tab]')) assert(document.getElementById(item.getAttribute('aria-controls')));
      await click(tab('事件分析·场景标签'));
      assert.equal(reads(), readCount + 1);
      assert.match(document.querySelector('.workflow-config-rule').textContent, /每个事件支持智能打标签模式.*单选.*多选.*后续事件/s);
      const labelTable = document.querySelector('table.workflow-config-table');
      assert.equal(labelTable.caption.textContent, '场景标签管理');
      assert.match(labelTable.textContent, /判断标准.*系统兜底/s);

      holdWorkflowWrite = true;
      const beforeMode = workflowWrites.length;
      await click(document.querySelector('input[value="multiple"]'));
      assert.equal(workflowWrites.length, beforeMode + 1);
      assert.equal(workflowWrites.at(-1).action.expected_revision, 3);
      assert.equal(workflowWrites.at(-1).action.config.mode, 'multiple');
      assert(document.querySelector('input[value="single"]').checked, 'selection reflects the committed setting until the write completes');
      assert(select('编辑标签 · 工作会议').disabled);
      await click(tab('处理队列'));
      holdWorkflowWrite = false;
      await settle(releaseWorkflowWrite);
      await click(tab('事件分析·场景标签'));
      assertApplied();
      assert.equal(workflowConfig.mode, 'multiple');
      assert(document.querySelector('input[value="multiple"]').checked);
      assert.equal(reads(), readCount + 1);

      await click(select('编辑标签 · 工作会议'));
      assert.equal(dialog().querySelector('textarea').value, '讨论项目进展');
      await click(button('去流程与策略调整', dialog()));
      await settle(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(document.activeElement, select('编辑自动精转'));
      await click(tab('事件分析·场景标签'));
      await click(button('添加标签'));
      await setValue(dialog().querySelector('input'), '现场笔记');
      await setValue(dialog().querySelector('textarea'), '记录关键决定');
      await save();
      assertApplied();
      const createdTag = workflowConfig.tags.at(-1);
      assert.match(createdTag.id, /^created-/);
      await click(select('编辑标签 · 现场笔记'));
      await setValue(dialog().querySelector('textarea'), '记录关键决定与负责人');
      await save();
      assertApplied();
      assert.equal(workflowConfig.tags.at(-1).criteria, '记录关键决定与负责人');

      await click(tab('流程与策略'));
      const flows = document.querySelector('table[aria-label="流程与策略"]');
      const asrRow = flows.querySelector('tbody tr:first-child');
      assert.match(asrRow.textContent, /自动精转\s*官方.*事件音频.*工作会议/s);
      assert(!flows.querySelector('input'));
      await click(button('添加策略'));
      await setValue(dialog().querySelector('input'), '行动清单');
      await setValue(dialog().querySelector('textarea'), '列出负责人');
      await click(tag('现场笔记'));
      await save();
      assertApplied();
      assert.deepEqual(workflowConfig.strategies.at(-1).tag_ids, [createdTag.id]);
      await click(select('编辑策略 · 行动清单'));
      await click(tag('现场笔记'));
      await setValue(dialog().querySelector('select'), 'precise');
      await save();
      assertApplied();
      assert.equal(workflowConfig.strategies.at(-1).input, 'precise');
      assert.match(flows.textContent, /行动清单精转录仅手动/);
      await click(select('编辑策略 · 行动清单'));
      await click(button('移除策略', dialog()));
      assertApplied();
      assert(!select('编辑策略 · 行动清单'));

      await click(select('编辑自动精转'));
      assert.equal(document.activeElement, dialog().querySelector('input[value="tags"]'));
      const beforeCancel = workflowWrites.length;
      await click(dialog().querySelector('input[value="all"]'));
      await click(button('取消', dialog()));
      assert.equal(workflowWrites.length, beforeCancel);
      assert.match(asrRow.textContent, /工作会议/);
      await click(select('编辑自动精转'));
      await click(dialog().querySelector('input[value="all"]'));
      await save();
      assertApplied();
      assert.match(asrRow.textContent, /全部有效事件/);
      await click(select('编辑自动精转'));
      await click(dialog().querySelector('input[value="tags"]'));
      assert(button('保存', dialog()).disabled);
      await click(tag('工作会议'));
      await save();
      assertApplied();

      const missingClient = [{ strategy_id: 'minutes', name: '会议纪要', tag_ids: ['client'] }];
      await click(select('编辑策略 · 会议纪要'));
      await click(tag('客户回访'));
      workflowFailure = workflowError('asr_rule_conflict', { affected: missingClient });
      const beforeDependency = workflowRevision;
      await save();
      assert.match(dialog().textContent, /会议纪要 · 客户回访.*音频可能发送/s);
      assert(tag('客户回访').checked);
      assert.equal(workflowRevision, beforeDependency);
      assert.deepEqual(workflowConfig.auto_asr.tag_ids, ['work']);
      assert.deepEqual(workflowConfig.strategies[0].tag_ids, ['work']);
      await click(button('补齐精转标签并保存', dialog()));
      assertApplied();
      assert.deepEqual(workflowWrites.slice(-2).map(({ action }) => [action.expected_revision, action.add_required_asr_tags]), [[beforeDependency, false], [beforeDependency, true]]);
      assert.deepEqual(workflowConfig.strategies[0].tag_ids, ['work', 'client']);
      assert.deepEqual([...asrRow.querySelectorAll('.workflow-chip')].map(chip => chip.textContent), ['工作会议', '客户回访']);

      await click(select('编辑自动精转'));
      await click(tag('客户回访'));
      workflowFailure = workflowError('asr_rule_conflict', { affected: missingClient });
      await save();
      assert(!button('补齐精转标签并保存', dialog()));
      assert.deepEqual(workflowConfig.auto_asr.tag_ids, ['work', 'client']);
      await click(button('去流程与策略调整', dialog()));
      await settle(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(document.activeElement, select('编辑策略 · 会议纪要'));

      await click(tab('事件分析·场景标签'));
      await click(select('编辑标签 · 工作会议'));
      workflowFailure = workflowError('tag_rule_conflict', { affected: [{ kind: 'auto_asr', tag_ids: ['work'] }] });
      await click(button('移除标签', dialog()));
      assert.match(dialog().textContent, /仍引用该标签.*自动精转 · 工作会议/s);
      assert(select('编辑标签 · 工作会议'));
      await click(button('取消', dialog()));
      await click(select('编辑标签 · 现场笔记'));
      await click(button('移除标签', dialog()));
      assertApplied();
      assert(!select('编辑标签 · 现场笔记'));

      await click(select('编辑标签 · 工作会议'));
      await setValue(dialog().querySelector('input'), '客户回访');
      workflowFailure = workflowError('invalid_tag_definition');
      await save();
      assert.match(dialog().querySelector('[role=alert]').textContent, /各不相同/);
      assert.equal(dialog().querySelector('input').value, '客户回访');
      await setValue(dialog().querySelector('input'), '团队会议');
      loseWorkflowReply = true;
      await save();
      assert.match(dialog().querySelector('[role=alert]').textContent, /未收到设置结果/);
      assert(dialog().querySelector('input').disabled);
      const lost = workflowWrites.at(-1), committed = workflowRevision;
      await click(button('重试', dialog()));
      assertApplied();
      assert.deepEqual(workflowWrites.at(-1), lost);
      assert.equal(workflowRevision, committed, 'retry must reuse the receipt instead of applying a second edit');
      assert(select('编辑标签 · 团队会议'));

      workflowRevision++;
      workflowConfig = { ...workflowConfig, mode: 'single' };
      await click(document.querySelector('input[value="single"]'));
      assert.match(document.querySelector('.workflow-config-feedback [role=alert]').textContent, /设置已更新/);
      assert(document.querySelector('input[value="multiple"]').checked);
      await click(button('读取最新配置'));
      assert(document.querySelector('input[value="single"]').checked);
      await settle(() => root.render(h(QueueView, { ...props, key: 'reopen-workflow', initialSection: 'automation' })));
      assert.match(document.querySelector('table[aria-label="流程与策略"]').textContent, /团队会议.*客户回访/s);
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
        await click(select('语音识别·精转录任务明细'));
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
          await click(select('智能分析'));
          assert.equal(document.querySelectorAll('.recognition-task-link button').length, 2);
          assert(!document.querySelector('[role=switch], .queue-counts > span, .queue-blocked'));
          if (mode === 'failure') assert.equal(document.querySelectorAll('.recognition-card [role=alert]').length, 2);
          else assert(document.querySelector('[aria-label="正在加载队列状态"]'));
          assert.match(select('语音识别·精转录任务明细').parentElement.textContent, /1 条待核对/);
          await click(select('事件分析任务明细'));
          assert.equal(document.querySelectorAll('.task-row').length, 1);
          assert.match(document.querySelector('.task-row').textContent, /事件分析/);
          await click(document.querySelector('.task-row button'));
          assert(requests.slice(before).some(r => r.type === 'retry_cloud_task' && r.task_id === 'app-partial-llm'));
          await click(document.querySelector('.recognition-back'));
          await click(select('语音识别·精转录任务明细'));
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
