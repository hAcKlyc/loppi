#!/usr/bin/env python3
"""Exercise real CLI/IPC export in an isolated data directory, without capture."""
import json
import os
from pathlib import Path
import sqlite3
import socket
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent
CLI = Path(os.environ.get('HEAR_TEST_CLI', ROOT/'target/debug/hear'))
CORE = Path(os.environ.get('HEAR_TEST_CORE', ROOT/'target/debug/hear-core'))


def main():
    with tempfile.TemporaryDirectory(prefix='hear-diagnose-') as directory:
        data=Path(directory)
        env=os.environ|{'HEAR_TEST_PARENT_PID':str(os.getpid()),'HEAR_DATA_DIR':directory,'HEAR_CORE_BIN':str(CORE)}
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
            # Exercise real IPC phase failures. Only the isolated Core is used;
            # no capture, provider request or private content is involved.
            # Core and log worker share one process run identity and one started record.
            for _ in range(40):
                if (data/'diagnostics.json').exists():
                    break
                time.sleep(.05)
            snapshot = json.loads((data/'diagnostics.json').read_text())
            entries = [json.loads(line) for line in (data/'logs/hear.jsonl').read_text().splitlines()]
            started_entries = [r for r in entries if r['component']=='core' and r['event']=='started']
            assert len(started_entries)==1
            assert started_entries[0]['run_id']==snapshot['run_id']
            version = report['report']['core_build']['protocol_version']
            def envelope(request):
                identity = str(uuid.uuid4())
                return identity, json.dumps(dict(version=version, id=identity,
                                                 client='cli', request=request)).encode()
            def wire(payload, close_reader=False):
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(5)
                    client.connect(str(data/'core.sock'))
                    if close_reader:
                        client.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 1024)
                    client.sendall(payload)
                    client.shutdown(socket.SHUT_WR)
                    if close_reader:
                        assert client.recv(1) # Close only after the response starts writing.
                    else:
                        response = b''
                        try:
                            while chunk := client.recv(8192):
                                response += chunk
                        except ConnectionResetError:
                            pass
                        return json.loads(response) if response else None
            wire(b'{"secret":"synthetic-private-marker"')
            execute_id, payload = envelope(dict(type='usage_statistics', boundaries=[]))
            wire(payload)
            # Semantic errors and database failures have different codes; both
            # are logged even when the dispatcher returns Response::Error.
            cases = [
                (dict(type='timeline', from_ms=100, to_ms=1), 'invalid_time_range'),
                (dict(type='event_detail', event_id=str(uuid.uuid4()), offset=0, limit=30), 'event_not_found'),
                (dict(type='calendar_days', boundaries=[]), 'invalid_calendar_range'),
            ]
            checked = []
            for request, expected in cases:
                identity, payload = envelope(request)
                response = wire(payload)
                assert response['code'] == expected, response
                checked.append((identity, request['type'], expected, expected))
            # A missing schema used to be mislabeled "invalid time range" or
            # "event changed". Simulate it only in this temporary database.
            with sqlite3.connect(data/'hear.sqlite') as db:
                db.execute('ALTER TABLE named_events RENAME TO unavailable_events')
            for request in [dict(type='timeline', from_ms=0, to_ms=1000), cases[1][0]]:
                identity, payload = envelope(request)
                response = wire(payload)
                assert response['code'] == 'query_failed', response
                checked.append((identity, request['type'], 'query_failed', 'db_schema'))
            # Force SQLite's actual three-second progress-handler deadline in
            # both GUI paths; a result-independent recursive aggregate cannot
            # finish within the deadline even on a fast machine.
            with sqlite3.connect(data/'hear.sqlite') as db:
                db.execute("""CREATE VIEW named_events AS
                    WITH RECURSIVE slow(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM slow WHERE x<1000000000)
                    SELECT '00000000-0000-0000-0000-000000000001' AS id,
                    0 AS started_ms,1000 AS ended_ms,'fixture' AS title,sum(x) AS revision,
                    1 AS source_mask,NULL AS summary,NULL AS deleted_ms FROM slow""")
            for request in [dict(type='timeline', from_ms=0, to_ms=1000),
                            dict(type='event_detail', event_id='00000000-0000-0000-0000-000000000001', offset=0, limit=30)]:
                identity, payload = envelope(request)
                response = wire(payload)
                assert response['code'] == 'query_timeout', response
                checked.append((identity, request['type'], 'query_timeout', 'query_timeout'))
            with sqlite3.connect(data/'hear.sqlite') as db:
                db.execute('DROP VIEW named_events')
                db.execute('ALTER TABLE unavailable_events RENAME TO named_events')
            # A response larger than the socket buffer makes write failure
            # deterministic; closing a tiny status response can race success.
            seed = dict(timestamp_ms=now, run_id=str(uuid.uuid4()), elapsed_ms=0,
                        client='core', request_id=str(uuid.uuid4()), component='synthetic',
                        level='info', event='x'*80, error_code='x'*80,
                        correlation_id=str(uuid.uuid4()), job_id=str(uuid.uuid4()),
                        capture_span_id=str(uuid.uuid4()))
            with (data/'logs/hear.jsonl').open('a') as logs:
                logs.write((json.dumps(seed)+'\n')*500)
            write_id, payload = envelope(dict(type='logs', filter=dict(limit=500)))
            wire(payload, close_reader=True)
            with socket.socket(socket.AF_UNIX) as stalled:
                stalled.settimeout(5)
                stalled.connect(str(data/'core.sock'))
                stalled.sendall(b'{')
                assert stalled.recv(1) == b''
            deadline = time.monotonic() + 3
            while True:
                records = [json.loads(line) for line in (data/'logs/hear.jsonl').read_text().splitlines()]
                failures = {r.get('error_code'): r for r in records if r.get('component') == 'ipc'}
                if all(code in failures for code in ['ipc_invalid_json','ipc_execute_failed','ipc_write_disconnected','ipc_read_timeout']):
                    break
                assert time.monotonic() < deadline, list(failures)
                time.sleep(0.05)
            for code, identity in [('ipc_execute_failed',execute_id),('ipc_write_disconnected',write_id)]:
                assert failures[code]['request_id'] == identity
                assert failures[code]['correlation_id'] == identity
                assert failures[code]['level']=='warn'
            assert failures['ipc_execute_failed']['context']['command'] == 'usage_statistics'
            assert failures['ipc_write_disconnected']['context']['command'] == 'logs'
            for identity, expected_command, code, cause in checked:
                record = next(r for r in records if r.get('request_id') == identity and r.get('error_code') == code)
                assert record['level'] == 'warn' and record['correlation_id'] == identity
                assert record['context']['command'] == expected_command
                assert record['context']['cause'] == cause
                assert record['duration_ms'] >= 0 and record['context']['output_bytes'] > 0
            assert 'synthetic-private-marker' not in (data/'logs/hear.jsonl').read_text()
            with sqlite3.connect(f'file:{data}/hear.sqlite?mode=ro',uri=True) as db:
                assert db.execute('select count(*) from recording_sessions').fetchone()[0]==0
                assert db.execute('select count(*) from cloud_tasks').fetchone()[0]==0
            print('diagnostics: read-only export, private logs, IPC phase failures and correlation passed')
        finally:
            command('quit')


if __name__=='__main__':
    main()
