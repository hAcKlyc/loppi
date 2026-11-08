#!/usr/bin/env python3
"""Isolated IPC lifecycle regression; no models, credentials or capture permission.
Build loppi-core + loppi-cli first. The first test crosses a real minute boundary.
"""
import json
import os
import socket
import sqlite3
import subprocess
import tempfile
import time
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLI = Path(os.environ.get('LOPPI_TEST_CLI', ROOT / 'target/debug/loppi'))
CORE = Path(os.environ.get('LOPPI_TEST_CORE', ROOT / 'target/debug/loppi-core'))


def main():
    with tempfile.TemporaryDirectory(prefix='loppi-schedule-ipc-') as directory:
        env = os.environ | {"LOPPI_TEST_PARENT_PID": str(os.getpid()), 'LOPPI_DATA_DIR': directory, 'LOPPI_CORE_BIN': str(CORE)}
        path = Path(directory)

        def cli(*args):
            return json.loads(subprocess.check_output([str(CLI), *args], env=env, text=True))

        def request(body, client='gui'):
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(5)
                sock.connect(str(path / 'core.sock'))
                sock.sendall(json.dumps({'version': json.loads(subprocess.check_output([str(CORE),'--build-info']))['protocol_version'], 'id': str(uuid.uuid4()),
                                         'client': client, 'request': body}).encode())
                sock.shutdown(socket.SHUT_WR)
                return json.loads(sock.makefile().read())

        def query(sql, *args):
            with sqlite3.connect(path / 'loppi.sqlite') as db:
                return db.execute(sql, args).fetchall()

        def quit():
            cli('quit')
            for _ in range(200):
                if not (path / 'core.sock').exists():
                    return
                time.sleep(.025)
            raise AssertionError('isolated core did not stop')

        def hh(minute):
            return f'{minute // 60:02d}:{minute % 60:02d}'

        process = subprocess.Popen([str(CORE), 'serve', '--on-demand'], env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(200):
            if (path / 'core.sock').exists():
                break
            if process.poll() is not None:
                raise AssertionError('isolated core exited before listening')
            time.sleep(.025)
        try:
            initial = cli('status')['state']
            assert not initial['recording_schedule']['enabled']
            assert initial['recording_schedule']['weekday_mask'] == 127
            request({'type': 'app_opened'})  # Before first enabling the rule.
            now = time.time()
            boundary = (int(now) // 60 + 1) * 60
            start = (time.localtime(boundary).tm_hour * 60 + time.localtime(boundary).tm_min)
            stop = (start + 10) % 1440
            saved = cli('schedule', 'on', '--start', hh(start), '--stop', hh(stop))['state']
            assert not saved['enabled_intent'] and not saved['paused_intent']
            request({'type': 'app_opened'})  # Existing host refresh cannot start immediately.
            if time.time() < boundary:
                assert not cli('status')['state']['enabled_intent']
            time.sleep(max(0, boundary + 1.2 - time.time()))
            started = cli('status')['state']
            assert started['enabled_intent'] and not started['paused_intent']
            assert not started['microphone_recording'] and not started['system_recording']
            assert started['capture_blocker'] == 'model_not_ready'
            print('PASS: GUI opened before first enable; save and repeated refresh leave intent unchanged; future minute executes', flush=True)

            cli('pause')
            cursor = query('SELECT cursor_ms FROM recording_schedule')[0][0]
            for _ in range(2):
                request({'type': 'app_opened'})
            assert cli('status')['state']['paused_intent']
            assert query('SELECT cursor_ms FROM recording_schedule')[0][0] == cursor

            # Restart the actual Core while leaving the host lifecycle active.
            # The existing host refresh sends AppOpened, with retry supplied by
            # EntryRuntime's existing loop (also exercised by desktop tests).
            process.kill()
            process.wait(timeout=5)
            query('UPDATE recording_schedule SET cursor_ms=0')
            queried = cli('status')['state']
            assert queried['paused_intent']
            time.sleep(.4)
            assert query('SELECT cursor_ms FROM recording_schedule')[0][0] == 0
            assert request({'type': 'app_opened'}, 'cli')['code'] == 'gui_required'
            restored = request({'type': 'app_opened'})['state']
            assert restored['enabled_intent'] and not restored['paused_intent']
            assert query('SELECT cursor_ms FROM recording_schedule')[0][0] > 0
            cli('pause')
            quit()
            cli('status')
            assert request({'type': 'app_opened'})['state']['paused_intent']
            print('PASS: query-only never consumes; host refresh restores restarted Core; processed boundary cannot override later manual pause', flush=True)

            friday = cli('schedule', 'off', '--days', 'fri', '--start', '22:00', '--stop', '06:00')['state']
            assert friday['recording_schedule']['weekday_mask'] == 16
            kept = cli('schedule', 'off', '--stop', '07:00')['state']
            assert kept['recording_schedule']['weekday_mask'] == 16
            invalid = request({'type': 'set_recording_schedule', 'enabled': False, 'start_minute': 1320,
                               'stop_minute': 420, 'weekday_mask': 0, 'expected_revision': kept['revision']})
            assert invalid['code'] == 'invalid_schedule'
            assert cli('status')['state']['recording_schedule']['weekday_mask'] == 16
            print('PASS: CLI weekday selection, omission preserves days, and empty mask is rejected', flush=True)
        finally:
            quit()
            process.wait(timeout=5)


if __name__ == '__main__':
    main()
