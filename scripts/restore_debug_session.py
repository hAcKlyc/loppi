#!/usr/bin/env python3
"""Restore the installed app through LaunchServices and verify capture readiness.

Consumes the status/cloud snapshots saved before a local debug acceptance run.
Never launches Core directly, edits the DB, or sends start/resume/app-opened.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import uuid


def rpc(data, request):
    rid = str(uuid.uuid4())
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(str(data / 'core.sock'))
        stream.sendall(json.dumps({'version': 22, 'id': rid, 'client': 'cli', 'request': request}).encode())
        stream.shutdown(socket.SHUT_WR)
        chunks = []
        while chunk := stream.recv(65536):
            chunks.append(chunk)
        result = json.loads(b''.join(chunks))
    if result.get('id') != rid or result.get('type') == 'error':
        raise RuntimeError(f'Unexpected Core response: {result}')
    return result


def inspect(data):
    try:
        return rpc(data, {'type': 'lifecycle', 'action': 'inspect'})
    except (FileNotFoundError, ConnectionRefusedError):
        return None


def gui_running(app):
    executable = str(app / 'Contents/MacOS/loppi-desktop')
    lines = subprocess.check_output(['ps', '-axo', 'comm='], text=True).splitlines()
    return executable in (line.strip() for line in lines)


def readiness_errors(before, state, owner, gui, build, now_ms):
    errors = []
    if not gui:
        errors.append('installed GUI is not running')
    if owner.get('build') != build:
        errors.append('installed Core build does not match')
    if not owner.get('normal_run') or not owner.get('allow_capture'):
        errors.append('normal application capture channel is not enabled')
    if state.get('capture_blocker') != 'none':
        errors.append(f"capture_blocker={state.get('capture_blocker')}")
    previous = before['recording_schedule']
    current = state.get('recording_schedule', {})
    for key in ['enabled', 'start_minute', 'stop_minute']:
        if current.get(key) != previous[key]:
            errors.append(f'recording schedule changed: {key}')
    if current.get('weekday_mask', 127) != previous.get('weekday_mask', 127):
        errors.append('recording schedule changed: weekday_mask')
    if current.get('error'):
        errors.append(f"schedule error: {current['error']}")
    # The normal scheduler may legitimately consume a boundary during testing.
    crossed = previous['enabled'] and previous.get('next_at_ms') is not None and now_ms >= previous['next_at_ms']
    for key in ['microphone_enabled', 'system_enabled'] + ([] if crossed else ['enabled_intent', 'paused_intent']):
        if state.get(key) != before[key]:
            errors.append(f'recording intent changed: {key}')
    if not crossed and current.get('paused_by_schedule') != previous.get('paused_by_schedule'):
        errors.append('pause origin changed')
    if state.get('paused_intent') and (state.get('microphone_recording') or state.get('system_recording')):
        errors.append('capture still active while paused')
    return errors


def restore(args):
    # Keep the pure readiness checks importable on every development host.
    import fcntl

    session, app = args.session_dir.resolve(), args.app.resolve()
    data = Path.home() / 'Library/Application Support/loppi'
    before = json.loads((session / 'original-status.json').read_text())['state']
    cloud = json.loads((session / 'original-cloud-status.json').read_text())['queues']
    cli = app / 'Contents/MacOS/loppi'
    core = app / 'Contents/Library/LoginItems/LoppiCore.app/Contents/MacOS/loppi-core'
    build = json.loads(subprocess.check_output([str(core), '--build-info'], text=True))
    owner = inspect(data)
    if owner and owner['build'] != build:
        debug = args.debug_core or session / 'loppi-debug.app/Contents/Library/LoginItems/LoppiCore.app/Contents/MacOS/loppi-core'
        expected = json.loads(subprocess.check_output([str(debug), '--build-info'], text=True))
        if owner['build'] != expected:
            raise RuntimeError('Another Core owns this directory; refusing to stop it')
        if gui_running(session / 'loppi-debug.app'):
            raise RuntimeError('Quit the debug GUI normally before restoring the installed app')
        rpc(data, {'type': 'lifecycle', 'action': 'quit'})
        deadline = time.monotonic() + 30
        while True:
            with (data / 'core.lock').open('a+') as lock:
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise RuntimeError('Debug Core has not released its owner lock')
            time.sleep(.1)
    # Use the installed application's real startup path. No synthetic GUI IPC.
    clean_env = {k: v for k, v in os.environ.items() if not k.startswith('LOPPI_')}
    subprocess.run(['open', '-a', str(app)], env=clean_env, check=True)
    deadline = time.monotonic() + 45
    result = {'ok': False}
    while True:
        try:
            owner = inspect(data) or {}
            state = rpc(data, {'type': 'status'})['state']
            errors = readiness_errors(before, state, owner, gui_running(app), build, int(time.time() * 1000))
            result = {'ok': not errors, 'capture_blocker': state.get('capture_blocker'),
                      'paused_intent': state.get('paused_intent'), 'schedule': state.get('recording_schedule'),
                      'gui_running': gui_running(app), 'allow_capture': owner.get('allow_capture'),
                      'normal_run': owner.get('normal_run'), 'build': owner.get('build'), 'errors': errors}
            if not errors:
                break
        except (OSError, ValueError, RuntimeError) as error:
            result = {'ok': False, 'errors': [str(error)]}
        if time.monotonic() >= deadline:
            return result
        time.sleep(.25)
    env = clean_env | {'LOPPI_DATA_DIR': str(data)}
    current = json.loads(subprocess.check_output([str(cli), 'cloud', 'status'], env=env, text=True))['queues']
    for queue in cloud:
        actual = next(q for q in current if q['provider'] == queue['provider'])
        if actual['paused'] != queue['paused']:
            subprocess.run([str(cli), 'cloud', 'pause' if queue['paused'] else 'resume', queue['provider']], env=env, check=True, stdout=subprocess.DEVNULL)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--session-dir', type=Path, required=True)
    parser.add_argument('--app', type=Path, default=Path('/Applications/loppi.app'))
    parser.add_argument('--debug-core', type=Path)
    args = parser.parse_args()
    try:
        result = restore(args)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result = {'ok': False, 'errors': [str(error)]}
    print(json.dumps(result, ensure_ascii=False, indent=2))
    (args.session_dir / 'restore-result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    sys.exit(main())
