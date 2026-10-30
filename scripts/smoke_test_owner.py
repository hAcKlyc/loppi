#!/usr/bin/env python3
"""Disposable Core exits when its explicit test owner dies; no Keychain/cloud."""
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
CORE = ROOT / 'target/debug/hear-core'
PROGRAM = '''import os,subprocess,sys
p=subprocess.Popen([sys.argv[1],'serve','--on-demand'],env=os.environ|{'HEAR_TEST_PARENT_PID':str(os.getpid())},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
print(p.pid,flush=True)
sys.stdin.read(1)
os._exit(0)
'''
with tempfile.TemporaryDirectory(prefix='hear-test-owner-') as directory:
    env = os.environ | {'HEAR_DATA_DIR': directory}
    owner = subprocess.Popen([sys.executable, '-c', PROGRAM, str(CORE)], env=env,
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    pid = int(owner.stdout.readline())
    exited = False
    try:
        for _ in range(200):
            if (Path(directory)/'core.sock').exists():
                break
            if owner.poll() is not None:
                raise AssertionError('test owner exited before Core readiness')
            time.sleep(.025)
        else:
            raise AssertionError('disposable Core did not become ready')
        with sqlite3.connect(Path(directory)/'hear.sqlite') as db:
            for provider, account in [('llm','deepseek'),('asr','volcengine')]:
                db.execute('UPDATE cloud_queues SET configured=1 WHERE provider=?',(provider,))
                db.execute('INSERT INTO cloud_credential_refs VALUES(?,?)',(provider,account+':00000000-0000-0000-0000-000000000001'))
        check = subprocess.run([str(CORE),'--check-credentials'], env=env|{'HEAR_TEST_PARENT_PID':str(os.getpid())},capture_output=True,text=True,timeout=10)
        report=json.loads(check.stdout)
        assert all(p['error']=='test_offline' for p in report['providers'])
        assert report['network_requests'] is False and report['user_interaction'] is False
        owner.kill()
        owner.wait(timeout=5)
        for _ in range(120):
            state=subprocess.run(['ps','-p',str(pid),'-o','stat='],capture_output=True,text=True).stdout.strip()
            if not state or state.startswith('Z'):
                exited = True
                break
            time.sleep(.025)
        else:
            raise AssertionError('Core survived its test owner')
        print('test owner death and offline credential isolation passed')
    finally:
        if owner.poll() is None:
            owner.kill()
        owner.communicate(timeout=5)
        if not exited:
            actual = subprocess.run(['ps','-p',str(pid),'-o','comm='],capture_output=True,text=True).stdout.strip()
            if actual == str(CORE):
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
