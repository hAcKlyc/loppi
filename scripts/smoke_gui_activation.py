"""Real GUI activation IPC with disposable databases and disabled audio sources.

Works on macOS and Windows; never touches default data or records audio.
Build Core and the existing gui_lifecycle example with one build epoch.
"""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time


def main():
    suffix = '.exe' if os.name == 'nt' else ''
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core', type=Path, default=Path('target/debug/loppi-core' + suffix))
    parser.add_argument('--driver', type=Path, default=Path('target/debug/examples/gui_lifecycle' + suffix))
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    core, driver = args.core.resolve(), args.driver.resolve()
    clean_env = {k: v for k, v in os.environ.items() if not k.startswith('LOPPI_TEST_') and k not in ['LOPPI_DATA_DIR', 'LOPPI_CORE_BIN']}
    cases = []
    for paused in [False, True]:
        with tempfile.TemporaryDirectory(prefix='loppi-activation-') as tmp:
            directory = Path(tmp)
            env = clean_env | {'LOPPI_DATA_DIR': tmp, 'LOPPI_CORE_BIN': str(core), 'LOPPI_TEST_PARENT_PID': str(os.getpid())}

            def call(binary, *action):
                result = subprocess.run([str(binary), *action], env=env, capture_output=True, text=True, encoding='utf8', timeout=45)
                assert result.returncode == 0, result.stderr
                return json.loads(result.stdout) if result.stdout.strip() else None

            def drive(*action):
                return call(driver, *action)

            def inspect():
                return drive('inspect')

            def state_unchanged(state):
                assert state['enabled_intent'] and state['paused_intent'] == paused
                assert not state['microphone_enabled'] and not state['system_enabled']
                assert state['capture_state'] != 'recording'

            process = None

            def launch(mode='query'):
                nonlocal process
                assert process is None
                flags = ['--on-demand'] if mode == 'query' else ['--handoff', 'normal']
                process = subprocess.Popen([str(core), 'serve', *flags], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                           creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
                deadline = time.monotonic() + 15
                while True:
                    result = subprocess.run([str(driver), 'inspect'], env=env, capture_output=True, text=True, encoding='utf8', timeout=10)
                    if result.returncode == 0:
                        return json.loads(result.stdout)
                    assert process.poll() is None and time.monotonic() < deadline, result.stderr
                    time.sleep(.05)

            def stop():
                nonlocal process
                if process is None:
                    return
                if process.poll() is None:
                    drive('quit')
                # A released owner lock is not the OS process-exit signal.
                # Wait on the actual owned child before opening its SQLite file.
                assert process.wait(timeout=15) == 0
                process = None

            try:
                # First-use intent remains off even after an explicit GUI open.
                facts = launch()
                assert not facts['allow_capture'] and not facts['normal_run']
                initial = drive()['state']
                assert not initial['enabled_intent']
                opened = drive('opened')['state']
                assert not opened['enabled_intent'] and opened['capture_state'] != 'recording'
                stop()
                # Offline fixture seeding only after the owned Core has exited.
                # Both sources stay off, so no real microphone can be activated.
                db = sqlite3.connect(directory / 'loppi.sqlite')
                try:
                    with db:
                        db.execute('UPDATE service_state SET enabled_intent=1, paused_intent=?, microphone_enabled=0, system_enabled=0 WHERE id=1', [int(paused)])
                finally:
                    db.close()
                facts = launch()
                assert not facts['allow_capture'] and not facts['normal_run']
                state_unchanged(drive('opened')['state'])
                assert inspect()['allow_capture'] and inspect()['normal_run']
                state_unchanged(drive('opened')['state'])
                stop()
                # Existing normal-only handoff must also be promoted by GUI.
                facts = launch('normal')
                assert facts['normal_run'] and not facts['allow_capture']
                state_unchanged(drive('opened')['state'])
                assert inspect()['normal_run'] and inspect()['allow_capture']
                stop()
                # The login quit marker cannot block a later explicit GUI open.
                facts = launch()
                assert not facts['normal_run'] and not facts['allow_capture']
                state_unchanged(drive('opened')['state'])
                assert inspect()['normal_run'] and inspect()['allow_capture']
                cases.append({'paused': paused, 'query_stays_query': True, 'first_use_stays_disabled': True,
                              'GUI_restores_run_and_capture_permission': True, 'normal_handoff_promoted': True,
                              'quit_then_reopen': True, 'sources_and_pause_preserved': True,
                              'fixture_Core_process_exit_waited': True, 'audio_recorded': False})
            finally:
                stop()
    report = {'passed': True, 'core_build': json.loads(subprocess.check_output([str(core), '--build-info'], text=True, encoding='utf8')), 'cases': cases}
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + '\n', encoding='utf8')
    print('PASS GUI opening restores intended recording permission; query, first use, pause and source boundaries preserved')


if __name__ == '__main__':
    main()
