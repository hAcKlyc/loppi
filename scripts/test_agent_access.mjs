// Real component and clipboard flow; only native IPC is mocked.
import assert from 'node:assert/strict';
import test from 'node:test';
import {registerHooks} from 'node:module';
import {readFileSync,existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import ts from 'typescript';
import {JSDOM} from 'jsdom';
const dom=new JSDOM('<div id="root"></div>',{url:'http://localhost/'});
for(const name of ['window','document','HTMLElement','HTMLButtonElement','Event','MouseEvent','KeyboardEvent'])globalThis[name]=dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT=true;
globalThis.isTauri=true;
registerHooks({
 resolve(specifier,context,next){
  if(specifier.startsWith('.')&&context.parentURL?.includes('/src/'))for(const suffix of ['.ts','.tsx']){
   const url=new URL(specifier+suffix,context.parentURL);if(existsSync(url))return next(url.href,context);
  }
  return next(specifier,context);
 },
 load(url,context,next){
  if(url.endsWith('.css'))return {format:'module',source:'',shortCircuit:true};
  if(/\.tsx?$/.test(url))return {format:'module',shortCircuit:true,source:ts.transpileModule(readFileSync(fileURLToPath(url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext,jsx:ts.JsxEmit.ReactJSX}}).outputText};
  return next(url,context);
 }
});
const {default:React,act}=await import('react');
const {createRoot}=await import('react-dom/client');
const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');
const {AgentAccess}=await import('../src/AgentAccess.tsx');
const settle=async fn=>act(async()=>{await fn?.();await new Promise(resolve=>setTimeout(resolve,0));});
const button=text=>[...document.querySelectorAll('button')].find(b=>b.textContent.trim()===text);

test('copies the exact workspace install prompt, removes redundant UI, and preserves MCP/errors',async()=>{
 let copied='',failCopy=false,failInfo=false,online=true;
 const prompt='请复制本机 Skill 并安装到当前工作区：\nD:\\用户资料 with spaces\\skills\\loppi';
 const info={cli_path:'D:\\Apps\\loppi.exe',cli_available:true,skill_prompt:prompt,codex_config:'[mcp_servers.loppi]',mcp_config:{mcpServers:{loppi:{command:'D:\\Apps\\loppi.exe',args:['mcp','serve']}}}};
 mockIPC(async(command,args)=>{
  if(command==='agent_access_info'){if(failInfo)throw 'Official SKILL.md is not a regular file';return {...info,status:{ok:true,data:{online}}};}
  if(command==='copy_agent_text'){if(failCopy)throw Error('fixture');copied=args.text;return;}
  throw Error(command);
 });
 const root=createRoot(document.getElementById('root'));
 try{
  await settle(()=>root.render(React.createElement(AgentAccess)));
  assert.ok(button('复制配置'));
  await settle(()=>button('复制配置').click());assert.match(copied,/mcpServers/);
  await settle(()=>button('CLI + Skill').click());
  assert.equal(document.querySelector('.agent-code').textContent,prompt);
  for(const redundant of ['在终端运行一次','已有自定义 Skill 不会被覆盖','试一条读取命令','只读已有记录','使用云端模型的 Agent'])assert.ok(!document.body.textContent.includes(redundant));
  assert.equal(document.querySelector('details'),null);assert.equal(document.querySelector('footer'),null);
  await settle(()=>button('复制安装提示词').click());assert.equal(copied,prompt);assert.match(document.querySelector('[aria-live]').textContent,/已复制/);
  failCopy=true;await settle(()=>button('复制安装提示词').click());assert.match(document.querySelector('[aria-live]').textContent,/复制失败/);failCopy=false;
  online=false;await settle(()=>button('刷新').click());assert.match(document.body.textContent,/后台未连接/);assert.ok(button('复制安装提示词'));
  failInfo=true;await settle(()=>button('刷新').click());assert.match(document.querySelector('[role=alert]').textContent,/暂时无法读取接入信息：Official SKILL.md is not a regular file/);assert.equal(button('复制安装提示词'),undefined);
  failInfo=false;await settle(()=>button('刷新').click());await settle(()=>button('MCP').click());assert.ok(button('复制配置'));
 }finally{await settle(()=>root.unmount());clearMocks();}
});
