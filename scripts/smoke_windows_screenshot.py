"""Native screenshot/OCR and HWND ownership acceptance on a synthetic covered desktop."""
import ctypes,json,os,subprocess,time,tkinter as tk,concurrent.futures
from ctypes import wintypes
from pathlib import Path
from windows_ipc import request

root_path=Path.cwd();runtime=root_path/'build/windows-dev-runtime';directory=root_path/'build/windows-screenshot-test';data=directory/'data';directory.mkdir(parents=True,exist_ok=True)
env=os.environ.copy();env.update(LOPPI_DATA_DIR=str(data),LOPPI_MODEL_ROOT=str(root_path/'build/cache/windows/models'),LOPPI_TEST_PARENT_PID=str(os.getpid()))
user=ctypes.WinDLL('user32');user.SetProcessDpiAwarenessContext.argtypes=[ctypes.c_void_p];user.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
user.GetAncestor.argtypes=[wintypes.HWND,wintypes.UINT];user.GetAncestor.restype=wintypes.HWND
user.GetShellWindow.restype=wintypes.HWND
core=subprocess.Popen([str(runtime/'loppi-core.exe'),'serve','--on-demand'],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
root=tk.Tk();root.attributes('-fullscreen',True);root.attributes('-topmost',True);root.configure(background='white')
tk.Label(root,text='LOPPI WINDOWS OCR 12345',font=('Segoe UI',48),background='white',foreground='black').pack(pady=180)
tk.Label(root,text='Synthetic screenshot acceptance',font=('Segoe UI',36),background='white',foreground='black').pack(pady=24)
tk.Label(root,text='离线截图测试',font=('Microsoft YaHei',40),background='white',foreground='black').pack(pady=24)
overlay=tk.Toplevel(root);overlay.geometry('600x150+70+70');overlay.attributes('-topmost',True);overlay.configure(background='red');tk.Label(overlay,text='DO NOT CAPTURE',font=('Segoe UI',28),background='red').pack(pady=40)
root.update();time.sleep(1);root.update();hwnd=user.GetAncestor(overlay.winfo_id(),2)
report={'schema':1,'real_windows':True,'synthetic_desktop':True,'checks':[]}
try:
    # A foreign HWND must be rejected before hiding or reading anything.
    rejected=request(data,{'type':'capture_screenshot','excluded_window_ids':[user.GetShellWindow()&0xffffffff],'anchor_ms':None})
    assert rejected['type']=='error',rejected
    report['checks'].append({'name':'foreign-window-rejected','response':rejected})
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
        result=pool.submit(request,data,{'type':'capture_screenshot','excluded_window_ids':[hwnd&0xffffffff],'anchor_ms':None})
        while not result.done():root.update();time.sleep(.01)
        captured=result.result()
    if captured['type']!='moment_saved':raise RuntimeError(captured)
    moment=captured['moment'];assert moment['ocr_state']=='complete',moment
    text=moment['ocr_text'];assert '12345' in text and 'WINDOWS' in text.upper(),text
    assert 'DO NOT CAPTURE' not in text.upper(),text
    report['checks'].append({'name':'native-png-local-ocr-window-exclusion','moment':moment})
    report['passed']=True
finally:
    overlay.destroy();root.destroy()
    try:request(data,{'type':'quit'});core.wait(timeout=15)
    except Exception:
        core.kill();core.wait()
    (directory/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
print(json.dumps(report,ensure_ascii=True,indent=2))
