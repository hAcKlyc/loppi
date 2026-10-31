"""Isolated synthetic audit; never opens the user's database or starts hear.

Run from any directory with Python 3; prints JSON. All databases are temporary.
Uses the audited Rust SQL, but Python's SQLite runtime, not hear's bundled one.
"""
import json
import pathlib
import platform
import re
import sqlite3
import statistics
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[4]
AUDITED_SOURCE = "2c81ebd49601c85d89aa3529555aeddaa98d63f5"
SOURCE = subprocess.check_output(
    ["git", "show", f"{AUDITED_SOURCE}:crates/hear-core/src/main.rs"],
    cwd=ROOT, text=True,
)
PENDING = re.search(
    r'"(SELECT b.id, b.session_id, b.source, b.sample_start, b.frames, b.media_path,.*?)",\s*params!\[idle_before_ms\]',
    SOURCE, re.S,
).group(1)
SCHEMA = "\n".join(
    re.search(r"(CREATE TABLE IF NOT EXISTS " + table + r" \(.*?\n             \);)", SOURCE, re.S).group(1)
    for table in ("audio_blocks", "recording_sessions")
)
SCHEMA += """
CREATE INDEX audio_blocks_source_time ON audio_blocks(source,started_ms);
CREATE INDEX audio_blocks_started ON audio_blocks(started_ms);
CREATE INDEX audio_blocks_ended ON audio_blocks(ended_ms);
"""


def measure(db, sql, params=(), repeats=9):
    db.execute(sql, params).fetchall()  # warm connection/cache; not a cold-start test
    elapsed = []
    for _ in range(repeats):
        start = time.perf_counter()
        db.execute(sql, params).fetchall()
        elapsed.append((time.perf_counter() - start) * 1000)
    return {"median_ms": round(statistics.median(elapsed), 4),
            "min_ms": round(min(elapsed), 4), "max_ms": round(max(elapsed), 4),
            "repeats": repeats,
            "plan": [row[3] for row in db.execute("EXPLAIN QUERY PLAN " + sql, params)]}


def row(i):
    source = "microphone" if i % 2 == 0 else "system"
    ordinal = i // 2
    return (f"block-{i:09d}", f"{source}-{ordinal // 17280}", source,
            ordinal * 5000, ordinal * 5000 + 5000, ordinal % 17280 * 240000,
            240000, 30000, f"media/{source}/block-{i:09d}.opus",
            "retain_speech_v2", "complete")


results = {"python": platform.python_version(), "sqlite": sqlite3.sqlite_version,
           "machine": platform.machine(), "os": platform.platform(),
           "method": "synthetic disk DB; WAL; warm repeated reads; no media files; all retained; original queries; no ANALYZE; timings are not app latency",
           "sizes": []}
for size in (34560, 100000, 1000000):
    with tempfile.TemporaryDirectory(prefix="hear-sql-audit-") as tmp:
        db = sqlite3.connect(pathlib.Path(tmp) / "synthetic.sqlite")
        db.execute("PRAGMA journal_mode=WAL")
        db.executescript(SCHEMA)
        db.executemany("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state) VALUES (?,?,?,?,?,?,?,?,?,?,?)", (row(i) for i in range(size)))
        db.execute("INSERT INTO recording_sessions SELECT session_id,source,min(started_ms),max(ended_ms),max(ended_ms),'closed' FROM audio_blocks GROUP BY session_id")
        db.commit()
        db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        queries = {
            "usage_sum": ("SELECT coalesce(sum(bytes),0) FROM audio_blocks", ()),
            "orphan_path_absent": ("SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path=?)", ("media/system/absent.opus",)),
            "orphan_path_last": ("SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path=?)", (row(size-1)[8],)),
            "session_replay": ("SELECT id,session_id,source,sample_start,frames,media_path,coalesce(speech_active,0) FROM audio_blocks WHERE session_id=? AND sample_start<? AND analysis_state='complete' AND media_path IS NOT NULL ORDER BY sample_start DESC LIMIT 6", (row(size-1)[1], row(size-1)[5])),
            "pending_empty": (PENDING, (size * 2500 + 10000,)),
        }
        before = {key: measure(db, *query) for key, query in queries.items()}
        db.execute("UPDATE audio_blocks SET analysis_state='pending' WHERE id=?", (row(size-1)[0],))
        db.commit()
        before["pending_one_at_end"] = measure(db, PENDING, (size * 2500 + 10000,))
        db.executescript("""
            CREATE INDEX audit_media_path ON audio_blocks(media_path) WHERE media_path IS NOT NULL;
            CREATE INDEX audit_session_samples ON audio_blocks(session_id,sample_start);
            CREATE INDEX audit_pending ON audio_blocks(started_ms,id)
              WHERE analysis_state='pending' AND analysis_attempts<3 AND media_path IS NOT NULL;
            CREATE TABLE audit_usage(id INTEGER PRIMARY KEY, bytes INTEGER NOT NULL);
            INSERT INTO audit_usage SELECT 1,sum(bytes) FROM audio_blocks;
        """)
        after = {key: measure(db, *query) for key, query in queries.items() if key not in ("usage_sum", "pending_empty")}
        after["usage_counter_read"] = measure(db, "SELECT bytes FROM audit_usage WHERE id=1")
        after["pending_one_at_end"] = measure(db, PENDING, (size * 2500 + 10000,))
        db.execute("UPDATE audio_blocks SET analysis_state='complete' WHERE id=?", (row(size-1)[0],))
        db.commit()
        after["pending_empty"] = measure(db, PENDING, (size * 2500 + 10000,))
        results["sizes"].append({"rows": size, "before": before, "after_prototype": after})
        db.close()
print(json.dumps(results, ensure_ascii=False, indent=2))
