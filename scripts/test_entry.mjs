import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { EntryRequests, commentCanSubmit, commentEnterSubmits } from '../src/entry-api.ts';

globalThis.window = globalThis;
test('comment validates Unicode length, whitespace and IME composition', () => {
 assert.equal(commentCanSubmit('  \n ',false), false);
 assert.equal(commentCanSubmit('确定候选词',true), false);
 assert.equal(commentCanSubmit('🐰'.repeat(5000),false), true);
 assert.equal(commentCanSubmit('🐰'.repeat(5001),false), false);
});
test('an uncertain request retries its original identity, time and text', async () => {
 const calls=[];
 mockIPC((command,args) => {assert.equal(command,'entry_action');calls.push(structuredClone(args.action));return {outcome:calls.length===1?'uncertain':'confirmed',message:null,response:null};});
 const requests=new EntryRequests();
 await requests.send('comment',1000,'原评论');
 assert.equal(requests.isUncertain('comment'),true);
 await requests.send('comment',9000,'不可替换的编辑');
 assert.deepEqual(calls[0],calls[1]);
 assert.equal(requests.isUncertain('comment'),false);
 clearMocks();
});
test('pending save prevents duplicate submission while leaving pause available', async () => {
 let finish;
 const calls=[];
 mockIPC((command,args) => {calls.push(args.action.kind);return args.action.kind==='mark'?new Promise(resolve=>{finish=resolve;}):{outcome:'confirmed'};});
 const requests=new EntryRequests();
 const first=requests.send('mark',1000,null);
 await assert.rejects(requests.send('mark',1000,null), /操作正在处理/);
 await requests.send('control',1001,null);
 assert.deepEqual(calls,['mark','control']);
 finish({outcome:'confirmed'});await first;clearMocks();
});
test('a lost bridge reply preserves the operation; a definite rejection allows a new edit', async () => {
 const calls=[];let failure=true;
 mockIPC((_command,args) => {calls.push(structuredClone(args.action));if(failure) {failure=false;throw new Error('reply lost');}return {outcome:'rejected',message:'moment_invalid'};});
 const requests=new EntryRequests();
 await assert.rejects(requests.send('comment',1000,'原文'), /reply lost/);
 assert.equal(requests.isUncertain('comment'),true);
 await requests.send('comment',2000,'改文');
 assert.deepEqual(calls[0],calls[1]);
 await requests.send('comment',3000,'新文');
 assert.notEqual(calls[2].id,calls[1].id);assert.equal(calls[2].text,'新文');clearMocks();
});

test('Enter submits only outside IME; Shift Enter remains a newline', () => {
 const key={key:'Enter',shiftKey:false,keyCode:13,isComposing:false};
 assert.equal(commentEnterSubmits(key,false),true);
 assert.equal(commentEnterSubmits({...key,isComposing:true},false),false);
 assert.equal(commentEnterSubmits(key,true),false);
 assert.equal(commentEnterSubmits({...key,keyCode:229},false),false);
 assert.equal(commentEnterSubmits({...key,shiftKey:true},false),false);
});
