#!/usr/bin/env python3
"""Verify bundled offline readiness with isolated data, no capture or cloud.

The renamed helper copy uses read-only model files. No historical recordings
or installed user data are read. Tests actual VAD/ASR loading and explicit retry.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    args = parser.parse_args()
    version = int(re.search(r'PROTOCOL_VERSION: u32 = (\d+)', (ROOT / 'crates/hear-protocol/src/lib.rs').read_text())[1])
    with tempfile.TemporaryDirectory(prefix='hear-offline-') as tmp:
        directory = Path(tmp)
        helper = directory / '离线 测试.app'
        shutil.copytree(args.bundle / 'Contents/Library/LoginItems/HearCore.app', helper)
        model_root = helper / 'Contents/Resources/models'
        lock = json.loads((model_root / 'models.lock.json').read_text())
        for entry in lock['files']:
            path = model_root / entry['path']
            assert hashlib.sha256(path.read_bytes()).hexdigest() == entry['sha256']
        for path in model_root.rglob('*'):
            path.chmod(0o555 if path.is_dir() else 0o444)
        model_root.chmod(0o555)
        data = directory / 'data'
        env = os.environ | {"HEAR_TEST_PARENT_PID": str(os.getpid()), 'HEAR_DATA_DIR': str(data), 'HEAR_MODEL_ROOT': '/invalid-development-cache'}
        process = subprocess.Popen(
            [str(helper / 'Contents/MacOS/hear-core'), 'serve', '--on-demand'],
            cwd='/', env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        )

        def query(request):
            with socket.socket(socket.AF_UNIX) as stream:
                stream.settimeout(8)
                stream.connect(str(data / 'core.sock'))
                stream.sendall(json.dumps({'version': version, 'id': str(uuid.uuid4()), 'client': 'cli', 'request': request}).encode())
                stream.shutdown(socket.SHUT_WR)
                chunks = []
                while chunk := stream.recv(65536):
                    chunks.append(chunk)
                return json.loads(b''.join(chunks))

        def ready():
            deadline = time.monotonic() + 100
            while time.monotonic() < deadline:
                assert process.poll() is None, process.stderr.read().decode()
                if (data / 'core.sock').exists():
                    state = query({'type': 'models'})['state']
                    assert state['phase'] != 'failed', state
                    if state['phase'] == 'ready' and state['speaker_phase'] == 'ready':
                        return
                time.sleep(.1)
            raise AssertionError('Bundled models did not become ready')

        try:
            ready()
            query({'type': 'prepare_models'})
            query({'type': 'prepare_models'})
            ready()
            state = query({'type': 'status'})['state']
            assert not state['enabled_intent'] and not state['microphone_recording'] and not state['system_recording']
            assert not (data / 'models').exists(), 'Runtime copied models to user data'
            with sqlite3.connect(data / 'hear.sqlite') as db:
                for table in ('recording_sessions', 'audio_blocks', 'coarse_segments', 'cloud_tasks'):
                    assert db.execute('SELECT count(*) FROM ' + table).fetchone()[0] == 0, table
            for entry in lock['files']:
                assert hashlib.sha256((model_root / entry['path']).read_bytes()).hexdigest() == entry['sha256']
            print('PASS: bundled read-only models, renamed Unicode path, automatic VAD/ASR readiness + retry; no model copy, capture or cloud tasks')
        finally:
            if process.poll() is None:
                process.terminate()
            process.communicate(timeout=15)
            for path in model_root.rglob('*'):
                if path.is_dir():
                    path.chmod(0o755)
            model_root.chmod(0o755)


if __name__ == '__main__':
    main()
