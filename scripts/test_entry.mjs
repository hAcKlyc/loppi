import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
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

test('top rabbit follows native width continuously and avoids the physical notch', async () => {
  const { topRabbitMotion } = await import('../src/entry-api.ts');
  // Evaluate the generated CSS length expression at actual viewport widths.
  const value = (css, width) => Function(`return ${css.replaceAll('100vw', String(width)).replaceAll('px', '').replaceAll('calc', '').replaceAll('max(', 'Math.max(').replaceAll('min(', 'Math.min(')}`)();
  for (const feedback of [false, true]) for (const band of [0, 32, 40]) {
    const compact = feedback ? 280 : 44;
    const shift = (412 - compact) / 2 + (band ? 111.5 : 0);
    const style = topRabbitMotion(band, shift, feedback);
    let previous = null;
    for (let i = 0; i <= 102; i++) {
      const progress = i / 100;
      const width = compact + (412 - compact) * progress;
      const x = value(style['--entry-rabbit-x'], width);
      const y = value(style['--entry-rabbit-y'], width);
      const size = value(style['--entry-rabbit-size'], width);
      const globalX = -shift * progress + x; // origin starts at zero
      if (i === 0) {
        assert.ok(Math.abs(x - (compact / 2 - 14)) < 1e-6);
        assert.ok(Math.abs(size - 28) < 1e-6);
      }
      if (i === 100) {
        assert.ok(Math.abs(x - 16) < 1e-6);
        assert.ok(Math.abs(y - (Math.max(24, band) + 14)) < 1e-6);
        assert.ok(Math.abs(size - 96) < 1e-6);
      }
      // The rabbit must first clear the notch vertically before moving left.
      if (band && y < band) assert.ok(globalX >= compact / 2 - 14 - 1e-6);
      if (previous) assert.ok(Math.abs(globalX - previous) < 10, 'no teleport at the trajectory join');
      previous = globalX;
    }
  }
});

// A visual-only radius change must not expose painted corners outside the host mask.
test('entry surface corner radii agree with the native pointer masks', async () => {
 const css=await readFile(new URL('../src/entry.css',import.meta.url),'utf8');
 const native=await readFile(new URL('../src-tauri/native/entry.m',import.meta.url),'utf8');
 const tokens=await readFile(new URL('../src/tokens.css',import.meta.url),'utf8');
 const px=name=>Number(tokens.match(new RegExp('--'+name+':\\s*(\\d+)px;'))?.[1]);
 const top=css.match(/\.notch-surface \{[\s\S]*?border-radius:\s*0 0 var\(--entry-top-radius\) var\(--entry-top-radius\);/);
 const menu=css.match(/\.rabbit-menu \{[\s\S]*?border-radius:\s*var\(--entry-menu-radius\);/);
 const topMask=native.match(/double radius=MIN\((\d+),g.width\/2\)/);
 const menuMask=native.match(/roundedRegion\(NSMakeRect\(8,g.menu_y,116,167\),(\d+),p\)/);
 assert(top&&menu&&topMask&&menuMask,'Native/CSS geometry declarations must remain inspectable');
 assert.equal(px('entry-top-radius'),Number(topMask[1]),'top bottom-left corner');
 assert.equal(px('entry-top-radius'),Number(topMask[1]),'top bottom-right corner');
 assert.equal(px('entry-menu-radius'),Number(menuMask[1]),'floating menu corners');
});
