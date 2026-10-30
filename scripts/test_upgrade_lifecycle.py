#!/usr/bin/env python3
"""Real legacy Core → GUI lifecycle → current Core, using disposable data only."""
import argparse
import concurrent.futures
import fcntl
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import uuid


def rpc(directory, version, request):
    request_id = str(uuid.uuid4())
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(8)
        sock.connect(str(directory / 'core.sock'))
        sock.sendall(json.dumps(dict(version=version, id=request_id, client='gui', request=request)).encode())
        sock.shutdown(socket.SHUT_WR)
        chunks = []
        while chunk := sock.recv(65536):
            chunks.append(chunk)
    reply = json.loads(b''.join(chunks))
    assert reply['id'] == request_id
    return reply


def wait_status(directory, version):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            return rpc(directory, version, {'type': 'status'})
        except (OSError, ValueError):
            time.sleep(.05)
    raise AssertionError('Core did not start')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--core', type=Path, default=Path('target/debug/hear-core'))
    parser.add_argument('--driver', type=Path, default=Path('target/debug/examples/gui_lifecycle'))
    parser.add_argument('--legacy17', type=Path, required=True)
    parser.add_argument('--legacy19', type=Path, required=True)
    args = parser.parse_args()
    core, driver = args.core.resolve(), args.driver.resolve()
    build = json.loads(subprocess.check_output([core, '--build-info']))
    version = build['protocol_version']

    def drive(directory, *action, **overrides):
        env = os.environ | {"HEAR_TEST_PARENT_PID": str(os.getpid()), 'HEAR_DATA_DIR': str(directory), 'HEAR_CORE_BIN': str(core)} | overrides
        return subprocess.run([driver, *action], env=env, capture_output=True, text=True, timeout=45)

    for old_version, binary in [(17, args.legacy17), (19, args.legacy19)]:
        for mode in ['--on-demand', '--login']:
            for action in ['upgrade', 'quit']:
                with tempfile.TemporaryDirectory(prefix='hear-upgrade-') as tmp:
                    directory = Path(tmp)
                    process = subprocess.Popen([binary.resolve(), 'serve', mode], env=os.environ | {"HEAR_TEST_PARENT_PID": str(os.getpid()), 'HEAR_DATA_DIR': tmp}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    try:
                        wait_status(directory, old_version)
                        before = rpc(directory, old_version, {'type':'pause', 'expected_revision':0})['state']
                        before = rpc(directory, old_version, {'type':'set_source', 'source':'microphone', 'enabled':False, 'expected_revision':before['revision']})['state']
                        if old_version == 19:
                            now = time.localtime()
                            minute = now.tm_hour * 60 + now.tm_min
                            before = rpc(directory, old_version, {'type':'set_recording_schedule', 'enabled':True, 'start_minute':(minute+60)%1440, 'stop_minute':(minute+120)%1440, 'expected_revision':before['revision']})['state']
                        marker = directory / 'retained-audio.opus'
                        marker.write_bytes(b'unchanged archived audio fixture')
                        if action == 'upgrade':
                            # Mixed bundles must not stop an existing service.
                            bad = drive(directory, HEAR_CORE_BIN=str(binary.resolve()))
                            assert bad.returncode != 0 and process.poll() is None, bad.stderr
                            with concurrent.futures.ThreadPoolExecutor(2) as pool:
                                results = list(pool.map(lambda _: drive(directory), range(2)))
                            for result in results:
                                assert result.returncode == 0, result.stderr
                                state = json.loads(result.stdout)['state']
                                for field in ['revision', 'enabled_intent', 'paused_intent', 'microphone_enabled', 'system_enabled']:
                                    assert state[field] == before[field], (field, state, before)
                            process.wait(timeout=10)
                            info = rpc(directory, 999, {'type':'lifecycle','action':'inspect'})
                            assert info['build'] == build and not info['allow_capture']
                            opened = drive(directory, 'opened')
                            assert opened.returncode == 0, opened.stderr
                            opened_state = json.loads(opened.stdout)['state']
                            for field in ['enabled_intent', 'paused_intent', 'microphone_enabled', 'system_enabled']:
                                assert opened_state[field] == before[field], (field, opened_state, before)
                            # Existing GUI activation legitimately enables the
                            # scheduler. Handoff must not disable that behavior.
                            activated = rpc(directory, version, {'type':'lifecycle','action':'inspect'})
                            assert activated['normal_run']
                            assert activated['allow_capture'] == (old_version == 19)
                            assert marker.read_bytes() == b'unchanged archived audio fixture'
                        result = drive(directory, 'quit')
                        assert result.returncode == 0, result.stderr
                        process.wait(timeout=10)
                        assert not (directory / 'core.sock').exists()
                        # Quit on an absent Core must not launch a replacement.
                        assert drive(directory, 'quit').returncode == 0
                        assert not (directory / 'core.sock').exists()
                        print(f'PASS legacy {old_version} {mode} {action}: state retained, owner drained, no relaunch', flush=True)
                    finally:
                        if process.poll() is None:
                            process.terminate()
                            process.wait(timeout=10)
                        if (directory / 'core.sock').exists():
                            drive(directory, 'quit')

    with tempfile.TemporaryDirectory(prefix='hear-upgrade-busy-') as tmp:
        directory = Path(tmp)
        with (directory / 'lifecycle.lock').open('w') as gate:
            fcntl.flock(gate, fcntl.LOCK_EX)
            started = time.monotonic()
            result = drive(directory)
            assert result.returncode != 0 and '稍后重试' in result.stderr
            assert time.monotonic() - started < 40
            assert not (directory / 'core.sock').exists()
        print('PASS bounded busy-owner coordination timeout; no Core launched', flush=True)

    # Controlled stable lifecycle server covers active/query permissions and a
    # newer build. The real replacement Core still loads the same real DB path.
    for permission in ['capture', 'normal', 'query', 'newer']:
        with tempfile.TemporaryDirectory(prefix='hear-handoff-') as tmp:
            directory = Path(tmp)
            owner_lock = (directory / 'core.lock').open('w')
            fcntl.flock(owner_lock, fcntl.LOCK_EX)
            server = socket.socket(socket.AF_UNIX)
            server.bind(str(directory / 'core.sock'))
            server.listen()
            server.settimeout(.2)
            stopped = threading.Event()
            actions = []
            fake_build = build | {'built_at_epoch': build['built_at_epoch'] + (1 if permission == 'newer' else -1)}

            def serve():
                while not stopped.is_set():
                    try:
                        conn, _ = server.accept()
                    except socket.timeout:
                        continue
                    with conn:
                        chunks = []
                        while chunk := conn.recv(65536):
                            chunks.append(chunk)
                        value = json.loads(b''.join(chunks))
                        req = value['request']
                        if req['type'] == 'status':
                            reply = {'id':value['id'], 'type':'status'}
                        else:
                            actions.append(req['action'])
                            reply = {'id':value['id'], 'type':'lifecycle','build':fake_build,'allow_capture':permission == 'capture','normal_run':permission in ['capture','normal']}
                        conn.sendall(json.dumps(reply).encode())
                    if req.get('action') == 'handoff':
                        server.close()
                        (directory / 'core.sock').unlink()
                        owner_lock.close()
                        return
            thread = threading.Thread(target=serve)
            thread.start()
            try:
                result = drive(directory)
                if permission == 'newer':
                    assert result.returncode != 0 and '更新' in result.stderr, result.stderr
                    assert 'handoff' not in actions and 'quit' not in actions
                else:
                    assert result.returncode == 0, result.stderr
                    info = rpc(directory, version, {'type':'lifecycle','action':'inspect'})
                    assert info['allow_capture'] == (permission == 'capture')
                    assert info['normal_run'] == (permission in ['capture','normal'])
                    assert actions.count('handoff') == 1
                    assert drive(directory, 'quit').returncode == 0
                print(f'PASS stable handoff {permission}', flush=True)
            finally:
                stopped.set()
                thread.join(timeout=5)
                server.close()
                owner_lock.close()


if __name__ == '__main__':
    main()
