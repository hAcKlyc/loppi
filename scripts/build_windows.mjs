// Windows build owner. Asset fetching happens here, never in the installed app.
import {createHash} from 'node:crypto';
import {existsSync,readFileSync,writeFileSync,mkdirSync,cpSync,readdirSync,statSync} from 'node:fs';
import {resolve,join,dirname} from 'node:path';
import {spawnSync,execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const spec=JSON.parse(readFileSync(join(root,'config/windows-assets.json')));
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
function run(program,args,env=process.env){const result=spawnSync(program,args,{cwd:root,env,stdio:'inherit',windowsHide:true});if(result.error)throw result.error;if(result.status!==0)throw new Error(`${program} failed (${result.status})`);}
function output(program,args){return execFileSync(program,args,{cwd:root,encoding:'utf8',windowsHide:true}).trim();}
function asset(path,item){mkdirSync(dirname(path),{recursive:true});if(!existsSync(path)||statSync(path).size!==item.bytes||hash(path)!==item.sha256){run('curl.exe',['--fail','--location','--retry','3','--silent','--show-error',item.url,'--output',path]);}if(statSync(path).size!==item.bytes||hash(path)!==item.sha256)throw new Error(`asset checksum mismatch: ${path}`);}
function files(directory){return readdirSync(directory,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?files(join(directory,entry.name)):[join(directory,entry.name)]);}
export async function main(argv=process.argv.slice(2)){
  if(process.platform!=='win32'||process.arch!=='x64')throw new Error('Windows 11 x64 build host required');
  process.chdir(root);
  const [command='release',...args]=argv;if(!['build','release'].includes(command)||args.length)throw new Error('Usage: node scripts/build_windows.mjs [build|release]');
  const release=command==='release';
  if(release&&output('git',['status','--porcelain']))throw new Error('Release requires a clean committed checkout');
  const version=JSON.parse(readFileSync('package.json')).version;
  if(version!==JSON.parse(readFileSync('src-tauri/tauri.conf.json')).version)throw new Error('package versions disagree');
  const cache=resolve('build/cache/windows');mkdirSync(cache,{recursive:true});
  const archive=join(cache,'sherpa-onnx-2.0.0.tar.bz2');asset(archive,spec.native);run('tar',['-xjf',archive,'-C',cache]);
  const native=join(cache,spec.native.directory,'lib');
  run(process.execPath,['scripts/prepare_windows_ocr.mjs']);
  const ocr=join(cache,'ocr-runtime');
  for(const language of spec.ocr.languages)asset(join(ocr,'tessdata',language.path),language);
  run(process.execPath,['scripts/prepare_windows_assets.mjs']);
  const epoch=process.env.LOPPI_BUILD_EPOCH??Math.floor(Date.now()/1000).toString();
  const env={...process.env,SHERPA_ONNX_LIB_DIR:native,LOPPI_BUILD_EPOCH:epoch,CARGO_PROFILE_DEV_DEBUG:'0',CARGO_INCREMENTAL:'0',CARGO_BUILD_JOBS:process.env.CARGO_BUILD_JOBS??'2'};
  // Build all four identities from the same source and epoch; no external task mutates this stage.
  run('cargo',['build','--locked','--release','--target',spec.target,'-p','loppi-core','-p','loppi-worker','-p','loppi-cli'],env);
  const stage=resolve('build/windows',`${spec.target}-${epoch}-${process.pid}`);mkdirSync(stage,{recursive:true});
  for(const name of ['loppi-core.exe','loppi-worker.exe','loppi.exe'])cpSync(join('target',spec.target,'release',name),join(stage,name));
  for(const name of readdirSync(native).filter(n=>n.endsWith('.dll')))cpSync(join(native,name),join(stage,name));
  const vswhere='C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe';
  const visualStudio=output(vswhere,['-latest','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-property','installationPath']);
  const redist=join(visualStudio,'VC/Redist/MSVC',spec.vc_runtime.version_directory);
  const crt=files(join(redist,'x64')).filter(path=>/Microsoft\.VC[0-9]+\.CRT[\\/]/i.test(path));
  for(const item of spec.vc_runtime.files){const source=crt.find(path=>path.endsWith(item.name));if(!source||statSync(source).size!==item.bytes||hash(source)!==item.sha256)throw new Error(`missing or changed pinned Microsoft redistributable ${item.name}`);cpSync(source,join(stage,item.name));}
  const license=join(cache,'Visual-C-V14-License.docx');asset(license,spec.vc_runtime.license);
  mkdirSync(join(stage,'resources/licenses'),{recursive:true});cpSync(license,join(stage,'resources/licenses/Microsoft-VC-runtime.docx'));
  writeFileSync(join(stage,'resources/licenses/Microsoft-VC-runtime-provenance.json'),JSON.stringify(spec.vc_runtime,null,2)+'\n');
  cpSync(join(cache,'models'),join(stage,'resources/models'),{recursive:true});
  const ocrStage=join(stage,'resources/ocr');mkdirSync(join(ocrStage,'tessdata'),{recursive:true});
  for(const name of readdirSync(ocr).filter(n=>n==='tesseract.exe'||n.endsWith('.dll')))cpSync(join(ocr,name),join(ocrStage,name));
  for(const language of spec.ocr.languages)cpSync(join(ocr,'tessdata',language.path),join(ocrStage,'tessdata',language.path));
  cpSync(join(ocr,'licenses'),join(ocrStage,'licenses'),{recursive:true});
  cpSync(join(ocr,'provenance.json'),join(ocrStage,'provenance.json'));
  cpSync('licenses',join(stage,'resources/licenses'),{recursive:true});cpSync('THIRD_PARTY_NOTICES.md',join(stage,'resources/THIRD_PARTY_NOTICES.md'));
  writeFileSync(join(stage,'resources/ocr/models-provenance.json'),JSON.stringify(spec.ocr.languages,null,2)+'\n');
  run('python',['scripts/verify_windows_dependencies.py',stage,'--report',join(stage,'resources/pe-dependencies.json')]);
  const inventory=files(stage).map(path=>({path:path.slice(stage.length+1).replaceAll('\\','/'),bytes:statSync(path).size,sha256:hash(path)}));
  writeFileSync(join(stage,'resources/windows-assets.json'),JSON.stringify({schema:1,minimum_build:spec.minimum_build,files:inventory},null,2)+'\n');
  const resources=Object.fromEntries(readdirSync(stage).map(name=>[join(stage,name).replaceAll('\\','/'),name]));
  const config=join(stage,'bundle-config.json');writeFileSync(config,JSON.stringify({bundle:{resources}},null,2));
  run(process.execPath,['node_modules/typescript/bin/tsc','--noEmit'],env);run(process.execPath,['node_modules/vite/bin/vite.js','build'],env);
  run(process.execPath,['node_modules/@tauri-apps/cli/tauri.js','build','--target',spec.target,'--bundles','nsis','--config',config],env);
  const binary=resolve('target',spec.target,'release/loppi-desktop.exe');
  cpSync(binary,join(stage,'loppi-desktop.exe'));
  run('python',['scripts/verify_windows_dependencies.py',stage,'--report',resolve('build/windows/pe-dependencies-final.json')]);
  const identities={};for(const[role,name]of Object.entries({gui:'loppi-desktop.exe',core:'loppi-core.exe',worker:'loppi-worker.exe',cli:'loppi.exe'}))identities[role]=JSON.parse(output(join(stage,name),[role==='cli'?'build-info':'--build-info']));
  const source=output('git',['rev-parse','HEAD']);const expected={...identities.core};delete expected.component;
  for(const[role,info]of Object.entries(identities)){const actual={...info};delete actual.component;if(JSON.stringify(actual)!==JSON.stringify(expected)||info.git_sha!==source||(release && info.dirty!==false))throw new Error(`mixed component identity: ${role}`);}
  const artifacts=files(resolve('target',spec.target,'release/bundle/nsis')).filter(p=>p.endsWith('.exe'));
  const report={schema:1,version,source,epoch,dirty:expected.dirty,platform:'windows-x64',minimum_build:spec.minimum_build,authenticode:'unsigned',stage,components:identities,artifacts:artifacts.map(path=>({path,bytes:statSync(path).size,sha256:hash(path)}))};
  writeFileSync(resolve('build/windows/release-info-windows-x64.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))await main();
