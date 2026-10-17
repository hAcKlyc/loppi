import assert from 'node:assert/strict';
import { test } from 'node:test';
import { daySpan, cardExcerpt } from '../src/timeline-view.ts';

test('day spans clip cross-day and future ranges without inflating short durations', () => {
  assert.deepEqual(daySpan(-100, 250, 0, 1000, 600), {left:'0%',width:'25%'});
  assert.deepEqual(daySpan(500, 1200, 0, 1000, 600), {left:'50%',width:'10%'});
  assert.deepEqual(daySpan(500, 501, 0, 1000, 600), {left:'50%',width:'0.1%'});
  assert.deepEqual(daySpan(500, 1200, 0, 1000, 2000), {left:'50%',width:'50%'});
  for (const [a,b] of [[600,700],[-100,0],[1000,1100],[400,400],[500,400],[NaN,100]]) assert.equal(daySpan(a,b,0,1000,600), null);
  assert.equal(daySpan(0,100,0,0,600),null);
});
const item={id:null,manual_range_id:null,preview:{text:'原始摘录',source:'microphone',is_precise:false}};
const note={text:'现场笔记',ocr_text:null,kind:'comment'};
test('hide coarse preserves moment clues without exposing hidden transcript', () => {
  assert.deepEqual(cardExcerpt(item,[note],true),{text:'现场笔记',label:'现场笔记'});
  assert.equal(cardExcerpt(item,[],false).label,'粗转 · 麦克风');
  assert.equal(cardExcerpt({...item,id:'event'},[note],true).text,'暂无内容摘要，打开查看转录');
  assert.equal(cardExcerpt({...item,manual_range_id:'range',preview:{text:'修正后的精转',source:'system',is_precise:true}},[],true).label,'精转 · 电脑声音');
});
test('empty and legacy summaries use honest placeholders; screenshot text remains bounded', () => {
  assert.equal(cardExcerpt({...item,preview:null},[],false).label,'');
  assert.match(cardExcerpt({...item,preview:undefined},[],false).text,/暂无文字摘录/);
  const result=cardExcerpt({...item,preview:null},[{text:null,ocr_text:'🙂'.repeat(200),kind:'screenshot'}],false);
  assert.equal(Array.from(result.text).length,161);
  assert.equal(result.label,'截图文字');
});

const { monthBoundaries, headerDays, liveCard } = await import('../src/timeline-view.ts');
test('monthly dates respect leap years, year boundaries and recorded-day-only navigation', () => {
  assert.equal(monthBoundaries(new Date(2024,1,12)).length,30);
  assert.equal(monthBoundaries(new Date(2025,1,12)).length,29);
  const dec=monthBoundaries(new Date(2025,11,12));
  assert.equal(new Date(dec.at(-1)).getFullYear(),2026);
  assert.deepEqual(headerDays([1,4,7,10,10],4),[7,4,1]);
  assert.deepEqual(headerDays([],4),[4]);
  assert.deepEqual(headerDays([1,4,7],1),[7,4,1]);
  assert.deepEqual(headerDays([1,4,7],5),[7,5,4]);
});
test('only newest matching live card glows; pause, errors, history and other sources do not', () => {
  const date=new Date(2026,9,17), now=date.getTime()+18*3600000;
  const live={...item,started_ms:now-10000,recording_sources:['microphone']};
  const state={capture_state:'recording',paused_intent:false,storage_paused:false,microphone_phase:'recording',microphone_recording:true,system_phase:'idle',system_recording:false};
  assert.equal(liveCard([live],state,date,now),live);
  assert.equal(liveCard([live],{...state,paused_intent:true},date,now),null);
  assert.equal(liveCard([live],null,date,now),null);
  assert.equal(liveCard([live],state,new Date(2026,9,16),now),null);
  assert.equal(liveCard([{...live,recording_sources:['system']}],state,date,now),null);
  assert.equal(liveCard([live,{...live,started_ms:now,recording_sources:[]}],state,date,now),null);
});

