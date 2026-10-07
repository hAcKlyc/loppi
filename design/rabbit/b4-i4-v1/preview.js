import { RabbitPlayer } from './rabbit-player.js';
const players = [];
const create = (id, state = 'listening', quiet = null) => {
  const player = new RabbitPlayer(document.getElementById(id), { state, reducedMotion: quiet });
  players.push(player);
  return player;
};
const top = create('top-rabbit'), panelRabbit = create('panel-rabbit'), floatingRabbit = create('float-rabbit');
const entryPlayers = [top, panelRabbit, floatingRabbit];
for (const name of ['large','small','medium']) { create('listen-'+name); create('sleep-'+name,'sleeping'); }
const actionPlayers = [];
for (const [name,label,note,duration] of [
  ['curious','歪头看你','hover 的一次轻回应','0.62 秒'],
  ['nod','点头确认','保存成功之后回应','0.52 秒'],
  ['stretch','小小伸展','真实恢复后再醒来','0.60 秒'],
  ['wave','抬爪回应','有意互动时的彩蛋','0.80 秒']
]) {
  const card = document.createElement('article'); card.className='card';
  card.innerHTML=`<button class="action" aria-label="播放${label}"><canvas class="pixel" id="action-${name}"></canvas><h3>${label}</h3><p>${note}</p><div class="duration">${duration} · 单次</div></button>`;
  document.getElementById('actions').append(card);
  const player=create('action-'+name); actionPlayers.push(player);
  const play=()=>player.playAction(name);
  card.addEventListener('pointerenter',play); card.querySelector('button').addEventListener('click',play);
}
for (const [name,label,note] of [
  ['sleeping','主动暂停','闭眼与放松的耳朵'],
  ['waiting','正在切换 / 等待','清醒，停止常驻微动'],
  ['partial','部分来源不可用','清醒，耳朵放低'],
  ['interrupted','异常全部停止','清醒垂耳 + 静态提示']
]) {
  const card=document.createElement('article'); card.className='card static';
  card.innerHTML=`<canvas class="pixel" id="state-${name}"></canvas><h3>${label}</h3><p>${note}</p>`;
  document.getElementById('states').append(card); create('state-'+name,name,true).fixedPose=true;
}
for(const size of [16,24,32,48,64,128]) {
  const div=document.createElement('div'); div.innerHTML=`<img src="icons/app-${size}.png" width="${size}" height="${size}" alt="${size} 像素的方形图标">${size} px`;
  document.getElementById('icon-sizes').append(div);
}
let state='listening', collapseTimer, toastTimer, suppress=false;
const stage=document.getElementById('stage'), panel=document.getElementById('panel'), entry=document.getElementById('top-entry'), trigger=document.getElementById('top-trigger'), input=document.getElementById('comment-input');
const content=document.getElementById('panel-content');
function setState(next) {
  state=next;
  for(const player of [...entryPlayers,...actionPlayers])player.setState(next,{transition:true});
  document.querySelectorAll('[data-state]').forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.state===next)));
  const label=next==='sleeping'?'继续':'暂停';
  document.getElementById('pause').setAttribute('aria-label',label);
  document.getElementById('float-pause').textContent=label;
  document.getElementById('overlay').textContent=next==='sleeping'?'▷':'Ⅱ';
}
function open() {
  clearTimeout(collapseTimer);
  if(suppress||stage.dataset.mode!=='notch')return;
  const already=panel.classList.contains('expanded');
  panel.classList.add('expanded'); entry.classList.add('hidden'); trigger.setAttribute('aria-expanded','true');
  if(!already)panelRabbit.playAction('curious');
}
function close(block=false) {
  clearTimeout(collapseTimer); panel.classList.remove('expanded'); entry.classList.remove('hidden'); trigger.setAttribute('aria-expanded','false');
  input.hidden=true; content.hidden=false; suppress=block;
}
function scheduleClose() {
  if(!input.hidden)return;
  collapseTimer=setTimeout(()=>{
    if(!panel.matches(':hover')&&!entry.matches(':hover')&&!panel.contains(document.activeElement)&&!entry.contains(document.activeElement))close();
  },200);
}
function feedback(text) {
  const toast=document.getElementById('toast'); toast.textContent=text+' · 演示';toast.classList.add('shown');
  clearTimeout(toastTimer);toastTimer=setTimeout(()=>toast.classList.remove('shown'),1600);
  for(const player of entryPlayers)player.playAction('nod');
}
trigger.addEventListener('pointerenter',open); trigger.addEventListener('focus',open); trigger.addEventListener('click',()=>{open();document.getElementById('pause').focus();});
entry.addEventListener('pointerleave',()=>{suppress=false;scheduleClose();});
panel.addEventListener('pointerenter',()=>clearTimeout(collapseTimer));panel.addEventListener('pointerleave',()=>{suppress=false;scheduleClose();});
panel.addEventListener('focusout',scheduleClose);
document.querySelectorAll('[data-state]').forEach(button=>button.addEventListener('click',()=>setState(button.dataset.state)));
for(const id of ['pause','float-pause'])document.getElementById(id).addEventListener('click',()=>setState(state==='sleeping'?'listening':'sleeping'));
document.querySelectorAll('[data-command]').forEach(button=>button.addEventListener('click',()=>{close(true);button.blur();feedback(button.dataset.command==='打开评论'?'打开主窗口评论':'已'+(button.dataset.command==='重点'?'标记重点':'截图'));}));
document.getElementById('float-trigger').addEventListener('pointerenter',()=>floatingRabbit.playAction('curious'));
document.getElementById('float-trigger').addEventListener('click',()=>feedback('已标记重点'));
document.getElementById('comment').addEventListener('click',()=>{content.hidden=true;input.hidden=false;input.focus();});
input.addEventListener('keydown',event=>{
  if(event.key==='Enter'&&!event.shiftKey&&!event.isComposing&&event.keyCode!==229){event.preventDefault();if(input.value.trim()&&Array.from(input.value).length<=5000){input.value='';input.blur();close(true);feedback('已保存评论');}}
});
document.addEventListener('keydown',event=>{if(event.key==='Escape'){input.blur();close(true);}});
document.addEventListener('pointerdown',event=>{if(!panel.contains(event.target)&&!entry.contains(event.target))close();});
document.querySelectorAll('button[data-mode]').forEach(button=>button.addEventListener('click',()=>{close();stage.dataset.mode=button.dataset.mode;document.querySelectorAll('button[data-mode]').forEach(b=>b.setAttribute('aria-pressed',String(b===button)));}));
document.getElementById('hardware-toggle').addEventListener('click',event=>{close();const active=stage.classList.toggle('no-notch');event.currentTarget.setAttribute('aria-pressed',String(active));});
document.getElementById('tone').addEventListener('click',event=>{const active=stage.classList.toggle('dark');event.currentTarget.setAttribute('aria-pressed',String(active));});
document.getElementById('reduce').addEventListener('change',event=>{
  players.forEach(player=>player.setReducedMotion(player.fixedPose||event.target.checked?true:null));
});
window.rabbitPreview={players,entryPlayers,actionPlayers,setState};
Promise.all(players.map(player=>player.ready)).then(()=>{
  setState('listening'); document.body.dataset.ready='true';
}).catch(error=>{const box=document.getElementById('error');box.hidden=false;box.textContent='素材加载失败：'+error.message;});
window.addEventListener('pagehide',()=>players.forEach(player=>player.dispose()),{once:true});
