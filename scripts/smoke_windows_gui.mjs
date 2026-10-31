// Actual isolated Tauri/WebView2 surface. CDP is enabled only for this disposable test process.
import {spawn} from 'node:child_process';
import {mkdirSync,writeFileSync} from 'node:fs';
import {resolve,join,dirname} from 'node:path';
import {createServer} from 'node:net';
const sleep=ms=>new Promise(yes=>setTimeout(yes,ms));
const directory=resolve('build/windows-gui-test');mkdirSync(directory,{recursive:true});
const server=createServer();await new Promise(yes=>server.listen(0,'127.0.0.1',yes));const port=server.address().port;await new Promise(yes=>server.close(yes));
const exe=resolve(process.argv[2]??'build/windows-dev-runtime/loppi-desktop.exe');
const app=spawn(exe,[],{windowsHide:true,env:{...process.env,LOPPI_DATA_DIR:join(directory,'data'),LOPPI_MODEL_ROOT:resolve('build/cache/windows/models'),LOPPI_TEST_PARENT_PID:String(process.pid),WEBVIEW2_USER_DATA_FOLDER:join(directory,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`},stdio:['ignore','pipe','pipe']});
let logs='';app.stderr.on('data',data=>{logs=(logs+data).slice(-8000);});
async function connect(){
  for(let i=0;i<150;i++){
    if(app.exitCode!==null)throw new Error(`app exited ${app.exitCode}: ${logs}`);
    try{const targets=await(await fetch(`http://127.0.0.1:${port}/json`)).json();const target=targets.find(t=>t.type==='page'&&t.url.includes('1420')&&!t.url.includes('entry'));
      for(const target of targets.filter(t=>t.type==='page'&&t.url.includes('1420'))){
        const socket=new WebSocket(target.webSocketDebuggerUrl);await new Promise((yes,no)=>{socket.addEventListener('open',yes,{once:true});socket.addEventListener('error',no,{once:true});});
        const label=await new Promise((yes,no)=>{const timer=setTimeout(()=>no(new Error('label timeout')),2000);socket.addEventListener('message',event=>{const reply=JSON.parse(event.data);if(reply.id===999999){clearTimeout(timer);yes(reply.result?.result?.value);}},{once:true});socket.send(JSON.stringify({id:999999,method:'Runtime.evaluate',params:{expression:'window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label',returnByValue:true}}));});
        if(label==='main')return socket;socket.close();
      }
    }catch{}
    await sleep(200);
  }throw new Error(`WebView2 debug endpoint unavailable: ${logs}`);
}
let socket;let next=0;const pending=new Map();const report={schema:1,real_windows:true,checks:[]};
function call(method,params={}){const id=++next;return new Promise((yes,no)=>{const timer=setTimeout(()=>{pending.delete(id);no(new Error(`CDP timeout: ${method}`));},30000);pending.set(id,{yes,no,timer});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(expression){const result=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(result.exceptionDetails)throw new Error(JSON.stringify(result.exceptionDetails));return result.result.value;}
async function invoke(command,args={}){return evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);}
try{
  socket=await connect();socket.addEventListener('message',event=>{const message=JSON.parse(event.data);if(message.id&&pending.has(message.id)){const task=pending.get(message.id);pending.delete(message.id);clearTimeout(task.timer);message.error?task.no(new Error(JSON.stringify(message.error))):task.yes(message.result);}});
  await call('Runtime.enable');await sleep(1500);
  report.checks.push({name:'build',value:await invoke('build_info')});
  report.checks.push({name:'privacy',value:await invoke('privacy_status')});
  const probed=await invoke('request_system_audio_permission');
  const privacy=await invoke('privacy_status');
  if(probed!==1||privacy.system_audio!==1)throw new Error(JSON.stringify({probed,privacy}));
  report.checks.push({name:'actual-system-probe-independent-of-locked-screen',privacy});
  report.checks.push({name:'startup-status-read-only',value:await invoke('login_item_status')});
  report.checks.push({name:'entry',value:await invoke('entry_snapshot',{refresh:true})});
  await invoke('entry_mode',{mode:'floating'});await sleep(200);await invoke('entry_mode',{mode:'top'});
  report.checks.push({name:'entry-mode-roundtrip',passed:true});
  await evaluate('(async()=>{window.__winEnded=[];await (await import("/node_modules/@tauri-apps/api/event.js")).listen("loppi-playback-ended",e=>window.__winEnded.push(e.payload));})()');
  const count=48000*4;const wav=Buffer.alloc(44+count*2);wav.write('RIFF');wav.writeUInt32LE(wav.length-8,4);wav.write('WAVEfmt ',8);wav.writeUInt32LE(16,16);wav.writeUInt16LE(1,20);wav.writeUInt16LE(1,22);wav.writeUInt32LE(48000,24);wav.writeUInt32LE(96000,28);wav.writeUInt16LE(2,32);wav.writeUInt16LE(16,34);wav.write('data',36);wav.writeUInt32LE(count*2,40);for(let i=0;i<count;i++)wav.writeInt16LE(Math.round(Math.sin(i*2*Math.PI*440/48000)*16000),44+i*2);
  const context=(await invoke('playback_output',{command:{type:'open'}})).context;
  await invoke('playback_output',{command:{type:'resume',context,epoch:1}});
  const buffer=await invoke('playback_output',{command:{type:'load',context,epoch:1,wav:wav.toString('base64')}});
  await invoke('playback_output',{command:{type:'schedule',context,epoch:1,buffer:buffer.buffer,when:0}});
  await sleep(200);
  const excluded=await invoke('core_request',{request:{type:'probe',source:'system',seconds:2}});
  if(excluded.type!=='probe_result'||excluded.stats.peak>0.0001)throw new Error(`own playback leaked into system capture: ${JSON.stringify(excluded)}`);
  report.checks.push({name:'actual-native-playback-exclusion',response:excluded});
  await sleep(2200);const ended=await evaluate('window.__winEnded');
  if(!ended.some(event=>event.context===context&&event.buffer===buffer.buffer&&event.status===1))throw new Error(`native output did not finish: ${JSON.stringify(ended)}`);
  report.checks.push({name:'actual-native-playback-ended-event',events:ended});
  await invoke('playback_output',{command:{type:'close',context,epoch:2}});
  const image=await call('Page.captureScreenshot',{format:'png'});writeFileSync(join(directory,'main.png'),Buffer.from(image.data,'base64'));
  report.checks.push({name:'main-window-screenshot',path:join(directory,'main.png')});
  report.text=await evaluate('document.body.innerText.slice(0,1800)');
  const shutdown=spawn(join(dirname(exe),'loppi.exe'),['prepare-install'],{windowsHide:true,env:{...process.env,LOPPI_DATA_DIR:join(directory,'data'),LOPPI_TEST_PARENT_PID:String(process.pid)},stdio:['ignore','pipe','pipe']});
  let shutdownError='';shutdown.stderr.on('data',data=>{shutdownError+=data;});
  const shutdownCode=await new Promise((yes,no)=>{const timer=setTimeout(()=>no(new Error('normal installation shutdown timed out')),30000);shutdown.once('error',no);shutdown.once('close',code=>{clearTimeout(timer);yes(code);});});
  if(shutdownCode!==0)throw new Error(shutdownError);
  if(app.exitCode===null)await new Promise(yes=>app.once('close',yes));
  if(app.exitCode!==0)throw new Error(`GUI normal exit returned ${app.exitCode}`);
  report.checks.push({name:'normal-gui-core-exit-before-installation',passed:true,forced:false});
}finally{
  socket?.close();
  // Ask the real window close/exit route in subsequent lifecycle acceptance; this
  // smoke owner tears down its disposable process if a preceding assertion fails.
  if(app.exitCode===null)app.kill();
  report.stderr=logs;writeFileSync(join(directory,'report.json'),JSON.stringify(report,null,2)+'\n');
}
console.log(JSON.stringify(report,null,2));
