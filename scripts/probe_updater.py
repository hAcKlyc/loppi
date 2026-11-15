"""Disposable desktop acceptance on the host OS. Run in an isolated checkout."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from threading import Thread
import time

ROOT = Path.cwd()
assert (ROOT / '.loppi-updater-probe').is_file(), 'use a disposable checkout'
windows = sys.platform == 'win32'
target = 'windows-x86_64' if windows else 'darwin-aarch64'
out = Path(tempfile.mkdtemp(prefix='loppi-updater-'))
gui = out / 'data/gui'
gui.mkdir(parents=True)
(gui / 'onboarding.json').write_text(json.dumps({'schema_version': 1, 'completed': True}))
cli = ROOT / 'node_modules/@tauri-apps/cli/tauri.js'
env = {key: value for key, value in os.environ.items() if not key.startswith(('TAURI_SIGNING_', 'LOPPI_TEST_'))}
env.update(LOPPI_TEST_PARENT_PID=str(os.getpid()), LOPPI_DATA_DIR=str(out/'data'),
           LOPPI_UPDATER_PROBE_DIR=str(out), CARGO_INCREMENTAL='0',
           WEBVIEW2_USER_DATA_FOLDER=str(out/'webview'), CARGO_PROFILE_DEV_DEBUG='0')

def replace(name, old, new):
    path = ROOT / name
    contents = path.read_text(encoding='utf-8')
    assert contents.count(old) == 1, (name, old)
    path.write_text(contents.replace(old, new), encoding='utf-8')

key = out / 'test.key'
subprocess.run(['node', str(cli), 'signer', 'generate', '--ci', '--password', '', '-w', str(key)],
               env=env, stdout=subprocess.DEVNULL, check=True)
env['TAURI_SIGNING_PRIVATE_KEY'] = key.read_text().strip()
env['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] = ''
packages = {}
for version in ('0.1.29', '0.1.30'):
    artifact = out / (version + ('.exe' if windows else '.app.tar.gz'))
    artifact.write_bytes(('disposable signed updater bytes '+version).encode())
    subprocess.run(['node', str(cli), 'signer', 'sign', '--app-version', version, str(artifact)],
                   env=env, stdout=subprocess.DEVNULL, check=True)
    packages[version] = (artifact.read_bytes(), Path(str(artifact)+'.sig').read_text().strip())
env.pop('TAURI_SIGNING_PRIVATE_KEY')
env.pop('TAURI_SIGNING_PRIVATE_KEY_PASSWORD')
key.unlink()

versions = []
downloads = {version: [] for version in packages}
mode = {'version': None, 'lookup_failures': 1}

class Feed(BaseHTTPRequestHandler):
    def send(self, status, data=b'', content_type='application/octet-stream'):
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == '/latest':
            versions.append(time.monotonic())
            if mode['lookup_failures']:
                mode['lookup_failures'] -= 1
                self.send(503)
            elif mode['version'] is None:
                self.send(204)
            else:
                version = mode['version']
                self.send(200, json.dumps({
                    'version': version,
                    'platforms': {target: {'url': f'{base}/package/{version}', 'signature': packages[version][1]}}
                }).encode(), 'application/json')
            return
        version = self.path.removeprefix('/package/')
        if version not in packages:
            self.send(404)
            return
        downloads[version].append(time.monotonic())
        if version == '0.1.29' and len(downloads[version]) <= 2:
            self.send(503)
            return
        content = packages[version][0]
        self.send(200, content if version == '0.1.29' else content+b'tampered')

    def log_message(self, *_):
        pass

server = ThreadingHTTPServer(('127.0.0.1', 0), Feed)
base = f'http://127.0.0.1:{server.server_port}'
Thread(target=server.serve_forever, daemon=True).start()

# Keep the ten-second startup delay and accelerate the other three timings.
# The proposed patch is applied only in this probe checkout.
schedule = 'src-tauri/src/updater/schedule.rs'
replace(schedule, 'secs(30 * 60)', 'secs(4)')
replace(schedule, 'secs(10 * 60)', 'secs(2)')
replace(schedule, 'secs(2 * 60 * 60)', 'secs(9)')
replace(schedule, 'wall: SystemTime::now(),', '''
            wall: SystemTime::now() + Duration::from_secs(
                PROBE_WALL_OFFSET.load(std::sync::atomic::Ordering::Relaxed)
            ),''')
with (ROOT / schedule).open('a', encoding='utf-8') as handle:
    handle.write('\npub(super) static PROBE_WALL_OFFSET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);\n')
with (ROOT / 'src-tauri/src/updater.rs').open('a', encoding='utf-8') as handle:
    handle.write('\npub(crate) fn probe_advance_wall() {\n    schedule::PROBE_WALL_OFFSET.store(30, std::sync::atomic::Ordering::Relaxed);\n}\n')
with (ROOT / 'src-tauri/src/entry.rs').open('a', encoding='utf-8') as handle:
    handle.write('''
pub(crate) fn probe_native_wake(app: &tauri::AppHandle) {
    unsafe extern "C" {
        fn loppi_probe_native_wake(window: *mut std::ffi::c_void);
    }
    let window = app.get_webview_window("float").unwrap();
    unsafe { loppi_probe_native_wake(window.native_window().unwrap()); }
}
''')
native = ROOT / ('src-tauri/native/windows.cpp' if windows else 'src-tauri/native/entry.m')
with native.open('a', encoding='utf-8') as handle:
    handle.write('''
extern "C" void loppi_probe_native_wake(void *window) {
    SendMessageW((HWND)window, WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, 0);
}
''' if windows else '''
void loppi_probe_native_wake(void *window) {
    (void)window;
    [NSWorkspace.sharedWorkspace.notificationCenter postNotificationName:NSWorkspaceDidWakeNotification
                                                                     object:NSWorkspace.sharedWorkspace];
}
''')
lib = 'src-tauri/src/lib.rs'
replace(lib, '            updater::start(app.handle().clone());', '''
            updater::start(app.handle().clone());
            updater_probe(app.handle().clone());''')
replace(lib, '            updater::update_status,', '''
            probe_update_button,
            updater::update_status,''')
with (ROOT / lib).open('a', encoding='utf-8') as handle:
    handle.write(r'''
#[tauri::command]
fn probe_update_button(value: String) {
    let path = std::path::PathBuf::from(std::env::var_os("LOPPI_UPDATER_PROBE_DIR").unwrap());
    std::fs::write(path.join("button"), value).unwrap();
}

fn updater_probe(app: tauri::AppHandle) {
    use std::time::Duration;
    let path = std::path::PathBuf::from(std::env::var_os("LOPPI_UPDATER_PROBE_DIR").unwrap());
    tauri::async_runtime::spawn(async move {
        let main = app.get_webview_window("main").unwrap();
        tokio::time::sleep(Duration::from_millis(700)).await;
        main.close().unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!main.is_visible().unwrap());
        std::fs::write(path.join("hidden"), b"yes").unwrap();
        for signal in ["wake", "show", "quit"] {
            while !path.join(signal).exists() {
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
            let host = app.clone();
            app.run_on_main_thread(move || match signal {
                "wake" => {
                    assert!(!host.get_webview_window("main").unwrap().is_visible().unwrap());
                    updater::probe_advance_wall();
                    entry::probe_native_wake(&host);
                }
                "show" => {
                    let main = host.get_webview_window("main").unwrap();
                    main.show().unwrap();
                    let _ = main.set_focus();
                    main.eval(r#"
                      {let remaining=20;
                       const timer=setInterval(()=>{
                         const title=document.querySelector('.restart-update-button')?.title;
                         if(title || --remaining<=0) {
                           clearInterval(timer);
                           void window.__TAURI_INTERNALS__.invoke('probe_update_button',{value:title||document.body.innerText.slice(0,300)||'empty'});
                         }
                       },250);}
                    "#).unwrap();
                }
                _ => host.exit(0),
            }).unwrap();
        }
    });
}
''')
overlay = {
    'identifier': 'app.loppi.updater-probe',
    'plugins': {'updater': {'dangerousInsecureTransportProtocol': True,
                            'endpoints': [base+'/latest'],
                            'pubkey': Path(str(key)+'.pub').read_text().strip()}}
}
env['TAURI_CONFIG'] = json.dumps(overlay)
subprocess.run(['npm.cmd' if windows else 'npm', 'run', 'build'], check=True, env=env,
               shell=windows)
subprocess.run(['cargo', 'build', '-p', 'loppi-desktop', '--features', 'tauri/custom-protocol', '--locked'],
               env=env, check=True)
binary = ROOT / 'target/debug' / ('loppi-desktop.exe' if windows else 'loppi-desktop')

def wait_for(predicate, seconds=45):
    deadline = time.monotonic()+seconds
    while time.monotonic()<deadline:
        if predicate():
            return
        if app.poll() is not None:
            break
        time.sleep(0.05)
    raise AssertionError({'app_exit':app.poll(), 'lookups':len(versions),
                          'downloads':{k:len(v) for k,v in downloads.items()},
                          'files':[p.name for p in out.iterdir()]})

start = time.monotonic()
with (out/'app.log').open('w') as output:
    app = subprocess.Popen([binary], env=env, stdout=output, stderr=subprocess.STDOUT)
    try:
        wait_for(lambda: (out/'hidden').exists())
        wait_for(lambda: len(versions)>=2)
        assert versions[0]-start >= 9.5 and 1.7 <= versions[1]-versions[0] < 3.7, versions
        mode['version'] = '0.1.29'
        wait_for(lambda: len(downloads['0.1.29']) >= 3)
        first, second, third = downloads['0.1.29']
        assert 1.7 <= second-first < 3.7, (first, second)
        assert third-second >= 8.7
        assert any(second+1 < checked < third-1 for checked in versions), versions
        mode['version'] = '0.1.30'
        before = len(versions)
        (out/'wake').touch()
        wait_for(lambda: len(downloads['0.1.30']) == 1)
        assert len(versions) == before+1 and versions[-1]-third < 3.5, versions
        wait_for(lambda: len(versions) >= before+2)
        assert len(downloads['0.1.30']) == 1, 'bad signature retried'
        assert len(downloads['0.1.29']) == 3
        (out/'show').touch()
        wait_for(lambda: (out/'button').exists())
        assert '0.1.29' in (out/'button').read_text(), (out/'button').read_text()
        (out/'quit').touch()
        assert app.wait(timeout=30) == 0
        print(json.dumps({
            'platform':sys.platform, 'startup_after_s':round(versions[0]-start, 2),
            'lookup_retry_after_s':round(versions[1]-versions[0], 2),
            'download_retry_after_s':round(second-first, 2),
            'download_backoff_s':round(third-second, 2),
            'real_hidden_window':True, 'native_wake':True,
            'checks_continue_during_download_backoff':True,
            'bad_signature_attempts':len(downloads['0.1.30']),
            'reopened_verified_button':(out/'button').read_text(),
            'accelerated_check_interval_s':4
        }, ensure_ascii=False))
    except BaseException:
        print((out/'app.log').read_text(errors='replace')[-5000:], file=sys.stderr)
        raise
    finally:
        if app.poll() is None:
            app.terminate()
            app.wait(timeout=10)
        server.shutdown()
