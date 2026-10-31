const fs = require('node:fs/promises');
const path = require('node:path');
const sharp = require(process.argv[2] || 'sharp');
const root = __dirname;
const size = 48;
const palette = ['#103e2f','#386a4e','#76ad89','#9acfac','#b4dfb8','#fff7df','#e9dec1','#ffbc94','#f89a72'].map(hex=>[1,3,5].map(i=>parseInt(hex.slice(i,i+2),16)));
const names = ['listening','blink','sleeping','curious','nod','stretch','wave','waiting','partial'];
function clean(data,w,h,threshold=160,minArea=30) {
  const seen = new Uint8Array(w*h);
  const groups=[];
  for(let i=0;i<w*h;i++) {
    if(seen[i]||data[i*4+3]<threshold)continue;
    const queue=[i];seen[i]=1;
    for(let n=0;n<queue.length;n++) {
      const p=queue[n],x=p%w,y=Math.floor(p/w);
      for(const q of [x>0?p-1:-1,x<w-1?p+1:-1,y>0?p-w:-1,y<h-1?p+w:-1])if(q>=0&&!seen[q]&&data[q*4+3]>=threshold){seen[q]=1;queue.push(q);}
    }
    groups.push(queue);
  }
  const keep = new Uint8Array(w*h);
  for(const g of groups)if(g.length>=minArea)for(const p of g)keep[p]=1;
  let left=w,right=0,top=h,bottom=0;
  for(let i=0;i<w*h;i++) {
    if(!keep[i])data.fill(0,i*4,i*4+4);
    else {data[i*4+3]=255;const x=i%w,y=Math.floor(i/w);left=Math.min(left,x);right=Math.max(right,x);top=Math.min(top,y);bottom=Math.max(bottom,y);}
  }
  return {left,top,width:right-left+1,height:bottom-top+1};
}
function quantize(data) {
  for(let i=0;i<data.length;i+=4) {
    if(data[i+3]<128){data.fill(0,i,i+4);continue;}
    let best=palette[0],distance=Infinity;
    for(const p of palette){const d=2*(data[i]-p[0])**2+3*(data[i+1]-p[1])**2+(data[i+2]-p[2])**2;if(d<distance){best=p;distance=d;}}
    data[i]=best[0];data[i+1]=best[1];data[i+2]=best[2];data[i+3]=255;
  }
  return data;
}
function breath(frame) {
  const out=Buffer.from(frame);
  // Whole face through row 34 and planted feet from row 40 remain byte-identical.
  for(let y=35;y<40;y++) {
    const source=35+Math.floor((y-35)*5/6);
    frame.copy(out,y*size*4,source*size*4,(source+1)*size*4);
  }
  return out;
}
function headDip(frame) {
  const out=Buffer.from(frame);
  for(let y=29;y>0;y--)frame.copy(out,y*size*4,(y-1)*size*4,y*size*4);
  out.fill(0,0,size*4);
  return out;
}
function blinkFrom(frame) {
  const out=Buffer.from(frame);
  // Locate the two isolated dark eye components; the outline stays untouched.
  const dark=(x,y)=>{const i=(y*size+x)*4;return frame[i+3]===255&&frame[i]===palette[0][0]&&frame[i+1]===palette[0][1]&&frame[i+2]===palette[0][2];};
  const seen=new Set(),eyes=[];
  for(let y=16;y<30;y++)for(let x=12;x<37;x++) {
    const id=y*size+x;if(seen.has(id)||!dark(x,y))continue;
    const q=[id];seen.add(id);
    for(let j=0;j<q.length;j++){const p=q[j],xx=p%size,yy=Math.floor(p/size);for(const [a,b]of[[xx-1,yy],[xx+1,yy],[xx,yy-1],[xx,yy+1]]){const n=b*size+a;if(a>=0&&a<size&&b>=0&&b<size&&!seen.has(n)&&dark(a,b)){seen.add(n);q.push(n);}}}
    if(q.length>=2&&q.length<=16&&q.every(p=>Math.floor(p/size)>=22&&Math.floor(p/size)<=29))eyes.push(q);
  }
  if(eyes.length!==2)throw new Error(`Expected two isolated eyes, found ${eyes.length}`);
  for(const eye of eyes) {
    const xs=eye.map(p=>p%size),ys=eye.map(p=>Math.floor(p/size));const minX=Math.min(...xs),maxX=Math.max(...xs),minY=Math.min(...ys),maxY=Math.max(...ys);
    for(const p of eye){const i=p*4;out[i]=palette[3][0];out[i+1]=palette[3][1];out[i+2]=palette[3][2];}
    for(let x=minX-1;x<=maxX;x++){const i=(maxY*size+x)*4;out[i]=palette[0][0];out[i+1]=palette[0][1];out[i+2]=palette[0][2];out[i+3]=255;}
  }
  return out;
}
async function main() {
  for(const dir of ['poses','animations','icons','icons/loppi.iconset'])await fs.mkdir(path.join(root,dir),{recursive:true});
  const source=await sharp(path.join(root,'source/poses-generated.png')).ensureAlpha().raw().toBuffer({resolveWithObject:true});
  const cells=[];
  for(let n=0;n<9;n++) {
    const left=Math.floor((n%3)*source.info.width/3),top=Math.floor(Math.floor(n/3)*source.info.height/3);
    const w=Math.floor((n%3+1)*source.info.width/3)-left,h=Math.floor((Math.floor(n/3)+1)*source.info.height/3)-top;
    const raw=await sharp(source.data,{raw:source.info}).extract({left,top,width:w,height:h}).raw().toBuffer();
    const bounds=clean(raw,w,h);
    cells.push({raw,w,h,bounds});
  }
  const scale=42/cells[0].bounds.height;
  const frames={};
  for(let n=0;n<9;n++) {
    const {raw,w,h,bounds}=cells[n];
    const width=Math.max(1,Math.round(bounds.width*scale)),height=Math.max(1,Math.round(bounds.height*scale));
    const small=await sharp(raw,{raw:{width:w,height:h,channels:4}}).extract(bounds).resize(width,height,{kernel:'nearest'}).raw().toBuffer();
    quantize(small);
    let footL=width,footR=0;
    for(let y=Math.max(0,height-3);y<height;y++)for(let x=0;x<width;x++)if(small[(y*width+x)*4+3]){footL=Math.min(footL,x);footR=Math.max(footR,x);}
    const x=Math.round(24-(footL+footR+1)/2),y=45-height;
    if(x<0||x+width>size||y<0)throw new Error('Pose exceeded cell '+names[n]);
    const canvas=Buffer.alloc(size*size*4);
    for(let row=0;row<height;row++)small.copy(canvas,((y+row)*size+x)*4,row*width*4,(row+1)*width*4);
    frames[names[n]]=canvas;
  }
  frames.blink=blinkFrom(frames.listening);
  frames.nod=headDip(frames.blink);
  frames['listening-breath']=breath(frames.listening);
  frames['sleeping-breath']=breath(frames.sleeping);
  frames['sleeping-nod']=headDip(frames.sleeping);
  const keys=Object.keys(frames);
  for(const name of keys) {
    await sharp(frames[name],{raw:{width:size,height:size,channels:4}}).png().toFile(path.join(root,'poses',name+'.png'));
    await sharp(frames[name],{raw:{width:size,height:size,channels:4}}).resize(192,192,{kernel:'nearest'}).png().toFile(path.join(root,'poses',name+'@4x.png'));
  }
  const clips={
    listening:{label:'倾听呼吸',loop:true,frames:[['listening',2000],['listening-breath',1600],['listening',2800],['listening',2000],['listening-breath',1600],['blink',110],['listening',2690]]},
    sleeping:{label:'睡眠待机',loop:true,frames:[['sleeping',3200],['sleeping-breath',3200],['sleeping',3200]]},
    curious:{label:'歪头看你',loop:false,frames:[['listening',100],['curious',360],['listening',160]]},
    nod:{label:'点头确认',loop:false,frames:[['listening',80],['blink',90],['nod',160],['blink',70],['listening',120]]},
    stretch:{label:'唤醒伸展',loop:false,frames:[['sleeping',120],['waiting',120],['stretch',200],['listening',160]]},
    wave:{label:'抬爪回应',loop:false,frames:[['listening',120],['wave',260],['listening',90],['wave',180],['listening',150]]},
    pause:{label:'合眼入睡',loop:false,frames:[['listening',120],['blink',140],['sleeping',300]]},
    'sleeping-response':{label:'睡眠轻回应',loop:false,frames:[['sleeping',120],['sleeping-nod',200],['sleeping',180]]},
    waiting:{label:'等待',loop:false,frames:[['waiting',1000]]},
    partial:{label:'部分来源异常',loop:false,frames:[['partial',1000]]},
    interrupted:{label:'异常停止',loop:false,frames:[['waiting',1000]],badge:true}
  };
  for(const [name,clip] of Object.entries(clips)) {
    const data=Buffer.concat(clip.frames.map(([key])=>frames[key]));
    const options={raw:{width:size,height:size*clip.frames.length,channels:4,pageHeight:size}};
    await sharp(data,options).webp({lossless:true,loop:clip.loop?0:1,delay:clip.frames.map(f=>f[1])}).toFile(path.join(root,'animations',name+'.webp'));
    await sharp(data,options).gif({loop:clip.loop?0:1,delay:clip.frames.map(f=>f[1]),dither:0,colours:16}).toFile(path.join(root,'animations',name+'.gif'));
  }
  const columns=4,rows=Math.ceil(keys.length/columns);
  await sharp({create:{width:columns*size,height:rows*size,channels:4,background:'#00000000'}}).composite(keys.map((key,n)=>({input:frames[key],raw:{width:size,height:size,channels:4},left:n%columns*size,top:Math.floor(n/columns)*size}))).png().toFile(path.join(root,'sprite-atlas.png'));
  const atlas=Object.fromEntries(keys.map((key,n)=>[key,{x:n%columns*size,y:Math.floor(n/columns)*size,width:size,height:size}]));
  const manifest={version:1,character:'B4 好奇折耳',icon:'I4 折耳近景',cell:{width:size,height:size,footBaseline:45},palette:palette.map(p=>'#'+p.map(c=>c.toString(16).padStart(2,'0')).join('')),atlas:'sprite-atlas.png',frames:atlas,clips,behavior:{idle:'listening',paused:'sleeping',sleepingResponse:'sleeping-response',reducedMotion:'state pose only',hidden:'stop timers; resume saved state',actions:'play once then return to current state'},status:'视觉素材首版；供实际尺寸和动作验收，未连接核心采集状态'};
  await fs.writeFile(path.join(root,'manifest.json'),JSON.stringify(manifest,null,2)+'\n');
  // The full-bleed square is the only master; masks belong exclusively to derivatives.
  const icon=await sharp(path.join(root,'source/icon-square-generated.png')).removeAlpha().resize(1024,1024).png().toBuffer();
  const sizes=[16,24,32,48,64,128,256,512,1024];
  for(const s of sizes)await sharp(icon).resize(s,s,{kernel:'lanczos3'}).png().toFile(path.join(root,'icons',`app-${s}.png`));
  const rounded=await sharp(icon).ensureAlpha().composite([{input:Buffer.from('<svg width="1024" height="1024"><rect width="1024" height="1024" rx="224" fill="white"/></svg>'),blend:'dest-in'}]).png().toBuffer();
  const circle=await sharp(icon).ensureAlpha().composite([{input:Buffer.from('<svg width="1024" height="1024"><circle cx="512" cy="512" r="512" fill="white"/></svg>'),blend:'dest-in'}]).png().toBuffer();
  await fs.writeFile(path.join(root,'icons/rounded-1024.png'),rounded);
  await fs.writeFile(path.join(root,'icons/circle-1024.png'),circle);
  for(const s of [16,32,128,256,512])for(const ratio of [1,2])await sharp(rounded).resize(s*ratio,s*ratio).png().toFile(path.join(root,'icons/loppi.iconset',`icon_${s}x${s}${ratio===2?'@2x':''}.png`));
  const icoSizes=[16,24,32,48,64,128,256],images=await Promise.all(icoSizes.map(s=>sharp(path.join(root,'icons',`app-${s}.png`)).ensureAlpha().png().toBuffer()));
  const header=Buffer.alloc(6+16*images.length);header.writeUInt16LE(1,2);header.writeUInt16LE(images.length,4);let offset=header.length;
  images.forEach((png,i)=>{const p=6+i*16,s=icoSizes[i];header[p]=s===256?0:s;header[p+1]=s===256?0:s;header.writeUInt16LE(1,p+4);header.writeUInt16LE(32,p+6);header.writeUInt32LE(png.length,p+8);header.writeUInt32LE(offset,p+12);offset+=png.length;});
  await fs.writeFile(path.join(root,'icons/app.ico'),Buffer.concat([header,...images]));
  console.log(JSON.stringify({poses:keys.length,animations:Object.keys(clips).length,iconSizes:sizes,bounds:cells.map(c=>c.bounds),paletteColors:palette.length}));
}
main().catch(error=>{console.error(error);process.exitCode=1;});
