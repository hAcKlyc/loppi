import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, symlinkSync, rmSync, openSync, closeSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, basename } from 'node:path';
import { execFileSync } from 'node:child_process';
import { prepareRelease, uploadDraft, verifyRemote, publishRelease, verifyPublished, githubClient } from './publish_update.mjs';
import { releaseMetadata, releaseNotes } from './release_metadata.mjs';
import { preflight, finalize, releasePlan } from './release_ci.mjs';
import { platforms, sha256, writePlatformManifest } from './updater-artifacts.mjs';

function fixture() {
  const temporary = mkdtempSync(join(tmpdir(), 'loppi-release-pipeline-'));
  const root = join(temporary, 'project'); mkdirSync(root);
  const key = join(temporary, 'fixture.key');
  execFileSync(process.execPath, [resolve('node_modules/@tauri-apps/cli/tauri.js'), 'signer', 'generate', '--ci', '--password', '', '-w', key], { stdio: 'pipe' });
  for (const file of ['package.json','package-lock.json','Cargo.toml','Cargo.lock','src-tauri/Cargo.toml','src-tauri/tauri.conf.json','src-tauri/native/LoppiCore-Info.plist']) {
    mkdirSync(join(root, file, '..'), { recursive: true });
    writeFileSync(join(root, file), readFileSync(file));
  }
  const config = JSON.parse(readFileSync(join(root,'src-tauri/tauri.conf.json')));
  config.plugins.updater.pubkey = readFileSync(`${key}.pub`, 'utf8').trim();
  writeFileSync(join(root,'src-tauri/tauri.conf.json'), JSON.stringify(config));
  writeFileSync(join(root,'.gitignore'), 'build/\nnode_modules/\n');
  const version = JSON.parse(readFileSync(join(root,'package.json'))).version;
  writeFileSync(join(root,'CHANGELOG.md'),`## ${version} — 2026-11-02\n\n- Synthetic user-visible change\n`);
  symlinkSync(resolve('node_modules'), join(root,'node_modules'), process.platform === 'win32' ? 'junction' : 'dir');
  const git = args => execFileSync('git', ['-c',`core.hooksPath=${join(temporary,'no-hooks')}`,'-c','user.name=Release Fixture','-c','user.email=fixture@example.invalid','-c','commit.gpgsign=false',...args], { cwd: root, stdio: 'pipe', encoding: 'utf8' }).trim();
  git(['init']);git(['add','.']);git(['commit','-m','synthetic version sources']);
  const metadata = releaseMetadata(root, `v${version}`);
  const notes = join(temporary,'notes.txt');writeFileSync(notes,'Synthetic release changes\n');
  const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: readFileSync(key,'utf8'), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' };
  const manifests = platforms.map((target,index) => {
    const platform = ['macos-arm64','macos-x86_64','windows-x64'][index];
    const directory = join(root,'build/releases',version,platform);mkdirSync(directory,{recursive:true});
    const artifact = join(directory, index===2 ? `loppi_${version}_x64-setup.exe` : `loppi_${version}_${platform}.app.tar.gz`);
    writeFileSync(artifact,`SYNTHETIC ${platform}`);
    const updater = writePlatformManifest({root,artifact,version,source:metadata.source,targets:[target],env});
    const report = {version,source:metadata.source,dirty:false,updater};
    if(index<2){report.dmg=`loppi_${version}_${platform}.dmg`;const bytes=Buffer.from(`SYNTHETIC DMG ${platform}`);writeFileSync(join(directory,report.dmg),bytes);report.dmg_bytes=bytes.length;report.dmg_sha256=sha256(bytes);}
    writeFileSync(join(directory,'release-info.json'),JSON.stringify(report));
    return join(directory,'updater.json');
  });
  return { temporary,root,notes,manifests,metadata,dispose:()=>rmSync(temporary,{recursive:true,force:true}) };
}
function remote(prepared) {
  const state={draft:true,tag_name:prepared.tag,prerelease:false,assets:[]};
  const data=new Map();let uploads=0,publishes=0,isPublic=true;
  const client={release:()=>structuredClone(state),source:()=>prepared.source,isPublic:()=>isPublic,latest:()=>({tag_name:prepared.tag}),
    upload:(_tag,files)=>{uploads++;for(const file of files){const bytes=readFileSync(file),name=basename(file);data.set(name,bytes);state.assets.push({name,size:bytes.length,state:'uploaded',digest:`sha256:${sha256(bytes)}`});}},
    publish:()=>{publishes++;state.draft=false;},bytes:(_tag,asset)=>data.get(asset.name)};
  return {client,state,data,get uploads(){return uploads;},get publishes(){return publishes;},private:()=>{isPublic=false;}};
}

