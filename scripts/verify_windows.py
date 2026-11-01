"""Offline Windows checks. Native capture/GUI/install acceptance is opt-in in smoke_windows_*.

Uses disposable test owners; never calls cloud providers or starts user recording.
"""
import argparse,json,os,subprocess,sys,time
from pathlib import Path

ROOT=Path(__file__).resolve().parent.parent

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--suite',choices=['all','static','test','smoke'],default='all')
    parser.add_argument('--report',type=Path)
    args=parser.parse_args()
    if sys.platform!='win32':parser.error('Windows host required')
    env={k:v for k,v in os.environ.items() if not k.startswith('LOPPI_TEST_') and k not in ['LOPPI_DATA_DIR','LOPPI_CORE_BIN']}
    env.update(LOPPI_TEST_PARENT_PID=str(os.getpid()),CARGO_PROFILE_DEV_DEBUG='0',CARGO_INCREMENTAL='0',LOPPI_BUILD_EPOCH=str(int(time.time())))
    native=ROOT/'build/cache/windows/sherpa-onnx-v2.0.0-win-x64-shared-MD-Release-lib/lib'
    if 'SHERPA_ONNX_LIB_DIR' not in env:env['SHERPA_ONNX_LIB_DIR']=str(native)
    checks=[];result=0
    def run(name,command):
        print(f'[{name}]',flush=True);start=time.monotonic()
        completed=subprocess.run(command,cwd=ROOT,env=env,timeout=1800)
        checks.append({'name':name,'command':command,'seconds':round(time.monotonic()-start,2),'exit_code':completed.returncode})
        completed.check_returncode()
    try:
        if args.suite in ['all','static']:
            run('typecheck',['node','node_modules/typescript/bin/tsc','--noEmit'])
            run('lint',['node','node_modules/@biomejs/biome/bin/biome','check','src'])
            run('rust-check',['cargo','check','--workspace','--locked'])
        if args.suite in ['all','test']:
            run('rust-regressions',['cargo','test','--workspace','--locked'])
            run('frontend-regressions',['node','--test',*[str(p.relative_to(ROOT)) for p in sorted((ROOT/'scripts').glob('test_*.mjs'))]])
            run('rabbit-assets',['node','scripts/test_rabbit_assets.cjs'])
            run('python-regressions',[sys.executable,'-m','unittest','discover','-s','scripts','-p','test_*.py'])
        if args.suite in ['all','smoke']:
            run('frontend-build',['node','node_modules/vite/bin/vite.js','build'])
            run('binary-build',['cargo','build','--workspace','--locked'])
            run('component-identities',[sys.executable,'scripts/verify_build_identity.py','--directory','target/debug'])
    except (OSError,subprocess.SubprocessError) as error:
        print(f'verification failed: {error}',file=sys.stderr);result=1
    if args.report:
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps({'schema':1,'suite':args.suite,'passed':result==0,'checks':checks,'excluded':['native audio/GUI/screenshot/install acceptance; run smoke_windows_* explicitly','cloud and private fixtures']},indent=2)+'\n',encoding='utf8')
    return result

if __name__=='__main__':raise SystemExit(main())
