#!/usr/bin/env python3
"""Frozen paired transcript fixtures and three-repeat paid Core replays.

The precise arm adapts existing utterances to the isolated coarse table shape;
it does not change production's input owner. No credentials leave signed Core.
"""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import sqlite3
import subprocess
import time
import uuid

from evaluate_event_prompt import read, write, verify_source, audit_case


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(args):
    root, old = args.root, args.original
    private = root / 'private'
    fixtures = private / 'fixtures'
    fixtures.mkdir(exist_ok=False)
    source = Path((old / 'private/source-path.txt').read_text().strip())
    suite = read(old / 'private/suite.json')
    src = sqlite3.connect(f'file:{source}?mode=ro', uri=True)
    src.row_factory = sqlite3.Row
    for case in suite:
        verify_source(source, old / 'private/cases' / case['case_id'])
    for arm in ('coarse', 'precise'):
        dest = sqlite3.connect(fixtures / f'{arm}.sqlite')
        src.backup(dest)
        if arm == 'precise':
            dest.execute('DELETE FROM coarse_segments')
        dest.commit()
        dest.close()
    dest = sqlite3.connect(fixtures / 'precise.sqlite')
    columns = [r[1] for r in src.execute('PRAGMA table_info(coarse_segments)')]
    assert columns == ['id', 'block_id', 'session_id', 'source', 'started_ms', 'ended_ms', 'text', 'model_version']
    cases = []
    for case in suite:
        cid, start, end = case['case_id'], case['from_ms'], case['to_ms']
        query = """SELECT DISTINCT p.* FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id
                   WHERE t.state='complete' AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
                   AND p.started_ms<? AND p.ended_ms>?"""
        params = [end, start]
        if case['source_event_id']:
            query += ' AND t.target_id=?'
            params.append(case['source_event_id'])
        precise = [dict(r) for r in src.execute(query + ' ORDER BY p.started_ms,p.ended_ms,p.id', params)]
        if cid == 'c07':
            repair = private / 'asr-repair'
            request = (repair / 'request-id.txt').read_text().strip()
            for n, item in enumerate(read(repair / 'mapped-utterances.json')):
                precise.append(dict(id=str(uuid.uuid5(uuid.UUID(request), str(n))), task_id=request,
                                    source='microphone', started_ms=item['start_ms'], ended_ms=item['end_ms'],
                                    text=item['text'], speaker_label=item['speaker'], speaker_scope=request,
                                    origin='isolated ASR repair'))
        precise.sort(key=lambda r: (r['started_ms'], r['ended_ms'], r['id']))
        assert precise and len({r['id'] for r in precise}) == len(precise)
        adapted = []
        for row in precise:
            assert row['started_ms'] < end and row['ended_ms'] > start
            block = src.execute("""SELECT id,session_id FROM audio_blocks WHERE source=?
                AND started_ms<? AND ended_ms>? ORDER BY started_ms LIMIT 1""",
                                (row['source'], row['ended_ms'], row['started_ms'])).fetchone()
            assert block, f'Precise utterance has no original audio block: {row["id"]}'
            adapted.append(dict(id=row['id'], block_id=block['id'], session_id=block['session_id'],
                                source=row['source'], started_ms=row['started_ms'], ended_ms=row['ended_ms'],
                                text=row['text'], model_version='fixture:precise'))
        dest.executemany('INSERT INTO coarse_segments VALUES (?,?,?,?,?,?,?,?)',
                         [[r[k] for k in columns] for r in adapted])
        windows = read(old / 'private/runs/v15-all-a' / cid / 'window-plan.json')
        for arm in ('coarse', 'precise'):
            path = private / 'cases' / arm / cid
            path.mkdir(parents=True, exist_ok=False)
            rows = read(old / 'private/cases' / cid / 'coarse-input.json') if arm == 'coarse' else adapted
            write(path / 'coarse-input.json', rows)
            manifest = dict(case, arm=arm, source_sha256=digest(path / 'coarse-input.json'), frozen_windows=windows)
            write(path / 'manifest.json', manifest)
            (path / 'transcript.txt').write_text('\n'.join(f'[{r["started_ms"]}..{r["ended_ms"]}] {r["source"]}: {r["text"]}' for r in rows) + '\n')
            if arm == 'precise':
                write(path / 'origin-utterances.json', precise)
            cases.append(manifest)
    dest.commit()
    dest.close()
    src.close()
    for case in cases:
        verify_source(fixtures / f'{case["arm"]}.sqlite', private / 'cases' / case['arm'] / case['case_id'])
    write(private / 'suite.json', cases)
    write(private / 'fixture-settings.json', dict(prompt_sha256=digest(root / 'prompt.txt'),
          schema_sha256=digest(root / 'output.schema.json'), model='deepseek-flash', reasoning_effort='high',
          deadline_seconds=120, repeats=3, windows_per_arm=sum(len(c['frozen_windows']) for c in cases if c['arm']=='coarse')))