test('complete signed release includes installers, reports, manifest/checksums; prepare and upload are repeatable', async()=>{
 const f=fixture();try{
  const prepared=prepareRelease(f.root,f.notes,f.manifests), again=prepareRelease(f.root,f.notes,f.manifests);
  assert.equal(prepared.assets.length,13);assert.deepEqual(prepared.manifest,again.manifest);
  assert.equal(prepared.manifest.pub_date,f.metadata.pubDate);
  const checksums=readFileSync(join(prepared.destination,'SHA256SUMS'),'utf8');
  for(const asset of prepared.assets.filter(a=>a.name!=='SHA256SUMS'))assert(checksums.includes(`${asset.sha256}  ${asset.name}\n`));
  const r=remote(prepared);uploadDraft(prepared,r.client);assert.equal(r.uploads,1);uploadDraft(prepared,r.client);assert.equal(r.uploads,1);
  const endpoint=async(url,options)=>{assert.equal(url,'https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json');assert.equal(options.headers,undefined);return new Response(r.data.get('latest.json'));};
  await publishRelease(prepared,r.client,endpoint);assert.equal(r.publishes,1);await publishRelease(prepared,r.client,endpoint);assert.equal(r.publishes,1);
  assert.throws(()=>uploadDraft(prepared,r.client),/draft/);
  await assert.rejects(verifyPublished(prepared,r.client,async()=>new Response('{}')),/differs/);
  await assert.rejects(verifyPublished(prepared,r.client,async()=>new Response('',{status:404})),/unavailable/);
 }finally{f.dispose();}
});
test('remote corruption, incomplete resources, private publication and dirty/mismatched reports stop release',async()=>{
 const f=fixture();try{
  const prepared=prepareRelease(f.root,f.notes,f.manifests),r=remote(prepared);
  assert.throws(()=>verifyRemote(prepared,r.client,{draft:true}),/missing/);
  uploadDraft(prepared,r.client);const first=r.state.assets[0],digest=first.digest;
  first.digest='sha256:wrong';assert.throws(()=>uploadDraft(prepared,r.client),/differs/);assert.equal(r.uploads,1);
  delete first.digest;verifyRemote(prepared,r.client,{draft:true});first.digest=digest;
  r.state.assets.push({...first,name:'unrecognized.exe'});assert.throws(()=>verifyRemote(prepared,r.client),/Unexpected/);r.state.assets.pop();
  r.private();await assert.rejects(publishRelease(prepared,r.client),/public repository/);assert.equal(r.publishes,0);
  const file=join(f.manifests[0],'..','release-info.json'),report=JSON.parse(readFileSync(file));report.dirty=true;writeFileSync(file,JSON.stringify(report));
  assert.throws(()=>prepareRelease(f.root,f.notes,f.manifests),/clean source/);
 }finally{f.dispose();}
});
test('portable release metadata validates every version source and exact dated CHANGELOG section',()=>{
 const f=fixture();try{
  assert.throws(()=>releaseMetadata(f.root,'v9.9.9'),/tag/);
  const lock=join(f.root,'Cargo.lock');writeFileSync(lock,readFileSync(lock,'utf8').replace(/\r?\n/g,'\r\n'));
  assert.equal(releaseMetadata(f.root,f.metadata.tag).version,f.metadata.version);
  const p=join(f.root,'src-tauri/native/LoppiCore-Info.plist');writeFileSync(p,readFileSync(p,'utf8').replace(f.metadata.version,'9.9.9'));
  assert.throws(()=>releaseMetadata(f.root,f.metadata.tag),/sources disagree/);
  const v=f.metadata.version;
  for(const header of [`## ${v} — 2026-11-02（UTC+08:00）`,`## [${v}] - 2026-11-02`])assert.equal(releaseNotes(`# Changes\n\n${header}\n\n- User-visible change\n\n## 0.0.1 - 2020-01-01\n- Old\n`,v),`${header}\n\n- User-visible change\n`);
  assert.throws(()=>releaseNotes(`## ${v}\n- missing date`,v),/dated/);
  assert.throws(()=>releaseNotes('## 未发行\n- Planned',v),/dated/);
 }finally{f.dispose();}
});

