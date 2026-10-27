#!/usr/bin/env python3
"""Explicit paid replay orchestration and offline structural audit of private fixtures.

Requires jsonschema. Run uses a signed hear-core test binary; credentials remain
inside the existing Core/Keychain path. Quality judgments are deliberately human.
"""
import argparse
import concurrent.futures
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import sqlite3
import subprocess
import time


def read(path):
    return json.loads(path.read_text())


def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')


def signature(item):
    return {k: v for k, v in item.items()
            if k not in ('item_index', 'replaces', 'evidence_indices', 'evidence_refs')}


def refset(refs):
    return {json.dumps(r, sort_keys=True) for r in refs}


def expected_summary_without_edits(summary, ctx, candidate):
    old_text = (ctx or {}).get('summary')
    normalized = (' '.join(summary.split()) or None) if isinstance(summary, str) and '\0' not in summary else None
    # Old replay inputs predate sentence edits and used full-text replacement.
    if not ctx or 'summary_sentences' not in ctx:
        return normalized
    history_outside = ctx['start_ms'] < candidate['start_ms'] or ctx['end_ms'] > candidate['end_ms']
    return old_text if history_outside else normalized or old_text


def verify_source(source, case_path):
    manifest = read(case_path / 'manifest.json')
    frozen = case_path / 'coarse-input.json'
    assert hashlib.sha256(frozen.read_bytes()).hexdigest() == manifest['source_sha256']
    with sqlite3.connect(f'file:{source}?mode=ro', uri=True) as db:
        db.row_factory = sqlite3.Row
        rows = db.execute('SELECT * FROM coarse_segments WHERE started_ms<? AND ended_ms>? ORDER BY started_ms,ended_ms,id',
                          (manifest['to_ms'], manifest['from_ms'])).fetchall()
    assert [dict(row) for row in rows] == read(frozen), 'Source DB differs from frozen fixture'


def run(args):
    if not args.allow_paid:
        raise SystemExit('Explicit --allow-paid required; no automatic retries')
    suite = read(args.root / 'private/suite.json')
    selected = [c for c in suite if args.split == 'all' or c['split'] == args.split]
    if args.cases:
        selected = [c for c in selected if c['case_id'] in args.cases.split(',')]
    assert selected, 'No matching cases'
    target = args.root / 'private/runs' / args.round
    target.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(args.prompt, target / 'prompt.txt')
    shutil.copyfile(args.root / 'output.schema.json', target / 'output.schema.json')
    write(target / 'round.json', {'cases': [c['case_id'] for c in selected],
          'prompt_sha256': hashlib.sha256((target / 'prompt.txt').read_bytes()).hexdigest(),
          'binary_sha256': hashlib.sha256(args.binary.read_bytes()).hexdigest(),
          'automatic_retries': 0, 'workers': args.workers,
          'reasoning_effort': args.reasoning_effort,
          'deadline_seconds': args.deadline_seconds})
    source = Path((args.root / 'private/source-path.txt').read_text().strip())
    for case in selected:
        verify_source(source, args.root / 'private/cases' / case['case_id'])
    def one(case):
        cid = case['case_id']; out = target / cid; out.mkdir()
        original = args.root / 'private/cases' / cid
        for name in ('manifest.json', 'coarse-input.json'):
            shutil.copyfile(original / name, out / name)
        os.link(source, out / 'source.sqlite')
        env = dict(os.environ, HEAR_REAL_UNDERSTANDING_ALLOW='1',
                   HEAR_REAL_UNDERSTANDING_OUTPUT=str(out.resolve()),
                   HEAR_REAL_UNDERSTANDING_PROMPT=str((target / 'prompt.txt').resolve()),
                   HEAR_REAL_UNDERSTANDING_EFFORT=args.reasoning_effort,
                   HEAR_REAL_UNDERSTANDING_DEADLINE_SECS=str(args.deadline_seconds))
        env.pop('HEAR_REAL_UNDERSTANDING_RESUME', None)
        start = time.monotonic()
        with (out / 'run.log').open('w') as log:
            result = subprocess.run([str(args.binary.resolve()), '--exact',
                'events::understanding_replay_tests::real_understanding_replay',
                '--ignored', '--nocapture', '--test-threads=1'],
                env=env, stdout=log, stderr=subprocess.STDOUT, check=False)
        status = {'case_id': cid, 'exit_code': result.returncode,
                  'seconds': round(time.monotonic() - start, 3)}
        write(out / 'run-status.json', status)
        print(json.dumps(status), flush=True)
        return status
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        results = list(pool.map(one, selected))
    write(target / 'run-status.json', results)


