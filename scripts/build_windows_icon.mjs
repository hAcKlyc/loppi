// Convert the approved square product ICON to Windows sizes; no artwork changes.
import sharp from 'sharp';
import {readFile, writeFile} from 'node:fs/promises';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const master=await readFile(resolve(root,'src/assets/rabbit/app-icon-square.png'));
const sizes=[16,24,32,48,64,128,256];
const {data:rgba,info}=await sharp(master).ensureAlpha().raw().toBuffer({resolveWithObject:true});
// Integer pixel-center sampling avoids CPU-dependent floating-point ties at48px.
// PNG compression may differ across hosts; decoded product artwork must not.
const frames=await Promise.all(sizes.map(async size=>{
 const pixels=Buffer.alloc(size*size*4);
 for(let y=0;y<size;y++)for(let x=0;x<size;x++){
  const sx=Math.floor((2*x+1)*info.width/(2*size));
  const sy=Math.floor((2*y+1)*info.height/(2*size));
  const start=(sy*info.width+sx)*4;
  rgba.copy(pixels,(y*size+x)*4,start,start+4);
 }
 return sharp(pixels,{raw:{width:size,height:size,channels:4}}).png().toBuffer();
}));
const directory=Buffer.alloc(6+16*frames.length);
directory.writeUInt16LE(1,2);
directory.writeUInt16LE(frames.length,4);
let offset=directory.length;
for(let i=0;i<frames.length;i++){
 const at=6+i*16;
 directory[at]=directory[at+1]=sizes[i]===256?0:sizes[i];
 directory.writeUInt16LE(1,at+4);
 directory.writeUInt16LE(32,at+6);
 directory.writeUInt32LE(frames[i].length,at+8);
 directory.writeUInt32LE(offset,at+12);
 offset+=frames[i].length;
}
await writeFile(resolve(root,'src-tauri/icons/icon.ico'),Buffer.concat([directory,...frames]));
console.log('Windows ICO exported from approved folded-ear square master:',sizes.join(', '));
