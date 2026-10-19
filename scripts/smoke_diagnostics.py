#!/usr/bin/env python3
"""Exercise real CLI/IPC export in an isolated data directory, without capture."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT/'target/debug/hear'
CORE = ROOT/'target/debug/hear-core'


def main():
    with tempfile.TemporaryDirectory(prefix='hear-diagnose-') as directory:
        data=Path(directory)
        env=os.environ|{'HEAR_DATA_DIR':directory,'HEAR_CORE_BIN':str(CORE)}
        def command(*args, **kwargs):
            return subprocess.run([str(CLI),*args],env=env,text=True,capture_output=True,timeout=20,**kwargs)
        # No auto-start, even when the binary is available.
        absent=command('diagnose','--from-ms','0','--to-ms','10000')
        assert absent.returncode!=0 and not (data/'hear.sqlite').exists()
        try:
            command('status',check=True)
            before=json.loads(command('status',check=True).stdout)['state']
            destination=data/'report.json'
            now=int(time.time()*1000)
            args=['diagnose','--from-ms',str(now-60000),'--to-ms',str(now+1000)]
            command(*args,'--output',str(destination),check=True)
            report=json.loads(destination.read_text())
            assert report['report']['core_build']['git_sha']==report['cli_build']['git_sha']
            assert report['report']['facts']['cloud_tasks']==[]
            assert report['report']['logs_available']
            assert destination.stat().st_mode&0o777==0o600
            assert command(*args,'--output',str(destination)).returncode!=0
            assert command('diagnose','--from-ms','100','--to-ms','1').returncode!=0
            after=json.loads(command('status',check=True).stdout)['state']
            assert before['revision']==after['revision']
            assert not after['enabled_intent']
            with sqlite3.connect(f'file:{data}/hear.sqlite?mode=ro',uri=True) as db:
                assert db.execute('select count(*) from recording_sessions').fetchone()[0]==0
                assert db.execute('select count(*) from cloud_tasks').fetchone()[0]==0
            print('diagnostics: no startup, no capture, read-only IPC, private export and no overwrite passed')
        finally:
            command('quit')


if __name__=='__main__':
    main()