def run(args):
    assert args.allow_paid, 'Explicit --allow-paid required'
    root = args.root
    private = root / 'private'
    cfg = read(private / 'fixture-settings.json')
    assert digest(root / 'prompt.txt') == cfg['prompt_sha256']
    assert digest(root / 'output.schema.json') == cfg['schema_sha256']
    assert digest(private / 'gold.json') == cfg['gold_sha256']
    suite = read(private / 'suite.json')
    arms = list(dict.fromkeys(case['arm'] for case in suite))
    assert arms and set(arms) <= {'coarse', 'precise'}
    target = private / 'runs'
    target.mkdir(exist_ok=False)
    for case in suite:
        verify_source(private / 'fixtures' / f'{case["arm"]}.sqlite', private / 'cases' / case['arm'] / case['case_id'])
    for repeat in range(1, 4):
        for arm in arms:
            path = target / f'{arm}-r{repeat}'
            path.mkdir()
            shutil.copyfile(root / 'prompt.txt', path / 'prompt.txt')
            shutil.copyfile(root / 'output.schema.json', path / 'output.schema.json')
            write(path / 'round.json', dict(cfg, arm=arm, repeat=repeat, workers=2,
                cases=[c['case_id'] for c in suite if c['arm']==arm], automatic_retries=0,
                binary_sha256=digest(args.binary)))
    rng = random.Random(20261024)
    schedule = []
    for repeat in range(1, 4):
        ids = [c['case_id'] for c in suite if c['arm']=='coarse']
        rng.shuffle(ids)
        for cid in ids:
            shuffled_arms = arms.copy()
            rng.shuffle(shuffled_arms)
            schedule.extend(dict(case_id=cid, arm=arm, repeat=repeat) for arm in shuffled_arms)
    write(private / 'execution-order.json', schedule)

    def one(job):
        cid, arm, repeat = job['case_id'], job['arm'], job['repeat']
        source = private / 'fixtures' / f'{arm}.sqlite'
        original = private / 'cases' / arm / cid
        out = target / f'{arm}-r{repeat}' / cid
        out.mkdir()
        for name in ('manifest.json', 'coarse-input.json'):
            shutil.copyfile(original / name, out / name)
        os.link(source, out / 'source.sqlite')
        env = dict(os.environ, LOPPI_REAL_UNDERSTANDING_ALLOW='1', LOPPI_REAL_UNDERSTANDING_OUTPUT=str(out.resolve()),
                   LOPPI_REAL_UNDERSTANDING_PROMPT=str((root / 'prompt.txt').resolve()),
                   LOPPI_REAL_UNDERSTANDING_EFFORT='high', LOPPI_REAL_UNDERSTANDING_DEADLINE_SECS='120')
        env.pop('LOPPI_REAL_UNDERSTANDING_RESUME', None)
        started = time.monotonic()
        with (out / 'run.log').open('w') as log:
            result = subprocess.run([str(args.binary.resolve()), '--exact',
                'events::understanding_replay_tests::real_understanding_replay', '--ignored', '--nocapture', '--test-threads=1'],
                env=env, stdout=log, stderr=subprocess.STDOUT, check=False)
        status = dict(job, exit_code=result.returncode, seconds=round(time.monotonic()-started, 3))
        write(out / 'run-status.json', status)
        print(json.dumps(status), flush=True)
        return status
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(one, schedule))
    write(private / 'run-status.json', results)


def audit(args):
    private = args.root / 'private'
    results = []
    for job in read(private / 'execution-order.json'):
        path = private / 'runs' / f'{job["arm"]}-r{job["repeat"]}' / job['case_id']
        status = read(path / 'run-status.json') if (path / 'run-status.json').exists() else None
        if status is None:
            results.append(dict(job, completed=False, structural_pass=False, error='Missing run status'))
            continue
        try:
            assert status['exit_code'] == 0, f"Replay exited {status['exit_code']}"
            assert (path / 'final-saved.json').exists(), 'Missing final saved result'
            assert read(path / 'window-plan.json') == read(path / 'manifest.json')['frozen_windows']
            result = dict(job, **{k:v for k,v in audit_case(path, read(args.root / 'output.schema.json')).items() if k!='case_id'})
        except Exception as exc:
            result = dict(job, structural_pass=False, error=f'{type(exc).__name__}: {exc}')
        result['completed'] = status['exit_code'] == 0 and (path / 'final-saved.json').exists()
        write(path / 'structure-audit.json', result)
        results.append(result)
    write(private / 'structure-audit.json', results)
    print(json.dumps(dict(expected=len(results), completed=sum(r['completed'] for r in results),
                         structural_pass=sum(r['structural_pass'] for r in results))))


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['prepare','run','audit'])
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--original', type=Path)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--allow-paid', action='store_true')
    args = parser.parse_args()
    if args.action=='prepare' and args.original is None: parser.error('prepare requires --original')
    if args.action=='run' and args.binary is None: parser.error('run requires --binary')
    {'prepare':prepare,'run':run,'audit':audit}[args.action](args)
