#!/usr/bin/env python3
"""Opt-in real X-ASR regression. Synthetic audio only; no Core, DB or cloud.

Pass --models pointing to a prepared speech-xasr-v1 directory.
"""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, default=Path('target/debug/loppi-worker'))
    parser.add_argument('--models', type=Path, required=True)
    args = parser.parse_args()
    worker = str(args.worker.resolve())
    models = str(args.models.resolve())
    lengths = [0, 1, 32, 1056, 2080, 3200, 16000, 32, 16000]
    requests = bytearray()
    for count in lengths:
        header = json.dumps(dict(source='microphone', session='synthetic',
                                 start_sample=0, samples=count, flush=True)).encode()
        requests += struct.pack('<I', len(header)) + header + bytes(count * 4)
    result = subprocess.run([worker, 'recognize', models, '--parent-pid', str(os.getpid())],
                            input=requests, capture_output=True, timeout=120)
    assert result.returncode == 0, (result.returncode, result.stderr[-2000:])
    remaining = result.stdout
    for count in lengths:
        size, = struct.unpack('<I', remaining[:4])
        text = json.loads(remaining[4:4 + size])
        assert text == '', (count, text)
        remaining = remaining[4 + size:]
    assert not remaining
    # Root holds stdin open after the actual owner exits. EOF cannot mask a
    # broken watchdog. Test both initialization and a confirmed loaded model.
    program = r"""import json,os,struct,subprocess,sys,time
reader,writer=int(sys.argv[3]),int(sys.argv[4])
p=subprocess.Popen([sys.argv[1],'recognize',sys.argv[2],'--parent-pid',str(os.getpid())],stdin=reader,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
print(p.pid,flush=True)
if sys.argv[5]=='loaded':
    header=json.dumps(dict(source='microphone',session='synthetic',start_sample=0,samples=0,flush=True)).encode()
    os.write(writer,struct.pack('<I',len(header))+header)
    size,=struct.unpack('<I',p.stdout.read(4))
    assert json.loads(p.stdout.read(size)) == ''
    print('loaded',flush=True)
else:
    time.sleep(0.01)
os._exit(0)
"""
    for phase in ['initializing', 'loaded']:
        reader, writer = os.pipe()
        pid = None
        owner = subprocess.Popen(['python3', '-c', program, worker, models,
                                  str(reader), str(writer), phase],
                                 pass_fds=(reader, writer), stdout=subprocess.PIPE, text=True)
        os.close(reader)
        try:
            pid = int(owner.stdout.readline())
            owner.wait(timeout=120)
            assert owner.returncode == 0
            if phase == 'loaded':
                assert owner.stdout.read().strip() == 'loaded'
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    pid = None
                    break
                time.sleep(0.05)
            else:
                raise AssertionError('orphan worker remained alive')
        finally:
            if owner.poll() is None:
                owner.kill()
                owner.wait()
            if pid is not None:
                try:
                    os.kill(pid, 9)
                except ProcessLookupError:
                    pass
            os.close(writer)
            owner.stdout.close()
    print(json.dumps(dict(lengths=lengths, sequential_responses=len(lengths),
                         orphan_during_load=True, orphan_after_load=True)))


if __name__ == '__main__':
    main()
