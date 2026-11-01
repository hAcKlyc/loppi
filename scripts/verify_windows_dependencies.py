"""Require a complete app-local PE dependency closure; host VC DLLs never count.

Windows 11 inbox DLLs are explicit. API-set contracts belong to the supported OS.
Checks both regular and delay imports, without loading any binary.
"""
import argparse,json,re
from pathlib import Path
from windows_pe import inspect
SYSTEM=set('advapi32 bcrypt bcryptprimitives combase comctl32 crypt32 cryptbase d2d1 d3d11 d3d12 d3dcompiler_47 dbghelp dhcpcsvc dnsapi dwrite dwmapi dxcore dxgi gdi32 gdi32full hid imm32 iphlpapi kernel32 kernelbase mmdevapi msimg32 msvcrt msvcp_win ncrypt netapi32 normaliz nsi ntdll ole32 oleaut32 opengl32 powrprof propsys psapi rpcrt4 secur32 setupapi shell32 shcore shlwapi sspicli ucrtbase user32 userenv usp10 uxtheme version win32u windowscodecs winhttp wininet winmm winspool wintrust wldap32 ws2_32 wtsapi32'.split())

def verify(directory):
    directory=Path(directory);images=[p for p in directory.rglob('*') if p.suffix.lower() in ['.exe','.dll']]
    inspected=[inspect(path) for path in images];missing=[]
    for info in inspected:
        if info['architecture']!='x64':raise ValueError(f'non-x64 bundled image: {info["path"]}')
        local={p.name.lower() for p in Path(info['path']).parent.iterdir() if p.is_file()}
        for name in info['imports']+info['delay_imports']:
            name=name.lower()
            if name in local:continue
            # Exclude every redistributable VC runtime spelling from the OS set.
            vc=re.match(r'(vcruntime|msvcp|concrt|vccorlib|vcomp)\d',name)
            system=not vc and (name.removesuffix('.dll') in SYSTEM or name.startswith(('api-ms-win-','ext-ms-win-')))
            if not system:missing.append({'image':info['path'],'dependency':name})
    for info in inspected:info['path']=str(Path(info['path']).relative_to(directory)).replace('\\','/')
    for item in missing:item['image']=str(Path(item['image']).relative_to(directory)).replace('\\','/')
    return {'schema':1,'passed':not missing,'images':inspected,'missing':missing,'system_baseline':'Windows 11 x64; explicit inbox list/API sets; VC runtime requires app-local files'}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('directory',type=Path);parser.add_argument('--report',type=Path);args=parser.parse_args();result=verify(args.directory)
    text=json.dumps(result,indent=2)+'\n'
    if args.report:args.report.write_text(text,encoding='utf8')
    else:print(text,end='')
    raise SystemExit(0 if result['passed'] else 1)