test('only main prepares new versions; published versions skip and source conflicts stop',()=>{
 const f=fixture();try{
  const client={maybeRelease:()=>null,maybeSource:()=>null,isPublic:()=>true};
  assert.throws(()=>preflight(f.root,client,'refs/heads/dev'),/only from main/);
  assert.equal(preflight(f.root,client,'refs/heads/main').shouldRelease,true);
  assert.equal(preflight(f.root,{...client,maybeRelease:()=>({draft:false})},'refs/heads/main').shouldRelease,false);
  assert.throws(()=>preflight(f.root,{...client,isPublic:()=>false},'refs/heads/main'),/public repository/);
  assert.throws(()=>releasePlan(f.metadata,null,'other-source'),/another commit/);
  assert.throws(()=>releasePlan(f.metadata,{draft:true,target_commitish:'main'},null),/not bound/);
 }finally{f.dispose();}
});

test('main assembles every platform before tagging, then publishes and verifies without a second approval',async()=>{
 const f=fixture();try{
  const prepared=prepareRelease(f.root,f.notes,f.manifests),r=remote(prepared);let hasTag=false,hasDraft=false;
  const order=[];const client={...r.client,maybeSource:()=>hasTag?prepared.source:null,maybeRelease:()=>hasDraft?r.client.release():null,
    createTag:(tag,source)=>{assert.equal(tag,prepared.tag);assert.equal(source,prepared.source);hasTag=true;order.push('tag');},
    createDraft:()=>{hasDraft=true;order.push('draft');},
    upload:(...args)=>{order.push('upload');r.client.upload(...args);},publish:()=>{order.push('publish');r.client.publish();}};
  const request=async()=>{order.push('anonymous-check');return new Response(r.data.get('latest.json'));};
  await assert.rejects(finalize(f.root,client,'refs/heads/dev',f.notes,f.manifests,request),/only from main/);
  assert.deepEqual(order,[]);
  await assert.rejects(finalize(f.root,client,'refs/heads/main',f.notes,f.manifests.slice(0,2),request),/both macOS/);
  assert.deepEqual(order,[],'missing Windows never creates a tag or public release');
  await finalize(f.root,client,'refs/heads/main',f.notes,f.manifests,request);
  assert.deepEqual(order,['tag','draft','upload','publish','anonymous-check']);
  await finalize(f.root,client,'refs/heads/main',f.notes,f.manifests,request);
  assert.equal(r.publishes,1,'retry after publication verifies without publishing again');
 }finally{f.dispose();}
});

test('GitHub adapter dereferences actual annotated tags and gates on the exact main Verify result',async()=>{
 const source='a'.repeat(40),calls=[];let failed=false;
 const temporary=mkdtempSync(join(tmpdir(),'loppi-watch-output-')),output=join(temporary,'watch.log');
 const execute=(_program,args,options)=>{
  calls.push(args);
  if(args[0]==='api'&&args[1].includes('/git/ref/tags/'))return JSON.stringify({object:{type:'tag',sha:'annotated-tag'}});
  if(args[0]==='api'&&args[1].includes('/git/tags/'))return JSON.stringify({object:{type:'commit',sha:source}});
  if(args[0]==='run'&&args[1]==='list'){
   assert.equal(args[args.indexOf('--commit')+1],source);assert.equal(args[args.indexOf('--branch')+1],'main');
   return JSON.stringify([{databaseId:1,headSha:'old',status:'completed',conclusion:'success'},{databaseId:2,headSha:source,status:'completed',conclusion:failed?'failure':'success'}]);
  }
  if(args[0]==='run'&&args[1]==='watch'){
   assert.equal(args[2],'2');assert.equal(options.stdio,'inherit','watch progress must not use a captured buffer');
   const fd=openSync(output,'w');
   try{return execFileSync(process.execPath,['-e','process.stdout.write(Buffer.alloc(2*1024*1024,120))'],{...options,stdio:['ignore',fd,'ignore']});}finally{closeSync(fd);}
  }
  if(args[0]==='run'&&args[1]==='view')return JSON.stringify({headSha:source,conclusion:'success'});
  throw Error('Unexpected fixture command');
 };
 try{
  const client=githubClient('.',execute);assert.equal(client.source('v1.2.3'),source);await client.waitForVerification(source);
  assert.equal(statSync(output).size,2*1024*1024);assert(calls.some(args=>args[1]==='view'),'final exact-SHA result reached after sustained watcher output');
  failed=true;await assert.rejects(client.waitForVerification(source),/failed or was cancelled/);
  assert.equal(calls.filter(args=>args[1]==='watch').length,1);
 }finally{rmSync(temporary,{recursive:true,force:true});}
});