test('rendered live cards name the ongoing recording and use an open time range', async () => {
  const { createServer } = await import('vite');
  const { createElement } = await import('react');
  const { renderToStaticMarkup } = await import('react-dom/server');
  const server = await createServer({server:{middlewareMode:true},appType:'custom'});
  try {
    const { TimelineView } = await server.ssrLoadModule('/src/TimelineView.tsx');
    const date=new Date(), now=date.getTime();
    const live={...item,title:null,started_ms:now-10000,ended_ms:now,processing_state:'pending_fit',sources:'microphone',recording_sources:['microphone']};
    const state={capture_state:'recording',paused_intent:false,storage_paused:false,microphone_phase:'recording',microphone_recording:true};
    const render=(recordingState,entry=live)=>renderToStaticMarkup(createElement(TimelineView,{date,items:[entry],gaps:[],moments:[],recordingState,loading:false,hideCoarse:false}));
    assert.match(render(state), /<h3>实时记录内容<\/h3>/);
    assert.match(render(state), /至今/);
    assert.match(render(null), /<h3>未整理素材<\/h3>/);
    assert.doesNotMatch(render(null), /至今/);
    assert.match(render(state,{...live,id:'named',title:'项目讨论'}), /<h3>项目讨论<\/h3>/);
    const summarized=render(null,{...live,id:'named',title:'项目讨论',summary:'讨论了页面布局，倾向保留色块。'});
    assert.match(summarized,/内容摘要 · 麦克风/);
    assert.match(summarized,/讨论了页面布局，倾向保留色块。/);
    assert.doesNotMatch(summarized,/原始摘录/);
    assert.match(render(null,{...live,id:'legacy',title:'历史事件'}),/暂无内容摘要，打开查看转录/);
  } finally { await server.close(); }
});

test('event cards use generated summary independently of transcript state and coarse filter', () => {
  const event={...item,id:'event',sources:'microphone',summary:'比较了布局方案，倾向保留色块。'};
  for(const hide of [true,false]) {
    assert.deepEqual(cardExcerpt(event,[note],hide),{text:event.summary,label:'内容摘要 · 麦克风'});
    for(const summary of [null,undefined,'  ']) {
      assert.equal(cardExcerpt({...event,summary},[note],hide).text,'暂无内容摘要，打开查看转录');
    }
  }
  assert.equal(cardExcerpt({...event,id:null},[],false).text,'原始摘录');
});

test('date header retains selected day while month availability is unknown or empty', async () => {
  const { createServer } = await import('vite');
  const { createElement } = await import('react');
  const { renderToStaticMarkup } = await import('react-dom/server');
  const server = await createServer({server:{middlewareMode:true},appType:'custom'});
  try {
    const { TimelineView } = await server.ssrLoadModule('/src/TimelineView.tsx');
    for(const recordedDays of [null,[]]) {
      const html=renderToStaticMarkup(createElement(TimelineView,{date:new Date(2026,9,7),recordedDays,items:[],gaps:[],moments:[],recordingState:null,loading:true,hideCoarse:false}));
      assert.doesNotMatch(html,/暂无记录/);
      assert.match(html,/aria-current="date"/);
      assert.match(html,/10月7日/);
    }
  } finally { await server.close(); }
});


test('coarse filtering preserves the active recording and its excerpt, but not paused or historical coarse cards', async () => {
  const { createServer } = await import('vite');
  const { createElement } = await import('react');
  const { renderToStaticMarkup } = await import('react-dom/server');
  const server = await createServer({server:{middlewareMode:true},appType:'custom'});
  try {
    const { TimelineView } = await server.ssrLoadModule('/src/TimelineView.tsx');
    const date=new Date(), now=date.getTime();
    const live={...item,title:null,started_ms:now-10000,ended_ms:now,processing_state:'pending_fit',sources:'microphone',recording_sources:['microphone']};
    const history={...live,started_ms:now-30000,ended_ms:now-20000,recording_sources:[],preview:{...item.preview,text:'历史粗转摘录'}};
    const state={capture_state:'recording',paused_intent:false,storage_paused:false,microphone_phase:'recording',microphone_recording:true};
    const render=(recordingState,hideCoarse=true,selected=date)=>renderToStaticMarkup(createElement(TimelineView,{date:selected,items:[history,live],gaps:[],moments:[],recordingState,loading:false,hideCoarse}));
    const active=render(state);
    assert.match(active, /<h3>实时记录内容<\/h3>/);
    assert.match(active, /至今/);
    assert.match(active, /原始摘录/);
    assert.doesNotMatch(active, /历史粗转摘录/);
    assert.match(render(state,false), /历史粗转摘录/);
    for(const html of [render(null),render({...state,paused_intent:true}),render(state,true,new Date(now-86400000))]) {
      assert.doesNotMatch(html, /<h3>实时记录内容<\/h3>/);
      assert.match(html, /粗转内容已隐藏/);
      assert.doesNotMatch(html, /原始摘录/);
    }
  } finally { await server.close(); }
});
