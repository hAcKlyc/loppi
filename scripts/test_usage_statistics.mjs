import assert from 'node:assert/strict';
import { test } from 'node:test';
import { usageBoundaries, usageNumber } from '../src/usage-statistics.ts';
test('ranges contain exactly N local calendar days including today', () => {
 for (const days of [7,30,90]) {
  const now = new Date(2026,9,17,17,34);
  const result=usageBoundaries(days,now);
  assert.equal(result.length,days+1);
  assert.equal(new Date(result.at(-2)).getDate(),17);
  assert.equal(new Date(result.at(-1)).getDate(),18);
  assert.ok(result.every(ms=>new Date(ms).getHours()===0));
 }
});
test('calendar boundaries preserve 23-hour and 25-hour local days', () => {
 const prior=process.env.TZ;
 process.env.TZ='America/New_York';
 for(const [now,hours] of [[new Date(2026,2,9),23],[new Date(2026,10,2),25]]){
  const days=usageBoundaries(7,now);
  assert.ok(days.slice(1).some((at,i)=>at-days[i]===hours*3600000));
 }
 if(prior===undefined) delete process.env.TZ; else process.env.TZ=prior;
});
test('small nonzero costs are never formatted as zero',()=>{
 assert.equal(usageNumber(1,'cost_nano'),'<¥0.00001');
 assert.equal(usageNumber(36000,'cost_nano'),'¥0.00004');
 assert.equal(usageNumber(0,'cost_nano'),'¥0.00');
 assert.equal(usageNumber(1000000,'input_tokens'),'1,000,000');
});
