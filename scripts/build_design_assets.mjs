#!/usr/bin/env node
// Static design exports only. Runtime assets stay ordinary SVG/PNG and existing atlas data.
import sharp from 'sharp';
import {readFile, writeFile, mkdir, cp, readdir, stat} from 'node:fs/promises';
import {resolve, join, relative, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const out=join(root,'design/system-v1');
const read=p=>readFile(join(root,p));
const save=async(p,data)=>{await mkdir(dirname(p),{recursive:true});await writeFile(p,data);};
const svg=await read('src/assets/brand/wordmark.svg');
const source=svg.toString();
const variants={color:source,mono:source.replaceAll('#B65B42','#292C28'),reversed:source.replaceAll('#292C28','#FAF8F2')};
for(const [name,body] of Object.entries(variants)){
 await save(join(out,'brand',`wordmark-${name}.svg`),body);
 for(const width of [96,240,1200])await save(join(out,'brand',`wordmark-${name}-${width}.png`),await sharp(Buffer.from(body)).resize({width}).png().toBuffer());
}
const rabbit=await read('src/assets/rabbit/poses/listening.png');
const rabbitURL='data:image/png;base64,'+rabbit.toString('base64');
const logoInner=source.match(/<svg[^>]+>([\s\S]*)<\/svg>/)[1];
const lockup=`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 382 120"><image href="${rabbitURL}" x="0" y="12" width="96" height="96" style="image-rendering:pixelated"/><g transform="translate(126 10)">${logoInner}</g></svg>`;
await save(join(out,'brand/lockup.svg'),lockup);
await save(join(out,'brand/lockup.png'),await sharp(Buffer.from(lockup)).resize({width:1146}).png().toBuffer());
// Product ICON is independently approved: export existing bytes, never rebuild
// or overwrite runtime icons from the upright listening animation.
await mkdir(join(out,'icons'),{recursive:true});
await mkdir(join(out,'source'),{recursive:true});
await cp(join(root,'src/assets/rabbit/app-icon-square.png'),join(out,'icons/app-square-1024.png'));
await cp(join(root,'design/rabbit/b4-i4-v1/icons/rounded-1024.png'),join(out,'icons/app-macos-1024.png'));
for(const size of [16,24,32,48,64,128,256,512,1024])await cp(join(root,'design/rabbit/b4-i4-v1/icons',`app-${size}.png`),join(out,'icons',`app-${size}.png`));
await cp(join(root,'design/rabbit/b4-i4-v1/icons/loppi.iconset'),join(out,'icons/loppi.iconset'),{recursive:true});
await cp(join(root,'src-tauri/icons/icon.icns'),join(out,'icons/loppi.icns'));
await cp(join(root,'assets/loppi-icon-source.png'),join(out,'source/product-icon-original.png'));
await cp(join(root,'src/assets/rabbit'),join(out,'rabbit'),{recursive:true});
await mkdir(join(out,'rabbit/animations'),{recursive:true});
for(const name of ['listening','sleeping'])await cp(join(root,'design/rabbit/upright-sleep-v2/animations',`${name}.gif`),join(out,'rabbit/animations',`${name}.gif`));
const tokenCSS=(await read('src/tokens.css')).toString();
const tokens=Object.fromEntries([...tokenCSS.matchAll(/--([\w-]+):\s*([^;]+);/g)].map(m=>[m[1],m[2].trim()]));
await save(join(out,'tokens.json'),JSON.stringify({schema:1,source:'src/tokens.css',tokens},null,2)+'\n');
await save(join(out,'tokens.css'),tokenCSS);
const designSource=(await read('specs/DESIGN.md')).toString();
const offlineDesign=designSource.replace(/\[([^\]]+)\]\(([^)]+)\)/g,(_,label,target)=>target.startsWith('http')?`[${label}](${target})`:`${label}（仓库：\`${relative(root,resolve(root,'specs',target))}\`）`);
await save(join(out,'DESIGN.md'),'<!-- Generated from specs/DESIGN.md; edit the source and rebuild. Repository paths are references, not bundled files. -->\n'+offlineDesign);
const cover=(width,height,copy)=>`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"><rect width="${width}" height="${height}" fill="#FAF8F2"/><g transform="translate(88 ${height/2-100}) scale(1.25)">${logoInner}</g><text x="92" y="${height/2+86}" fill="#625E57" font-family="-apple-system, BlinkMacSystemFont, sans-serif" font-size="24">${copy}</text><image href="${rabbitURL}" x="${width-320}" y="${height/2-96}" width="192" height="192" style="image-rendering:pixelated"/><rect x="88" y="${height-48}" width="${width-176}" height="1" fill="#DED9D0"/></svg>`;
for(const [name,width,height,copy] of [['readme-header',1280,360,'声音留在这里。'],['social-cover',1280,640,'A quiet companion for your everyday audio.']]){
 const body=cover(width,height,copy);await save(join(out,'release',`${name}.svg`),body);await save(join(out,'release',`${name}.png`),await sharp(Buffer.from(body)).png().toBuffer());
}
const sleepingURL='data:image/png;base64,'+(await read('src/assets/rabbit/poses/sleeping.png')).toString('base64');
const swatches=[['炭黑','#292C28'],['纸白','#FAF8F2'],['暖石','#EDE9E1'],['完成绿','#8BBDA1'],['进行中','#F4A686'],['i 方点','#B65B42']];
const usage=`<svg xmlns="http://www.w3.org/2000/svg" width="1440" height="1200" viewBox="0 0 1440 1200">
<rect width="1440" height="1200" fill="#FAF8F2"/><g font-family="-apple-system, PingFang SC, sans-serif" fill="#292C28">
<text x="64" y="72" font-size="30" font-weight="600">loppi · 设计使用速查</text><text x="64" y="108" font-size="17" fill="#625E57">安静的桌边伙伴。完整规则以 specs/DESIGN.md 为准。</text>
${swatches.map(([name,color],i)=>`<rect x="${64+i*220}" y="146" width="192" height="72" rx="4" fill="${color}" stroke="#DED9D0"/><text x="${64+i*220}" y="246" font-size="17">${name} · ${color}</text>`).join('')}
<path d="M64 282H1376M64 666H1376M64 966H1376" stroke="#DED9D0"/>
<text x="64" y="330" font-size="22" font-weight="600">字标：保留比例与一个方点的净空</text>
<rect x="64" y="354" width="532" height="244" fill="none" stroke="#B65B42" stroke-dasharray="5 5"/>
<g transform="translate(92 376) scale(2)">${logoInner}</g>
<text x="64" y="632" font-size="16" fill="#625E57">虚线表示最小净空；字标建议宽度 ≥ 60px。i 点不做阶梯。</text>
<rect x="740" y="354" width="636" height="244" rx="8" fill="#292C28"/>
<g transform="translate(778 376) scale(2)">${logoInner.replaceAll('#292C28','#FAF8F2')}</g>
<text x="740" y="632" font-size="16" fill="#625E57">反白用于暖黑底；单色版用于单色输出。</text>
<text x="64" y="710" font-size="22" font-weight="600">颜色表达事实，角色保持分寸</text>
<image href="${rabbitURL}" x="64" y="738" width="96" height="96" style="image-rendering:pixelated"/>
<rect x="192" y="750" width="8" height="8" rx="2" fill="#F4A686"/><text x="216" y="763" font-size="20" fill="#A6533C">正在记录</text>
<text x="192" y="796" font-size="16" fill="#625E57">暖红只占少量标记、细线与浅底</text>
<image href="${sleepingURL}" x="740" y="738" width="96" height="96" style="image-rendering:pixelated"/>
<text x="868" y="763" font-size="20">已暂停</text><text x="868" y="796" font-size="16" fill="#625E57">主动休息使用中性色；故障不能伪装成睡眠</text>
<rect x="64" y="862" width="8" height="8" rx="2" fill="#8BBDA1"/><text x="88" y="874" font-size="18" fill="#47745B">已保存 · 留存绿不代表所有云任务完成</text>
<text x="740" y="874" font-size="18">14:08 — 15:20</text><text x="740" y="908" font-size="16" fill="#625E57">时间使用等宽数字；内容保持自然、清楚的阅读节奏</text>
<text x="64" y="1012" font-size="22" font-weight="600">避免</text>
<text x="64" y="1054" font-size="18">拉伸字标、改成阶梯 i 点、给兔子加平滑滤镜、用浅绿或浅暖红写小字</text>
<text x="64" y="1092" font-size="18">把选择涂成完成绿、闪烁录音灯、装饰波形、持续扫光、用厚重卡片堆叠内容</text>
<text x="64" y="1150" font-size="15" fill="#625E57">Design resources v1 · 2026-10-31 · 产品 ICON 保留折耳歪头，与双竖耳倾听动画独立。</text>
</g></svg>`;
await save(join(out,'usage.svg'),usage);
await save(join(out,'usage.png'),await sharp(Buffer.from(usage)).png().toBuffer());
execFileSync(process.execPath,[join(root,'scripts/build_design_preview.mjs')],{cwd:root,stdio:'inherit'});
const files=[];
async function walk(dir){for(const entry of await readdir(dir,{withFileTypes:true})){const path=join(dir,entry.name);if(entry.isDirectory())await walk(path);else if(entry.isFile()&&path!==join(out,'manifest.json')){const bytes=await readFile(path);files.push({path:relative(out,path),bytes:(await stat(path)).size,sha256:createHash('sha256').update(bytes).digest('hex')});}}}
await walk(out);files.sort((a,b)=>a.path.localeCompare(b.path));
await save(join(out,'manifest.json'),JSON.stringify({schema:1,designVersion:1,sources:{wordmark:'src/assets/brand/wordmark.svg',icon:'assets/loppi-icon-source.png',rabbit:'src/assets/rabbit/manifest.json',tokens:'src/tokens.css'},files},null,2)+'\n');
console.log(`Exported ${files.length} assets to design/system-v1`);
