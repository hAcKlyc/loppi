// Minimal PNG-only OCR runtime built from checksum-pinned upstream sources.
// No installer execution, archive/network codecs, training tools or MinGW DLLs.
import {readFileSync,writeFileSync,mkdirSync,existsSync,statSync,cpSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const lock=JSON.parse(readFileSync('config/windows-ocr-sources.json'));
const base=resolve('build/cache/windows/ocr-source'),prefix=join(base,'installed'),runtime=resolve('build/cache/windows/ocr-runtime');
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const env={...process.env,_CL_:'/MP2'};
function run(p,args){const result=spawnSync(p,args,{stdio:'inherit',env,windowsHide:true});if(result.error)throw result.error;if(result.status!==0)throw new Error(`${p} failed (${result.status})`);}
mkdirSync(base,{recursive:true});mkdirSync(join(runtime,'licenses'),{recursive:true});
for(const item of lock.sources){const archive=join(base,item.archive);if(!existsSync(archive)||statSync(archive).size!==item.bytes||hash(archive)!==item.sha256)run('curl.exe',['--fail','--location','--silent','--show-error','--retry','3',item.url,'--output',archive]);if(statSync(archive).size!==item.bytes||hash(archive)!==item.sha256)throw new Error(`OCR source checksum mismatch: ${item.name}`);run('tar',['-xzf',archive,'-C',base]);}
// CMake owns generator selection; CI supplies CMAKE_GENERATOR for its VS image.
const common=['-A','x64',`-DCMAKE_INSTALL_PREFIX=${prefix}`,`-DCMAKE_PREFIX_PATH=${prefix}`,'-DCMAKE_POLICY_DEFAULT_CMP0091=NEW','-DCMAKE_POLICY_VERSION_MINIMUM=3.5','-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded','-DBUILD_SHARED_LIBS=OFF','-DSW_BUILD=OFF'];
for(const item of lock.sources){
  const options={
    zlib:['-DZLIB_BUILD_EXAMPLES=OFF'],
    libpng:['-DPNG_SHARED=OFF','-DPNG_STATIC=ON','-DPNG_TESTS=OFF','-DPNG_TOOLS=OFF',`-DZLIB_LIBRARY=${prefix}/lib/zlibstatic.lib`],
    leptonica:['-DBUILD_PROG=OFF','-DENABLE_GIF=OFF','-DENABLE_JPEG=OFF','-DENABLE_TIFF=OFF','-DENABLE_WEBP=OFF','-DENABLE_OPENJPEG=OFF','-DSTRICT_CONF=ON',`-DZLIB_LIBRARY=${prefix}/lib/zlibstatic.lib`,`-DPNG_LIBRARY_RELEASE=${prefix}/lib/libpng16_static.lib`],
    tesseract:['-DBUILD_TRAINING_TOOLS=OFF','-DBUILD_TESTS=OFF','-DOPENMP_BUILD=OFF','-DGRAPHICS_DISABLED=ON','-DDISABLE_ARCHIVE=ON','-DDISABLE_CURL=ON','-DDISABLE_TIFF=ON','-DWIN32_MT_BUILD=ON','-DINSTALL_CONFIGS=OFF','-DENABLE_NATIVE=OFF',`-DCMAKE_EXE_LINKER_FLAGS=/MANIFEST:EMBED /MANIFESTINPUT:"${resolve('config/windows-ocr.manifest')}"`],
  }[item.name];
  const build=join(base,`${item.name}-build`);
  run('cmake',['-S',join(base,item.directory),'-B',build,...common,...options]);
  run('cmake',['--build',build,'--config','Release','--target','INSTALL','--parallel','2']);
  const license={zlib:'LICENSE',libpng:'LICENSE',leptonica:'leptonica-license.txt',tesseract:'LICENSE'}[item.name];
  cpSync(join(base,item.directory,license),join(runtime,'licenses',`${item.name}.txt`));
}
cpSync(join(prefix,'bin/tesseract.exe'),join(runtime,'tesseract.exe'));
cpSync('config/windows-ocr.manifest',join(runtime,'windows-ocr.manifest'));
writeFileSync(join(runtime,'provenance.json'),JSON.stringify({...lock,profile:'MSVC x64 Release static CRT; UTF-8 process manifest; PNG-only; no archive/curl/openmp/training',manifest_sha256:hash('config/windows-ocr.manifest'),binary_sha256:hash(join(runtime,'tesseract.exe'))},null,2)+'\n');
console.log(runtime);