def audit_case(path, schema):
    from jsonschema import Draft202012Validator
    validator = Draft202012Validator(schema)
    assert (path / 'final-saved.json').exists(), 'Replay incomplete'
    manifest = read(path / 'manifest.json')
    settings = read(path / 'request-settings.json')
    round_settings = read(path.parent / 'round.json')
    assert settings['model'] == manifest['model']
    assert settings['system_prompt'] == (path.parent / 'prompt.txt').read_text()
    # The earliest low-effort rounds predate optional profiles and used 60s.
    assert settings['deadline_seconds'] == round_settings.get('deadline_seconds', 60)
    assert settings['reasoning_effort'] == round_settings.get('reasoning_effort', 'low'), 'Replay reasoning profile differs from requested round'
    assert hashlib.sha256((path / 'coarse-input.json').read_bytes()).hexdigest() == manifest['source_sha256']
    source = {s['id']: s for s in read(path / 'coarse-input.json')}
    plan = read(path / 'window-plan.json')
    db = sqlite3.connect(f'file:{path / "replay.sqlite"}?mode=ro', uri=True)
    prior = {}; details = []
    def unique_pairs(pairs):
        obj = {}
        for key, value in pairs:
            assert key not in obj, f'Duplicate JSON key: {key}'
            obj[key] = value
        return obj
    raw_outputs = []
    for raw in path.glob('provider-response-*.json'):
        body = read(raw)
        content = body['choices'][0]['message']['content']
        raw_outputs.append(json.loads(content, object_pairs_hook=unique_pairs))
    assert len(raw_outputs) == len(plan), 'One raw response required per window'
    for n, window in enumerate(plan, 1):
        inp = read(path / f'window-{n:02}-input.json')
        model = read(path / f'window-{n:02}-model.json')
        assert model in raw_outputs, 'Model export differs from raw response'
        raw_outputs.remove(model)
        validator.validate(model)
        saved = read(path / f'window-{n:02}-saved.json')['events']
        byid = {e['id']: e for e in saved}
        catalog = inp['evidence']
        seen_segments = set()
        for observation in inp['observations']:
            index = observation.get('evidence_index')
            if index is None:
                assert observation['text'] is None, 'Text observation lacks source reference'
                continue
            ref = catalog[index]
            original = source[ref['segment_id']]
            seen_segments.add(original['id'])
            assert observation['text'] == original['text'], 'Observation text differs from frozen source'
            assert observation['source'] == ref['source'] == original['source']
            assert (ref['start_ms'], ref['end_ms']) == (original['started_ms'], original['ended_ms'])
            assert (observation['start_ms'], observation['end_ms']) == (
                max(original['started_ms'], window['start_ms']), min(original['ended_ms'], window['end_ms']))
        expected_segments = {s['id'] for s in source.values() if s['text'].strip()
                             and s['started_ms'] < window['end_ms'] and s['ended_ms'] > window['start_ms']}
        assert seen_segments == expected_segments, 'Frozen source observation omitted or added'
        current = {o['evidence_index'] for o in inp['observations']
                   if o.get('evidence_index') is not None and o.get('text')}
        context = inp['context']['previous_events'] + inp['context']['linked_events']
        for ctx in context:
            if ctx['id'] not in prior:
                continue
            old = prior[ctx['id']]
            assert ctx['summary'] == old['summary'], 'Previous summary not transmitted'
            if ctx['understanding'] is None:
                assert old['understanding'] is None
                continue
            assert ctx['understanding']['topics'] == old['understanding']['topics']
            for kind in ('key_points', 'mentioned_objects'):
                assert len(ctx['understanding'][kind]) == len(old['understanding'][kind])
                for i, (a, b) in enumerate(zip(ctx['understanding'][kind], old['understanding'][kind])):
                    assert a['item_index'] == i and signature(a) == signature(b)
                    assert refset([catalog[j] for j in a['evidence_indices']]) == refset(b['evidence_refs'])
        used_ids = set(); spans = []; replacements = 0
        for slot, event in enumerate(model['events']):
            assert inp['candidate']['start_ms'] <= event['start_ms'] < event['end_ms'] <= inp['candidate']['end_ms']
            mask = {'microphone': 1, 'system': 2, 'both': 3}[event['sources']]
            for start, end, other_mask in spans:
                assert not (mask & other_mask and start < event['end_ms'] and end > event['start_ms']), 'Overlapping same-source events'
            spans.append((event['start_ms'], event['end_ms'], mask))
            merge = event['merge_with_event_id']
            if merge:
                assert merge not in used_ids
                used_ids.add(merge)
                old_ctx = next(c for c in inp['context']['previous_events'] if c['id'] == merge)
                assert not old_ctx['boundary_locked']
            link = db.execute('SELECT event_id FROM event_candidate_links WHERE candidate_id=? AND slot=?', (window['id'], slot)).fetchone()
            assert link and link[0] in byid, 'Model event rejected by publication'
            saved_event = byid[link[0]]
            ctx = next((c for c in context if c['id'] == (merge or link[0])), None)
            if merge:
                assert link[0] == merge, 'Requested continuation was not published to target'
            expected_mask = mask | (prior[merge]['source_mask'] if merge else 0)
            assert saved_event['source_mask'] == expected_mask, 'Persisted event source union differs'
            assert saved_event['title'] == event['title'], 'Model title rejected or changed by publication'
            old_u = (ctx or {}).get('understanding')
            allowed = set(current)
            inherited = set()
            if old_u:
                inherited.update(j for k in ('key_points', 'mentioned_objects') for v in old_u[k] for j in v['evidence_indices'])
                allowed.update(inherited)
            summary = event['summary']
            if isinstance(summary, list):
                old_text = (ctx or {}).get('summary')
                old_sentences = [s.strip() for s in re.split(r'(?<=[。！？])', old_text or '') if s.strip()]
                if ctx:
                    assert ctx['summary_sentences'] == [dict(item_index=i,text=s) for i,s in enumerate(old_sentences)]
                    if 'summary_chars' in ctx:
                        assert ctx['summary_chars'] == len(old_text or '')
                used = set(); revised = {}; added = []
                for update in summary:
                    indices = update['replaces']; refs = update['evidence_indices']
                    assert len(set(indices)) == len(indices) and not used.intersection(indices)
                    assert all(0 <= i < len(old_sentences) for i in indices)
                    assert refs and all(i in allowed and i < len(catalog) for i in refs)
                    assert all(expected_mask & {'microphone':1,'system':2}[catalog[i]['source']] for i in refs)
                    assert all(catalog[i]['start_ms'] < saved_event['end_ms'] and catalog[i]['end_ms'] > saved_event['start_ms'] for i in refs)
                    text = ' '.join(update['text'].split()); assert text
                    mode = update.get('mode', 'update')
                    assert mode in ('update', 'compress')
                    if mode == 'compress':
                        assert indices and all(i in inherited for i in refs)
                        rendered = ' '.join(s.strip() for s in re.split(r'(?<=[。！？])', text) if s.strip())
                        assert len(rendered) < sum(len(old_sentences[i]) for i in indices) + len(indices) - 1
                    if indices:
                        assert mode == 'compress' or current.intersection(refs)
                        revised[min(indices)] = text; used.update(indices)
                    else:
                        added.append(text)
                pieces = []
                for i, text in enumerate(old_sentences):
                    if i in revised:
                        pieces.extend(s.strip() for s in re.split(r'(?<=[。！？])', revised[i]) if s.strip())
                    elif i not in used:
                        pieces.append(text)
                for text in added:
                    for sentence in re.split(r'(?<=[。！？])', text):
                        sentence = sentence.strip()
                        if sentence and sentence not in pieces: pieces.append(sentence)
                expected_summary = (' '.join(pieces) or None) if used or added else old_text
                assert saved_event['summary'] == expected_summary, 'Summary sentence merge differs from saved text'
            else:
                assert saved_event['summary'] == expected_summary_without_edits(summary, ctx, inp['candidate'])
            patch = event['understanding']; stored = saved_event['understanding']
            if patch is None:
                assert not old_u, 'Unexpected missing patch despite valid history'
                continue
            assert stored is not None, 'Understanding discarded during publication'
            for kind in ('key_points', 'mentioned_objects'):
                old_items = old_u[kind] if old_u else []; consumed = set()
                for item in patch[kind]:
                    replace = item['replaces']
                    assert not consumed.intersection(replace)
                    assert all(i < len(old_items) for i in replace)
                    consumed.update(replace)
                    refs = item['evidence_indices']
                    assert all(i in allowed and i < len(catalog) for i in refs), 'Invalid/foreign evidence index'
                    assert all(expected_mask & {'microphone': 1, 'system': 2}[catalog[i]['source']] for i in refs), 'Evidence source not in event sources'
                    if replace:
                        assert current.intersection(refs), 'Replacement without fresh evidence'
                    match = next((x for x in stored[kind] if signature(x) == signature(item)), None)
                    assert match, 'Model item rejected by normalizer'
                    assert refset([catalog[i] for i in refs]).issubset(refset(match['evidence_refs']))
                replacements += len(consumed)
                for i, old in enumerate(old_items):
                    if i in consumed:
                        continue
                    match = next((x for x in stored[kind] if signature(x) == signature(old)), None)
                    assert match, 'Omitted prior item lost'
                    assert refset([catalog[j] for j in old['evidence_indices']]).issubset(refset(match['evidence_refs']))
            assert set((old_u or {}).get('topics', []) + patch['topics']).issubset(stored['topics'])
        for event in saved:
            if not event['understanding']:
                continue
            for kind in ('key_points', 'mentioned_objects'):
                for item in event['understanding'][kind]:
                    assert not set(item).intersection(('item_index', 'replaces', 'evidence_indices'))
                    for ref in item['evidence_refs']:
                        original = source[ref['segment_id']]
                        assert (ref['source'], ref['start_ms'], ref['end_ms']) == (original['source'], original['started_ms'], original['ended_ms'])
                        assert ref['start_ms'] < event['end_ms'] and ref['end_ms'] > event['start_ms']
                        assert event['source_mask'] & {'microphone': 1, 'system': 2}[ref['source']]
        details.append({'window': n, 'events': len(saved), 'replaced_items': replacements,
                        'points': sum(len(e['understanding']['key_points']) for e in saved if e['understanding'])})
        prior = byid
    final = read(path / 'final-saved.json')['events']
    actual = []
    for row in db.execute('SELECT id,started_ms,ended_ms,title,source_mask,summary,understanding_json,revision FROM named_events WHERE deleted_ms IS NULL ORDER BY started_ms,id'):
        actual.append(dict(zip(('id', 'start_ms', 'end_ms', 'title', 'source_mask', 'summary', 'understanding', 'revision'),
                               (*row[:6], json.loads(row[6]) if row[6] else None, row[7]))))
    assert final == actual, 'Final export differs from complete active SQL snapshot'
    attempts = [read(p) for p in path.glob('attempt-*.json')]
    budget = settings['deadline_seconds']
    assert len(attempts) == len(plan) and all(a['success'] and a['elapsed_seconds'] < budget for a in attempts)
    db.close()
    return {'case_id': manifest['case_id'], 'structural_pass': True, 'windows': details,
            'final_events': len(final), 'max_call_seconds': max(a['elapsed_seconds'] for a in attempts),
            'evaluation_deadline_seconds': budget,
            'reasoning_effort': settings['reasoning_effort'],
            'production_60s_budget_pass': all(a['elapsed_seconds'] < 60 for a in attempts),
            'quality_status': 'pending human review'}


