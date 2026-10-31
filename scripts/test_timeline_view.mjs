import assert from 'node:assert/strict';
import { test } from 'node:test';
import { daySpan, cardExcerpt, dayMapPoint } from '../src/timeline-view.ts';

test('hover cursor maps exact track position to local time and keeps the right edge at 24:00', () => {
  const from = new Date(2026,9,18).getTime(), to = new Date(2026,9,19).getTime();
  assert.equal(dayMapPoint(80,100,720,from,to).label,'00:00');
  assert.equal(dayMapPoint(460,100,720,from,to).label,'12:00');
  assert.equal(dayMapPoint(100+720*(17*60+54)/1440,100,720,from,to).label,'17:54');
  assert.equal(dayMapPoint(850,100,720,from,to).label,'24:00');
  assert.equal(dayMapPoint(850,100,720,from,to).x,720);
  for (const args of [[NaN,100,720,from,to],[400,100,0,from,to],[400,100,720,to,from]]) assert.equal(dayMapPoint(...args),null);
  // The local day range can be 23/25 hours; interpolation follows its actual duration.
  for (const hours of [23,25]) assert.equal(dayMapPoint(460,100,720,from,from+hours*3600000).at,from+hours*1800000);
});

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
    const statusLabels=html=>[...html.matchAll(/<span class="event-card-state[^"]*"[^>]*>(.*?)<\/span>/g)].map(match=>match[1]);
    assert.deepEqual(statusLabels(render(state)),['正在记录']);
    assert.match(render(state),/recording-label">正在记录<\/span><\/div><h3>/);
    assert.doesNotMatch(render(state), /待整理/);
    for(const processing_state of ['pending_fit','fitting','pending_asr','transcribing','transcribed']) {
      assert.deepEqual(statusLabels(render(state,{...live,processing_state})),['正在记录']);
    }
    assert.deepEqual(statusLabels(render({...state,paused_intent:true})),['待整理']);
    for (const [processing_state, label] of Object.entries({fit_credentials_unavailable:'整理受阻 · 无法读取密钥',fit_retrying:'整理异常 · 等待重试',fit_failed:'事件整理失败'})) {
      const html=render(state,{...live,processing_state});
      assert.match(html,/正在记录/);
      assert.deepEqual(statusLabels(html),[label,'正在记录']);
    }
    for(const [processing_state,label] of [['analysis_failed','本地语音分析失败'],['asr_failed','精转失败'],['partial_asr_failed','部分精转失败 · 已完成文字保留']]) {
      assert.deepEqual(statusLabels(render(state,{...live,processing_state})),[label,'正在记录']);
    }
    assert.deepEqual(statusLabels(render(state,{...live,local_analysis_failures:2})),['部分本地分析失败 · 2 项','正在记录']);
    const completed=render(null,{...live,id:'named',title:'讨论',processing_state:'partial_transcript',transcription:{total:5,complete:5,empty:1,boundary_clipped:false}});
    assert.match(completed,/精转完成 · 1 段无文字/);
    assert.doesNotMatch(completed,/精转中|部分精转/);
    const blockedNamed=render(state,{...live,id:'named',title:'讨论',processing_state:'transcribed',fit_state:'fit_credentials_unavailable'});
    assert.match(blockedNamed,/正在记录/);
    assert.match(blockedNamed,/整理受阻 · 无法读取密钥/);
    assert.deepEqual(statusLabels(blockedNamed),['整理受阻 · 无法读取密钥','正在记录']);
    assert.doesNotMatch(blockedNamed,/精转完成/);
    for (const [processing_state, label] of [['pending_asr','待精转'],['transcribed','精转完成']]) {
      const partial=render(null,{...live,id:'named',processing_state,local_analysis_failures:2});
      assert.match(partial,/部分本地分析失败 · 2 项/);
      assert.ok(partial.includes(label));
    }
    assert.doesNotMatch(completed,/部分本地分析失败/);
    const partlyFitted=render(null,{...live,id:'named',title:'讨论',processing_state:'transcribed',fit_state:'fit_failed'});
    assert.match(partlyFitted,/部分整理失败/);
    assert.match(partlyFitted,/精转完成/);
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

test('hiding coarse keeps failed material visible with its own status and time', async () => {
  const { createServer } = await import('vite');
  const { createElement } = await import('react');
  const { renderToStaticMarkup } = await import('react-dom/server');
  const server = await createServer({server:{middlewareMode:true},appType:'custom'});
  try {
    const { TimelineView } = await server.ssrLoadModule('/src/TimelineView.tsx');
    const date=new Date(2026,9,23), start=date.getTime()+16*3600000;
    for(const processing_state of ['fit_failed','fit_retrying','fit_credentials_unavailable','raw_audio']) {
      const failed={...item,id:null,manual_range_id:null,title:null,sources:'microphone',started_ms:start,ended_ms:start+900000,processing_state,local_analysis_failures:processing_state==='raw_audio'?1:undefined,recording_sources:[]};
      const html=renderToStaticMarkup(createElement(TimelineView,{date,items:[failed],gaps:[],moments:[],recordingState:null,loading:false,hideCoarse:true}));
      assert.match(html,/未整理素材/);
      assert.match(html,/16:00/);
      assert.doesNotMatch(html,/粗转内容已隐藏/);
      assert.doesNotMatch(html,/原始摘录/);
    }
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
