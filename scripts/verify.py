#!/usr/bin/env python3
"""One offline gate for local work and CI. No capture, credentials or cloud calls.
Dependencies are installed explicitly with npm ci / Cargo. Ignored Rust tests
remain opt-in; see specs/tech_docs/diagnostics-and-verification.md.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--suite', choices=['all', 'static', 'test', 'smoke'], default='all')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('hear currently requires macOS for Core/native builds')
    env = os.environ.copy()
    # Explicitly remove opt-in private fixtures/provider tests from normal verification.
    for key in list(env):
        if key.startswith('HEAR_TEST_') or key in ['HEAR_DATA_DIR', 'HEAR_CORE_BIN']:
            del env[key]
    env['HEAR_TEST_PARENT_PID'] = str(os.getpid())
    env['MACOSX_DEPLOYMENT_TARGET'] = '13.0'
    env['HEAR_BUILD_EPOCH'] = str(int(time.time()))
    checks = []
    result = 0

    def run(name, command):
        print(f'\n[{name}]', flush=True)
        started = time.monotonic()
        completed = subprocess.run(command, cwd=ROOT, env=env, timeout=1800)
        checks.append({'name': name, 'command': command, 'seconds': round(time.monotonic()-started, 2), 'exit_code': completed.returncode})
        if completed.returncode:
            raise subprocess.CalledProcessError(completed.returncode, command)

    try:
        if args.suite in ['all', 'static']:
            run('typecheck', ['npm', 'run', 'typecheck'])
            run('lint', ['npm', 'run', 'lint'])
            run('rust-check', ['cargo', 'check', '--workspace', '--locked'])
        if args.suite in ['all', 'test']:
            run('rust-regressions', ['cargo', 'test', '--workspace', '--locked'])
            run('frontend-regressions', ['node', '--test', *[str(p.relative_to(ROOT)) for p in sorted((ROOT/'scripts').glob('test_*.mjs'))]])
            run('rabbit-assets', ['node', 'scripts/test_rabbit_assets.cjs'])
            run('python-regressions', [sys.executable, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_*.py'])
            with tempfile.TemporaryDirectory(prefix='hear-native-policy-') as directory:
                binary = str(Path(directory)/'entry-policy')
                run('native-policy-build', ['clang', '-fobjc-arc', '-fblocks', '-mmacosx-version-min=13.0', 'scripts/test_native_entry.m', '-framework', 'Cocoa', '-framework', 'QuartzCore', '-o', binary])
                run('native-policy', [binary])
                binary = str(Path(directory)/'capture-errors')
                run('native-capture-errors-build', ['clang', '-fobjc-arc', '-fblocks', '-mmacosx-version-min=13.0', 'scripts/test_native_capture_errors.m', '-framework', 'Foundation', '-framework', 'AppKit', '-framework', 'ScreenCaptureKit', '-framework', 'CoreMedia', '-framework', 'CoreGraphics', '-o', binary])
                run('native-capture-errors', [binary])
                binary = str(Path(directory)/'playback')
                run('native-playback-build', ['clang', '-fobjc-arc', '-fblocks', '-mmacosx-version-min=13.0', 'scripts/test_native_playback.m', '-framework', 'Foundation', '-framework', 'AVFoundation', '-o', binary])
                run('native-playback', [binary])
                binary = str(Path(directory)/'power-policy')
                run('native-power-build', ['clang', '-fobjc-arc', '-fblocks', '-mmacosx-version-min=13.0', 'scripts/test_native_power.m', '-framework', 'Foundation', '-framework', 'IOKit', '-o', binary])
                run('native-power', [binary])
        if args.suite in ['all', 'smoke']:
            run('frontend-build', ['npm', 'run', 'build'])
            run('binary-build', ['cargo', 'build', '--workspace', '--locked'])
            run('component-identities', [sys.executable, 'scripts/verify_build_identity.py', '--directory', 'target/debug'])
            run('isolated-core', [sys.executable, 'scripts/smoke_core.py'])
            run('isolated-diagnostics', [sys.executable, 'scripts/smoke_diagnostics.py'])
            run('isolated-test-owner', [sys.executable, 'scripts/smoke_test_owner.py'])
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError) as error:
        print(f'verification failed: {error}', file=sys.stderr)
        result = 1
    finally:
        report = {'schema': 1, 'suite': args.suite, 'passed': result == 0, 'checks': checks,
                  'excluded': ['private fixture tests', 'cloud API tests', 'permission/capture/sleep acceptance', 'signed installation', 'browser visual acceptance']}
        if args.report:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            args.report.write_text(json.dumps(report, indent=2)+'\n')
    return result


if __name__ == '__main__':
    raise SystemExit(main())
