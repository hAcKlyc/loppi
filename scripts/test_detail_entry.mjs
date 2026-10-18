import assert from 'node:assert/strict';
import {test} from 'node:test';
import {entryTime,nearestSegmentIndex,pageCoversTime} from '../src/detail-entry.ts';
const span=(start,end)=>({started_ms:start,ended_ms:end});
test('entry clamps to real range and defaults to start without inventing a seek',()=>{
 const item=span(100,1000);
 for(const [at,expected] of [[undefined,100],[NaN,100],[Infinity,100],[0,100],[540,540],[1000,999],[2000,999]]) assert.equal(entryTime(item,at),expected);
});
test('nearest text prefers containing intervals and resolves gaps and empty results',()=>{
 const rows=[span(100,200),span(200,300),span(400,500)];
 assert.equal(nearestSegmentIndex(rows,200),1);
 assert.equal(nearestSegmentIndex(rows,280),1);
 assert.equal(nearestSegmentIndex(rows,370),2);
 assert.equal(nearestSegmentIndex(rows,50),0);
 assert.equal(nearestSegmentIndex(rows,800),2);
 assert.equal(nearestSegmentIndex([],200),-1);
 assert.equal(nearestSegmentIndex([span(100,400),span(200,300)],250),0);
});
test('initial paging continues until each stream covers the target or is exhausted',()=>{
 const rows=Array.from({length:100},(_,i)=>span(i*10,i*10+5));
 assert.equal(pageCoversTime(rows,990,100),false);
 assert.equal(pageCoversTime(rows,700,100),true);
 assert.equal(pageCoversTime(rows,1500,100),false);
 assert.equal(pageCoversTime(rows.slice(0,99),1500,100),true);
 assert.equal(pageCoversTime([],1500,100),true);
});

test('positioned text respects playback source, including gaps and missing sources',()=>{
 const rows=[{...span(100,200),source:'microphone'},{...span(210,300),source:'system'}];
 assert.equal(nearestSegmentIndex(rows,205,'microphone'),0);
 assert.equal(nearestSegmentIndex(rows,205,'system'),1);
 assert.equal(nearestSegmentIndex(rows,250,'microphone'),0);
 assert.equal(nearestSegmentIndex(rows,250,'system'),1);
 assert.equal(nearestSegmentIndex([rows[0]],250,'system'),-1);
});
