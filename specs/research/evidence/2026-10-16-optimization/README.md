# P0/P1 implementation evidence

All write tests use temporary isolated data. No personal recordings, transcripts or credentials are included. Current development status and outstanding gates: [devplan](../../../prd/devplan_261016_reliability-performance.md).

## Batch A

- `history-1m.txt`: `cargo test -p hear-core --release history_scale_benchmark -- --ignored --nocapture`; product bundled SQLite 3.53.2, actual Store schema and production pending-claim path. 1,000,000 synthetic rows on this M5 / 24 GiB Mac. The scalar query distributions use 21 repetitions, warm cache, median and nearest-rank P95; the actual claim timings are single observations. No statistical end-to-end app latency claim. The legacy capacity query is measured against the same populated database for comparison. Temporary DB is removed by the test.
- Larger scale gate is now covered by Batch D: 12,614,400 blocks (365 dual-source days), with actual production read snapshots and SQL plans.
- Core suite: 128 passed, one explicitly ignored scale benchmark run separately. `cargo check -p hear-core -p hear-cli`, debug build, `python3 scripts/smoke_core.py` all passed.
- `batch-a-review.md`: fixed review contract, one confirmed repair, verification and independent review decisions.

Native device behavior, long recording coverage, pressure/memory plateaus, speech holdout quality and whole UI integration remain separate mandatory gates. These database/media checks do not establish them.

Batch C：`batch-c-review.md` 记录修复与最终三视角门禁；`batch-c-quality.json` 为公开固定输入、声纹复用、模型footprint、双源恢复/旧升级及最终签名原生短链聚合。170项core回归通过，不替代最终长时/硬件/真实质量验收。

## Batch D

- `batch-d-review.md`: fixed contract, two independently confirmed failure repairs, actual-year query failure/root fix, and final lens decisions.
- `batch-d-scale.json`: bundled SQLite 3.53.2, 30/90/365 dual-source days. Warm latest-day/all-history queries use 5 repeats and report median/max; 4000 newest precise rows in one group exercise revision work. The earlier 365-day timeout was repaired without relaxing the 3-second read budget. Synthetic metadata does not prove native capture or cold-disk latency.
- `batch-d-native.json`: final signed core SCK/Tap, simultaneous microphone/system, 12 seconds each. Fixed dropped/callback/ring/durable/seal counters included. No long-run claim.
- `batch-d-gui.json`: real signed WKWebView with public 20-second audio: continuation, seek, pause/resume, source switch, close; repaired prefetch error waits for current valid chunk boundary.
- `batch-d-fixture-coverage.json`: explicit 20-second fixture manifest, retained media, per-file decode frame integrity. Empty system source is intentional; no speech-recall claim.
- 191 workspace Rust tests (178 core + 9 desktop + 4 protocol); four isolated CLI smoke groups, 21 frontend tests, 3 sampler/coverage tests; Rust/TS/lint, signed release and deep/strict signature checks passed. Global long-run/minimum-system/low-memory/real holdout release gates remain separate and unresolved.

## Final integration

- `final-review.md`: complete baseline-to-candidate contract and adjudicated cross-batch fixes; review status is recorded separately from release acceptance.
- `final-integration.json`: 199 Rust /24 frontend /4 native-resource-and-sampler /4 CLI groups; public eight-case exact comparison, dual-source restart and delayed-import onset regression, final signed SCK/Tap short capture and writer-lock/core-crash recovery. Contains source/binary hashes, build limits, and unresolved mandatory gates.
- `final-history-1m.txt`: repaired candidate production claim path on bundled SQLite; actual empty/tail claims 0.105/0.187ms (single observations), capacity query median0.00125ms vs legacy full scan31.884ms (21 warm repeats). Larger unchanged read paths retain Batch D 30/90/365-day evidence.
- `scripts/verify_ingestion_quality.py` can repeat the actual-model onset case with explicit `--core --cli --models --wav --output`; it writes only isolated temporary data and does not start capture or cloud work. The public zh fixture has voiced material at1.0–1.5s; arbitrary silent WAVs are not equivalent fixtures.

- `preliminary-ambient.json`: superseded candidate ambient observation requested2h, explicitly stopped at~3614s after F06 native child-footprint undercount was found. Not a passed2h run; neither budget nor quality/final-candidate evidence. Independent sampled resource peak, sample continuity and retained-file decode counts preserved without audio/transcript content.