def audit(args):
    target = args.root / 'private/runs' / args.round
    schema = read(target / 'output.schema.json')
    results = []; review = [f'# {args.round}：质量核对材料', '结构通过不等于质量通过。原文、必保事实和禁错项逐例核对。']
    for cid in read(target / 'round.json')['cases']:
        path = target / cid
        try:
            result = audit_case(path, schema)
        except Exception as exc:
            result = {'case_id': cid, 'structural_pass': False, 'error': f'{type(exc).__name__}: {exc}'}
        results.append(result)
        write(path / 'structure-audit.json', result)
        m = read(path / 'manifest.json')
        review += [f'\n## {cid} {m["label"]} — {m["minutes"]} 分钟',
                   f'结构：{result["structural_pass"]}',
                   '必保事实：' + '；'.join(m['required_facts']),
                   '禁错：' + '；'.join(m['forbidden_errors'])]
        if (path / 'final-saved.json').exists():
            for event in read(path / 'final-saved.json')['events']:
                review += [f'\n### {event["title"]}', event['summary'] or 'summary=null']
                if event['understanding']:
                    review += ['\n要点：'] + [f'{i+1}. {x["text"]}' for i, x in enumerate(event['understanding']['key_points'])]
                    review += ['\n对象：'] + [f'- {x["name"]}：{x["context"]}' for x in event['understanding']['mentioned_objects']]
    write(target / 'structure-audit.json', results)
    (target / 'quality-review.md').write_text('\n\n'.join(review) + '\n')
    print(json.dumps(results, ensure_ascii=False, indent=2))


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['run', 'audit'])
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--round', required=True)
    parser.add_argument('--prompt', type=Path)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--allow-paid', action='store_true')
    parser.add_argument('--split', choices=['all', 'dev', 'holdout'], default='all')
    parser.add_argument('--cases')
    parser.add_argument('--workers', type=int, choices=[1, 2], default=2)
    parser.add_argument('--reasoning-effort', choices=['low', 'high'], default='low',
                        help='high is a separately reported test-only comparison; production remains low')
    parser.add_argument('--deadline-seconds', type=int, choices=[60, 120], default=60,
                        help='120 is opt-in quality evaluation only, not production compatibility')
    args = parser.parse_args()
    if Path(args.round).name != args.round or args.round in ('.', '..'):
        parser.error('round must be one directory name')
    if args.action == 'run' and (args.prompt is None or args.binary is None):
        parser.error('run requires --prompt and --binary')
    (run if args.action == 'run' else audit)(args)


if __name__ == '__main__':
    main()
