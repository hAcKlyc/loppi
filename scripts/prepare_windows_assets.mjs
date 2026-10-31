// Build-time only downloads; the installed app never downloads inference assets.
import { readFileSync, existsSync, mkdirSync, writeFileSync, renameSync, statSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
const lock = readFileSync('config/models.lock.json');
const models = JSON.parse(lock);
const root = resolve('build/cache/windows/models');
mkdirSync(root, {recursive:true});
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
async function fetchAsset(asset) {
  const path=join(root,asset.path); mkdirSync(dirname(path),{recursive:true});
  if(existsSync(path)&&statSync(path).size===asset.bytes&&hash(path)===asset.sha256)return;
  const pending=`${path}.download`;
  await new Promise((yes,no)=>{
    const child=spawn('curl.exe',['--fail','--location','--retry','3','--silent','--show-error',asset.url,'--output',pending],{stdio:'inherit',windowsHide:true});
    child.on('error',no);child.on('exit',code=>code===0?yes():no(new Error(`download failed: ${asset.path}`)));
  });
  if(statSync(pending).size!==asset.bytes||hash(pending)!==asset.sha256)throw new Error(`model checksum mismatch: ${asset.path}`);
  renameSync(pending,path); console.log(`verified ${asset.path}`);
}
const queue=[...models.files];
await Promise.all(Array.from({length:3},async()=>{for(let asset;asset=queue.shift();)await fetchAsset(asset);}));
writeFileSync(join(root,'models.lock.json'),lock);
console.log(root);
