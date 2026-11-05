import assert from 'node:assert/strict';
import { test } from 'node:test';
import { usageBoundaries, usageNumber, usageAxis } from '../src/usage-statistics.ts';
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
test('cost labels and axes consistently round to two decimal places',()=>{
 for (const [value, expected] of [[0,'¥0.00'],[1,'¥0.00'],[36000,'¥0.00'],[4999999,'¥0.00'],[5000000,'¥0.01'],[1234567890,'¥1.23'],[1235000000,'¥1.24'],[1000000000000,'¥1,000.00']]) {
  assert.equal(usageNumber(value,'cost_nano'),expected);
  assert.equal(usageAxis(value,true),expected);
 }
 assert.equal(usageNumber(1000000,'input_tokens'),'1,000,000');
});
