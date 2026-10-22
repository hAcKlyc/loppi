import React, {useId, useRef, useState} from 'react';
import {createRoot} from 'react-dom/client';
import {Button, Field, Modal, TabPanel, Tabs} from '/src/Controls';
import {mockIPC} from '@tauri-apps/api/mocks';
import {CloudKeyInput} from '/src/CloudKeyInput';
import {SpeakerPicker} from '/src/SpeakerPicker';
import '/src/App.css';

const cloudWrites: string[] = [];
if (!window.isTauri) window.isTauri = true;
mockIPC(async (command, args) => {
 if (command==='core_request' && args.request.type==='configure_cloud') { cloudWrites.push(args.request.provider); return {type:'cloud_status'}; }
 throw Error('Unexpected mock command');
});
const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
function Fixture() {
 const id = useId();
 const [tab, setTab] = useState('one');
 const [modal, setModal] = useState(false);
 const [nested, setNested] = useState(false);
 const [busy, setBusy] = useState(false);
 const [picker, setPicker] = useState(false);
 const [result, setResult] = useState('隔离组件，不连接用户数据、录音或云服务');
 const opener = useRef<HTMLButtonElement>(null);
 const safe = useRef<HTMLButtonElement>(null);
 const speaker = useRef<HTMLButtonElement>(null);
 const [count, setCount] = useState(0);
 const run = async () => {
  let assertions = 0;
  const check=(ok: unknown, label: string)=>{if(!ok)throw Error(label);assertions++;};
  try {
   opener.current?.focus(); opener.current?.click(); await wait(80);
   let dialog=document.querySelector<HTMLDialogElement>('dialog[open]')!;
   check(dialog && dialog.contains(document.activeElement) && document.activeElement?.tagName==='TEXTAREA','初始聚焦 textarea');
   const field=dialog.querySelector('textarea')!;
   check(field.labels?.[0]?.textContent==='草稿','输入有可见关联标签');
   for(let i=0;i<12;i++) {
    document.activeElement?.dispatchEvent(new KeyboardEvent('keydown',{key:'Tab',bubbles:true,cancelable:true}));
    check(dialog.contains(document.activeElement),'Tab 始终留在活动模态');
   }
   field.focus();field.dispatchEvent(new KeyboardEvent('keydown',{key:'Tab',bubbles:true,cancelable:true}));
   check(document.activeElement?.tagName==='SUMMARY','Tab 可达原生折叠入口');
   document.activeElement?.dispatchEvent(new KeyboardEvent('keydown',{key:'Tab',shiftKey:true,bubbles:true,cancelable:true}));
   check(document.activeElement===field,'Shift Tab 从折叠入口返回');
   check(field.getAttribute('aria-describedby') && document.getElementById(field.getAttribute('aria-describedby')!), '输入说明关联');
   dialog.querySelector<HTMLButtonElement>('[data-nested]')!.click();await wait(50);
   check(document.querySelectorAll('dialog[open]').length===2 && document.activeElement===safe.current,'嵌套危险确认聚焦取消');
   safe.current?.click();await wait(50);
   check(document.querySelectorAll('dialog[open]').length===1 && document.activeElement?.hasAttribute('data-nested'),'嵌套关闭恢复触发点');
   const busyButton=dialog.querySelector<HTMLButtonElement>('[data-busy]')!;const buttonWidth=busyButton.getBoundingClientRect().width;busyButton.click();await wait(40);
   check(Math.abs(busyButton.getBoundingClientRect().width-buttonWidth)<1,'busy 标签不改变按钮宽度');
   check(dialog.querySelector('[aria-label="关闭弹窗"]')?.hasAttribute('disabled'),'提交锁定关闭');
   check(dialog.querySelector('[role=status]')?.textContent?.includes('正在处理'),'提交锁定有可读反馈');
   dialog.dispatchEvent(new Event('cancel',{bubbles:false,cancelable:true}));await wait(30);
   check(dialog.open,'忙碌时不取消');
   await wait(700);
   dialog.dispatchEvent(new Event('cancel',{bubbles:false,cancelable:true}));await wait(50);
   check(!document.querySelector('dialog[open]') && document.activeElement===opener.current,'取消与焦点恢复');
   const tabs=document.querySelectorAll<HTMLButtonElement>('[role=tab]');
   tabs[0].focus();tabs[0].dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));
   check(document.activeElement===tabs[1] && tabs[0].getAttribute('aria-selected')==='true','方向键只移动焦点');
   tabs[1].click();await wait(30);
   check(tabs[1].tabIndex===0 && tabs[0].tabIndex===-1 && document.querySelectorAll('[role=tabpanel]:not([hidden])').length===1,'手动激活与可见面板');
   check([...tabs].every(t=>document.getElementById(t.getAttribute('aria-controls')!)?.getAttribute('aria-labelledby')===t.id),'全部面板关联');
   tabs[1].dispatchEvent(new KeyboardEvent('keydown',{key:'Home',bubbles:true}));
   check(document.activeElement===tabs[0], 'Home 移到首项');
   for (const label of ['DeepSeek API Key','火山引擎 API Key']) {
    const key=document.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
    key.focus();Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(key,'test-only-input');key.dispatchEvent(new Event('input',{bubbles:true}));await wait(30);
    const before=cloudWrites.length;
    key.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:'测'}));
    key.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',isComposing:true,bubbles:true,cancelable:true}));await wait(30);
    check(cloudWrites.length===before&&document.activeElement===key,'中文组合输入 Enter 不提交或失焦');
    key.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'测'}));
    key.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));await wait(80);
    check(cloudWrites.length===before+1,'组合结束后的 Enter 保留普通/compact 提交语义');
   }
   setTab('one');
   setResult(`PASS · ${assertions} 项公共组件检查；另以真实按键检查 Tab / Escape`);
  }catch(error){setResult(`FAIL · ${String(error)}`);}
 };
 return <main className="page-container" style={{height:'100vh',overflow:'auto'}}>
  <h1>公共组件验证</h1>
  <p>模态、嵌套、忙碌、草稿关联和标签页；也可手动按 Tab / Escape。</p>
  <Button onClick={()=>void run()}>运行公共组件回归</Button>
  <p role="status" id="result">{result}</p>
  <Button ref={opener} onClick={()=>setModal(true)}>打开编辑</Button>
  <Button onClick={()=>setCount(c=>c+1)}>背景操作 {count}</Button>
  <Tabs id={id} label="演示内容" value={tab} items={[{value:'one',label:'第一项'},{value:'two',label:'第二项'}]} onChange={setTab}/>
  <TabPanel tabsId={id} value="one" selected={tab}><p>第一项内容</p></TabPanel>
  <TabPanel tabsId={id} value="two" selected={tab}><p>第二项内容</p></TabPanel>
  <CloudKeyInput provider="llm" configured={false} onRefresh={async()=>{}}/>
  <CloudKeyInput provider="asr" configured={false} compact onRefresh={async()=>{}}/>
  {modal && <Modal title="编辑草稿" busy={busy} onClose={()=>setModal(false)}>
   <Field label="草稿" description="取消不会提交；此页只使用内存数据。">{attrs=><textarea {...attrs} defaultValue="可以使用中文输入法验证 Enter"/>}</Field>
   <details><summary>更多说明</summary><p>可用键盘展开的内容。</p></details>
   <Button data-nested onClick={()=>setNested(true)}>打开危险确认</Button>
   <Button data-busy busy={busy} busyLabel="保存中…" onClick={()=>{setBusy(true);setTimeout(()=>setBusy(false),600);}}>模拟提交</Button>
   <Button ref={speaker} onClick={()=>setPicker(true)}>指定人物</Button>
   <Button onClick={()=>setResult('已继续编辑')}>继续编辑</Button>
   {picker && speaker.current && <SpeakerPicker anchor={speaker.current} people={[]} localSpeakers={[]} currentGroup={null} personId={null} confirmed={false} onClose={()=>setPicker(false)} onApply={async()=>{setPicker(false);}}/>}
   {nested && <Modal title="删除示例？" initialFocusRef={safe} onClose={()=>setNested(false)}>
    <p>此页不连接真实数据。初始焦点在取消。</p>
    <Button ref={safe} onClick={()=>setNested(false)}>取消</Button>
    <Button tone="danger" onClick={()=>setNested(false)}>删除示例</Button>
   </Modal>}
  </Modal>}
 </main>;
}
createRoot(document.getElementById('root')!).render(<Fixture/>);
