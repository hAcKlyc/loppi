import {build} from 'vite';
import react from '@vitejs/plugin-react';
import {readFile,writeFile,mkdir,rm,} from 'node:fs/promises';
import {resolve} from 'node:path';
import {execFileSync} from 'node:child_process';

const root=resolve(import.meta.dirname,'../../..');
const base='e705b714f5e52449f4f9671af002b8b0290c4372';
const stage=resolve(root,'build/homepage-review');
await mkdir(stage,{recursive:true});
for(const name of ['before','after'])await rm(resolve(stage,name),{recursive:true,force:true});
await mkdir(resolve(stage,'before'),{recursive:true});
execFileSync('tar',['-xf','-','-C',resolve(stage,'before')],{input:execFileSync('git',['archive',base,'src'],{cwd:root,maxBuffer:32*1024*1024})});

const currentTokens=await readFile(resolve(root,'src/tokens.css'),'utf8');
const colorVars=[...currentTokens.matchAll(/--([\w-]+):\s*([^;]+);/g)]
 .filter(([,name,value])=>name.startsWith('palette-')||name.startsWith('color-')||(name.startsWith('entry-')&&/^(#|var\(--(?:color|palette)-)/.test(value.trim())))
 .map(([,name,value])=>`--${name}:${value};`).join('\n');
const tokenPath=resolve(stage,'before/src/tokens.css');
await writeFile(tokenPath,(await readFile(tokenPath,'utf8'))+'\n/* Shared colors; baseline geometry stays intact. */\n:root{'+colorVars+'}\n');
await mkdir(resolve(stage,'after'),{recursive:true});
execFileSync('tar',['-xf','-','-C',resolve(stage,'after')],{input:execFileSync('git',['archive','b7c9d1c6344dc873df9b2a2009bff1ff76b4ca79','src'],{cwd:root,maxBuffer:32*1024*1024})});
const afterTokens=resolve(stage,'after/src/tokens.css');
await writeFile(afterTokens,(await readFile(afterTokens,'utf8'))+'\n:root{'+colorVars+'}\n');

const fixture=await readFile(resolve(root,'scripts/test_design_scene.html'),'utf8');
let moduleSource=fixture.match(/<script type="module">([\s\S]*?)<\/script>/)[1];
moduleSource=moduleSource.replace('return items;','return items.slice(0,Number(window.frameElement?.dataset.count ?? 3));');
moduleSource=moduleSource.replace('result.cards === 3','result.cards === Number(window.frameElement?.dataset.count ?? 3)');
moduleSource=moduleSource.replace("const requested = new URLSearchParams(location.search).get('scene') ?? 'timeline';","const requested = window.frameElement?.dataset.scene ?? 'timeline';");
moduleSource=moduleSource.replace("if (new URLSearchParams(location.search).has('reduce'))",'if (true)');
const documents={};
for(const [name,path] of Object.entries({candidate:'src/App.tsx',before:'build/homepage-review/before/src/App.tsx',after:'build/homepage-review/after/src/App.tsx'})){
 const entry=resolve(stage,name+'-entry.tsx');
 const result=await build({root,configFile:false,define:{'process.env.NODE_ENV':'"production"'},plugins:[react(),{
  name:'homepage-review',resolveId(id){if(id===entry)return entry},load(id){if(id===entry)return moduleSource.replace("'/src/App.tsx'",JSON.stringify(resolve(root,path)))}
 }],build:{write:false,assetsInlineLimit:Infinity,cssCodeSplit:false,minify:true,lib:{entry,formats:['iife'],name:'LoppiReview'}}});
 const output=Array.isArray(result)?result.flatMap(r=>r.output):result.output;
 const js=output.find(x=>x.type==='chunk').code.replace(/<\/script/gi,'<\\/script');
 const css=output.filter(x=>x.type==='asset'&&x.fileName.endsWith('.css')).map(x=>String(x.source)).join('\n');
 documents[name]=`<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>${css}</style><div id="root"></div><output id="fixture-evidence" hidden></output><script>${js}</script></html>`;
}
const payload=JSON.stringify(documents).replace(/</g,'\\u003c');
const html=`<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>loppi · 日期与当前时间</title><style>${currentTokens}
*{box-sizing:border-box}body{margin:0;background:var(--color-canvas);color:var(--color-text);font:14px/1.65 -apple-system,BlinkMacSystemFont,'PingFang SC',sans-serif}header,.notes{max-width:1184px;margin:auto;padding:24px 32px}h1{font-size:26px;letter-spacing:-.025em;margin:0 0 6px}p{color:var(--color-text-secondary);margin:6px 0 14px}.tools{display:flex;gap:12px;align-items:center;flex-wrap:wrap}.switch{display:flex;gap:4px}button,select{font:inherit;color:inherit;padding:6px 12px;border:1px solid var(--color-border);border-radius:5px;background:var(--color-surface);cursor:pointer}button[aria-pressed=true]{background:var(--color-text);color:var(--color-canvas)}button:focus-visible,select:focus-visible{outline:2px solid var(--color-logo-dot);outline-offset:3px}.compare{display:grid;grid-template-columns:1fr 1fr;gap:20px;padding:0 32px;max-width:2360px;margin:auto}.compare.single{grid-template-columns:1fr;max-width:1184px}.panel{min-width:0}.panel[hidden]{display:none}figcaption{display:flex;gap:12px;align-items:center;padding:4px 0 10px}figcaption strong{font-size:16px}figcaption span{color:var(--color-text-secondary)}.viewport{overflow:hidden;background:var(--color-canvas);border:1px solid var(--color-border);border-radius:9px}iframe{display:block;border:0;transform-origin:0 0;background:var(--color-canvas)}.notes{padding-top:16px}.notes ul{margin:0;padding:16px 0 0 20px;border-top:1px solid var(--color-border)}.notes li{padding:4px 0}.meta{font-size:12px;color:var(--color-text-secondary)}.dot{display:inline-block;width:8px;height:8px;border-radius:2px;margin:0 5px}.complete{background:var(--color-complete)}.live{background:var(--color-live)}.reference[hidden]{display:none}@media(max-width:900px){header,.notes{padding:20px}.compare{padding:0 20px;gap:12px}.compare:not(.single){grid-template-columns:1fr}}
</style></head><body><header><h1>日期与当前时间</h1><p>前后逐日浏览，历史可回到今天；当前时间置于红线上方。</p><div class="tools"><div class="switch" aria-label="查看方式"><button data-view="candidate" aria-pressed="true">本轮方案</button><button data-view="both" aria-pressed="false">并排对照</button></div><label class="reference" hidden>对照 <select id="reference"><option value="before">调整前 · 0.1.17</option><option value="after">上一轮 · 开放式条目</option></select></label><label>状态 <select id="scene"><option value="timeline">已暂停</option><option value="recording">正在记录</option><option value="empty">空白一天</option></select></label><label>内容 <select id="count"><option value="1">1 条记录</option><option value="3">3 条记录</option></select></label><label>窗口 <select id="size"><option value="1120">1120 × 760</option><option value="760">760 × 540</option></select></label></div></header>
<section class="compare single" id="compare"><article class="panel" data-panel="candidate"><figcaption><strong>本轮 · 日期与时刻</strong><span>逐日切换 · 纯时间定位</span></figcaption><div class="viewport"><iframe id="candidate" title="优化后的时间线首页" data-count="1" data-scene="timeline"></iframe></div></article><article class="panel" data-panel="reference" hidden><figcaption><strong id="reference-title">调整前 · 0.1.17</strong><span>同一组资料与窗口</span></figcaption><div class="viewport"><iframe id="reference-frame" title="首页对照版本" data-count="1" data-scene="timeline"></iframe></div></article></section>
<section class="notes"><ul><li><strong>字标：</strong>圆润矢量字形与深暖红 i 方点；窄窗保留原折耳歪头 ICON，倾听动画仍独立。</li><li><strong>状态：</strong><i class="dot complete"></i>已留存记录、已保存标记；<i class="dot live"></i>录制中、当前时间与播放位置。普通导航和控件保持中性。</li><li><strong>布局：</strong>日期箭头逐日切换；历史可回今天。当前HH:MM在红线上方，靠近两端内收文字。白卡和已校正节点保持。</li></ul><p class="meta">本轮方案直接构建自当前生产组件，不添加样式覆盖 · 合成资料 · 不连接真实录音、用户数据或云端。顶部选择器用于查看不同状态。调整前来自e705b71；“上一轮”来自b7c9d1c。</p></section>
<script>const documents=${payload};const frames=[...document.querySelectorAll('iframe')],container=document.getElementById('compare');let mode='candidate';function fit(){const width=Number(document.getElementById('size').value),height=width===760?540:760;for(const frame of frames){if(frame.closest('.panel').hidden)continue;const box=frame.parentElement,scale=Math.min(1,box.clientWidth/width);frame.width=width;frame.height=height;frame.style.transform='scale('+scale+')';box.style.height=height*scale+'px';}}function load(){for(const frame of frames){frame.dataset.count=document.getElementById('count').value;frame.dataset.scene=document.getElementById('scene').value;frame.srcdoc=documents[frame.id==='candidate'?'candidate':document.getElementById('reference').value];}fit();}for(const frame of frames)frame.onload=fit;for(const button of document.querySelectorAll('[data-view]'))button.onclick=()=>{mode=button.dataset.view;container.classList.toggle('single',mode!=='both');document.querySelector('[data-panel=reference]').hidden=mode!=='both';document.querySelector('.reference').hidden=mode!=='both';for(const b of document.querySelectorAll('[data-view]'))b.setAttribute('aria-pressed',String(b===button));fit();};document.getElementById('reference').onchange=()=>{document.getElementById('reference-title').textContent=document.getElementById('reference').selectedOptions[0].text;load();};for(const id of ['scene','count'])document.getElementById(id).onchange=load;document.getElementById('size').onchange=fit;new ResizeObserver(fit).observe(container);load();</script></body></html>`;
await writeFile(resolve(root,'design/reviews/homepage-same-palette.html'),html);
console.log('Updated homepage review: original layout + approved wordmark and semantics');
