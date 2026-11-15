#!/usr/bin/env python3
"""Opt-in real macOS processes; isolated DB, no capture or credentials."""
import argparse
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import uuid
from test_upgrade_lifecycle import rpc, wait_status


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--core', type=Path, default=Path('target/debug/loppi-core'))
    args = parser.parse_args()
    core = args.core.resolve()
    version = json.loads(subprocess.check_output([core, '--build-info']))['protocol_version']
    with tempfile.TemporaryDirectory(prefix='loppi-login-') as tmp:
        directory = Path(tmp)
        env = os.environ | {'LOPPI_DATA_DIR': tmp, 'LOPPI_TEST_PARENT_PID': str(os.getpid())}
        children = []
        stop = threading.Event()
        def spawn(mode):
            child = subprocess.Popen([core, 'serve', mode], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            children.append(child)
            return child
        def contenders():
            while not stop.is_set():
                spawn('--on-demand')
                try:
                    rpc(directory, version, {'type': 'status'})
                except (OSError, ValueError):
                    pass  # Transport can disconnect once during handoff.
                stop.wait(.01)
        try:
            ordinary = spawn('--on-demand')
            wait_status(directory, version)
            # Prove that a pending startup reservation excludes ordinary starts.
            import fcntl
            with (directory / 'core-start.lock').open('a') as gate:
                fcntl.flock(gate, fcntl.LOCK_EX)
                rpc(directory, version, {'type': 'quit'})
                ordinary.wait(timeout=10)
                excluded = spawn('--on-demand')
                assert excluded.wait(timeout=5) == 0
                assert not (directory / 'core.sock').exists(), 'ordinary startup ignored pending handoff'
            ordinary = spawn('--on-demand')
            wait_status(directory, version)
            flood = threading.Thread(target=contenders)
            flood.start()
            login = spawn('--login')
            # The login process itself must make the ordinary owner leave.
            assert ordinary.wait(timeout=8) == 0
            wait_status(directory, version)
            assert login.poll() is None
            status = rpc(directory, version, {'type': 'yield_to_login'})
            assert status.get('code') == 'login_owner_present', 'login did not become owner'
            duplicate = spawn('--login')
            assert duplicate.wait(timeout=5) == 0
            stop.set()
            flood.join(timeout=5)
            status = rpc(directory, version, {'type': 'status'})
            assert not status['state']['microphone_recording'] and not status['state']['system_recording']
            rpc(directory, version, {'type': 'quit'})
            assert login.wait(timeout=10) == 0
            restarted = spawn('--on-demand')
            wait_status(directory, version)
            # Close the reply end before Core completes its quit transaction.
            db = sqlite3.connect(directory / 'loppi.sqlite', isolation_level=None)
            quit_id = str(uuid.uuid4())
            def logged(fragment):
                return any(quit_id in line and fragment in line
                           for path in (directory / 'logs').glob('loppi*.jsonl')
                           for line in path.read_text().splitlines())
            try:
                db.execute('BEGIN IMMEDIATE')
                with socket.socket(socket.AF_UNIX) as connection:
                    connection.connect(str(directory / 'core.sock'))
                    connection.sendall(json.dumps(dict(version=version, id=quit_id, client='gui',
                                                       request={'type': 'lifecycle', 'action': 'quit'})).encode())
                    connection.shutdown(socket.SHUT_WR)
                    deadline = time.monotonic() + 3
                    while not logged('"event":"request_received"'):
                        assert time.monotonic() < deadline, 'Core did not admit the quit request'
                        time.sleep(.01)
                db.execute('ROLLBACK')
            finally:
                db.close()
            assert restarted.wait(timeout=10) == 0, 'accepted quit must release the owner without a reply'
            assert logged('"error_code":"ipc_write_'), 'fixture did not disconnect the server-side reply'
            replacement = spawn('--on-demand')
            wait_status(directory, version)
            rpc(directory, version, {'type': 'quit'})
            assert replacement.wait(timeout=10) == 0
            print('PASS startup exclusion, login takeover under contention, duplicate login and restart after lost quit reply')
        finally:
            stop.set()
            if 'flood' in locals():
                flood.join(timeout=5)
            for child in children:
                if child.poll() is None:
                    child.terminate()
                child.wait(timeout=10)

if __name__ == '__main__':
    main()
