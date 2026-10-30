// Opt-in, no capture/cloud. A closed SQLite backup is copied into a temporary Core.
// HEAR_PLAYWRIGHT_MODULE=/path/to/playwright-core/index.mjs node scripts/measure_gui_reads.mjs
//   CORE_BIN SNAPSHOT.sqlite OUTPUT.json [http://127.0.0.1:5198] [/path/to/old/DetailDrawer.tsx]
import {mkdtempSync,copyFileSync,readFileSync,writeFileSync,existsSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,dirname,resolve} from 'node:path';
import {spawn,execFileSync} from 'node:child_process';
import {connect} from 'node:net';
import {randomUUID} from 'node:crypto';
import {performance} from 'node:perf_hooks';
const [coreArg,snapshot,output,base='http://127.0.0.1:5198',drawer='/src/DetailDrawer.tsx']=process.argv.slice(2);
if(!coreArg||!snapshot||!output)throw Error('CORE_BIN SNAPSHOT OUTPUT required; snapshot must be a closed backup');
const core=resolve(coreArg),cli=process.env.HEAR_BENCH_CLI || join(dirname(core),'hear');
const data=mkdtempSync(join(tmpdir(),'hear-gui-measure-'));
copyFileSync(snapshot,join(data,'hear.sqlite'));
const env={...process.env,HEAR_DATA_DIR:data,HEAR_CORE_BIN:core,HEAR_TEST_PARENT_PID:String(process.pid)};
const build=JSON.parse(execFileSync(core,['--build-info'],{env}));
const child=spawn(core,['serve','--handoff','query'],{env,stdio:'ignore'});
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
let browser;
const result={build,cli:[],wire:[],browser:[],limits:'Real Core socket + Playwright WebKit through an exposed-function bridge, not native Tauri invoke. React development build. No private payloads retained.'};
async function wire(request,id=randomUUID()) {
 const started=performance.now();
 const raw=await new Promise((ok,no)=>{
  const chunks=[];const s=connect(join(data,'core.sock'));
  s.setTimeout(10000,()=>s.destroy(Error('socket timeout')));
  s.on('error',no);s.on('data',b=>chunks.push(b));s.on('end',()=>ok(Buffer.concat(chunks)));
  s.on('connect',()=>s.end(JSON.stringify({version:build.protocol_version,id,client:'gui',request})));
 });
 const transportMs=performance.now()-started,parsed=JSON.parse(raw),parseMs=performance.now()-started-transportMs;
 result.wire.push({command:request.type,id,offset:request.offset,limit:request.limit,ms:transportMs,parse_ms:parseMs,bytes:raw.length,code:parsed.code});
 return parsed;
}
try {
 for(let i=0;!existsSync(join(data,'core.sock'));i++){if(i>400||child.exitCode!==null)throw Error('Core did not start');await sleep(50);}
 for(const args of [
  ['timeline','1793116800000','1793203200000'],['context','list','--date','2026-10-28'],
  ['context','read','--event','4b0b5314-0fad-4e0b-bdab-d7afbba4e3e7','--limit','50'],
  ['context','read','--from','2026-10-28T12:00:00+08:00','--to','2026-10-28T15:00:00+08:00','--limit','50']]) {
  const start=performance.now();let raw,error;
  try {raw=execFileSync(cli,args,{env,timeout:15000,stdio:['ignore','pipe','pipe']});}
  catch(e){raw=e.stdout;error=String(e.stderr).slice(0,180);}
  result.cli.push({command:args,ms:performance.now()-start,bytes:raw?.length,error});
 }
 const timeline=await wire({type:'timeline',from_ms:1793116800000,to_ms:1793203200000});
 const item=timeline.items?.find(x=>x.id==='4b0b5314-0fad-4e0b-bdab-d7afbba4e3e7');
 if(!item)throw Error('Benchmark event unavailable');
 for(const limit of [100,30]) {
  const detail=await wire({type:'event_detail',event_id:item.id,offset:0,limit});
  result.wire.at(-1).fields=Object.fromEntries(Object.entries(detail.event??{}).map(([k,v])=>[k,Buffer.byteLength(JSON.stringify(v))]));
 }
 if(process.env.HEAR_PLAYWRIGHT_MODULE) {
  const {webkit}=await import(process.env.HEAR_PLAYWRIGHT_MODULE);
  browser=await webkit.launch({headless:true});
  for(let run=0;run<5;run++) {
   const page=await browser.newPage({viewport:{width:1280,height:900}});
   await page.exposeFunction('queryCore',async({request,requestId})=>{
    if(!['event_detail','range_detail','session_speakers','moments'].includes(request.type))throw Error('read-only benchmark');
    return wire(request,requestId);
   });
   await page.addInitScript(item=>{
    window.fixtureItem=item;window.isTauri=true;
    window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
     const started=performance.now(),r=await window.queryCore(args),ended=performance.now();
     window.measurement?.requests.push({command:args.request.type,id:args.requestId,started,ended});return r;
    }};
   },item);
   // Evict the existing per-connection last-page cache with a different key.
   await wire({type:'event_detail',event_id:item.id,offset:300,limit:1});
   await page.goto(`${base}/scripts/test_gui_read_performance.html?drawer=${encodeURIComponent(drawer)}`);
   await page.waitForFunction(()=>window.fixtureReady);
   await page.locator('#open').click();
   await page.waitForFunction(()=>window.measurement?.readable || document.querySelector('.detail-main .detail-load-error'),{},{timeout:15000});
   if(await page.evaluate(()=>Boolean(window.measurement.readable)))
    await page.waitForFunction(()=>window.measurement.requests.some(r=>r.command==='session_speakers'),{},{timeout:15000});
   result.browser.push(await page.evaluate(()=>({...window.measurement,
    click_to_readable_ms:window.measurement.readable ? window.measurement.readable-window.measurement.started : null,
    error:document.querySelector('.detail-main .detail-load-error')?.textContent,
   })));
   await page.close();
  }
 }
 await sleep(150);
 const logs=readFileSync(join(data,'logs/hear.jsonl'),'utf8').trim().split('\n').map(JSON.parse);
 for(const req of result.wire){
  const log=logs.find(x=>x.request_id===req.id&&x.event===req.command);
  if(log){req.core_ms=log.duration_ms;req.serialize_us=log.context?.serialize_us;}
 }
 writeFileSync(output,JSON.stringify(result,null,2)+'\n');
 console.log(JSON.stringify({output,cli:result.cli,browser_ms:result.browser.map(x=>x.click_to_readable_ms)},null,2));
}finally{
 await browser?.close();child.kill('SIGTERM');await new Promise(r=>child.exitCode!==null?r():child.once('exit',r));rmSync(data,{recursive:true,force:true});
}
