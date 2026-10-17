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
  assert.equal(cardExcerpt({...item,id:'event'},[note],true).text,'原始摘录');
  assert.equal(cardExcerpt({...item,manual_range_id:'range',preview:{text:'修正后的精转',source:'system',is_precise:true}},[],true).label,'精转 · 电脑声音');
});
test('empty and legacy summaries use honest placeholders; screenshot text remains bounded', () => {
  assert.equal(cardExcerpt({...item,preview:null},[],false).label,'');
  assert.match(cardExcerpt({...item,preview:undefined},[],false).text,/暂无文字摘录/);
  const result=cardExcerpt({...item,preview:null},[{text:null,ocr_text:'🙂'.repeat(200),kind:'screenshot'}],false);
  assert.equal(Array.from(result.text).length,161);
  assert.equal(result.label,'截图文字');
});
