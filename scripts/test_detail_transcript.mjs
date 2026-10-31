import assert from 'node:assert/strict';
import { test } from 'node:test';
import { bestSegments, transcriptPageEnd } from '../src/detail-transcript.ts';

const coarse = (start, end, source = 'microphone') => ({ id: `c-${source}-${start}`, source, started_ms: start, ended_ms: end, text: 'coarse' });
const precise = (start, end, source = 'microphone') => ({ ...coarse(start, end, source), id: `p-${source}-${start}`, revision: 0, text: 'precise' });
const detail = (c, p, processing_state = 'partial_transcript') => ({ coarse_segments: c, precise_segments: p, processing_state });
const done = { coarse: false, precise: false };

test('the 100th precise sentence is a page boundary, not a missing transcript', () => {
  const c = Array.from({ length: 100 }, (_, i) => coarse(i * 20, (i + 1) * 20));
  const p = Array.from({ length: 465 }, (_, i) => precise(i * 5, (i + 1) * 5));
  const first = bestSegments(detail(c, p.slice(0, 100)), { coarse: true, precise: true });
  assert(first.length > 0);
  assert(first.every(row => row.text === 'precise'));
  assert(!first.some(row => row.started_ms >= 495));
  const full = bestSegments(detail(c, p), done);
  assert.deepEqual(full, p, 'saved precise text replaces the entire coarse range once loaded');
});

test('real gaps and the other audio source remain visible after their time is known', () => {
  const c = [coarse(0, 10), coarse(20, 30), coarse(40, 50, 'system')];
  const p = [precise(0, 10), precise(40, 50)];
  assert.deepEqual(bestSegments(detail(c, p), done).map(r => r.id), [p[0].id, c[1].id, p[1].id, c[2].id]);
  assert.deepEqual(bestSegments(detail(c, []), done), c);
  assert.deepEqual(bestSegments(detail([], p), done), p);
  assert.deepEqual(bestSegments(detail(c, p, 'transcribed'), { coarse: true, precise: false }), p);
});

test('straddling coarse sentences, equal starts and long precise ends do not prove coverage', () => {
  const c = [coarse(90, 130)];
  const p = [precise(95, 100), precise(99, 400, 'system')];
  const item = detail(c, p);
  assert.equal(transcriptPageEnd(item, { coarse: false, precise: true }), 90);
  assert.deepEqual(bestSegments(item, { coarse: false, precise: true }), []);
  assert.equal(bestSegments(item, done).length, 3, 'uncovered microphone text is retained at EOF');
  const ties = [precise(10, 20), precise(10, 30, 'system')];
  assert.deepEqual(bestSegments(detail([], ties), { coarse: false, precise: true }), []);
  assert.deepEqual(bestSegments(detail([], ties), done), ties);
});

test('a sparse precise stream cannot jump ahead of unknown coarse gaps', () => {
  const c = [coarse(0, 10), coarse(20, 30)];
  const p = [precise(50, 60)];
  assert.deepEqual(bestSegments(detail(c, p), { coarse: true, precise: false }), [c[0]]);
  assert.deepEqual(bestSegments(detail(c, p), done), [...c, ...p]);
});
