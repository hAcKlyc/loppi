//! Incremental storage accounting. Triggers keep every existing write path,
//! including retention and recovery, in the same transaction as its total.
use rusqlite::Connection;

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS core_migrations(name TEXT PRIMARY KEY);
         CREATE INDEX IF NOT EXISTS audio_blocks_media_path ON audio_blocks(media_path)
           WHERE media_path IS NOT NULL;
         CREATE INDEX IF NOT EXISTS audio_blocks_session_sample ON audio_blocks(session_id,sample_start);
         CREATE INDEX IF NOT EXISTS audio_blocks_pending ON audio_blocks(started_ms,id)
           WHERE analysis_state='pending' AND analysis_attempts<3 AND media_path IS NOT NULL;
         CREATE INDEX IF NOT EXISTS audio_blocks_recovery ON audio_blocks(analysis_state)
           WHERE analysis_state='sealing' OR analysis_state='processing';
         CREATE INDEX IF NOT EXISTS recording_sessions_open ON recording_sessions(state)
           WHERE state='open';
         CREATE TABLE IF NOT EXISTS storage_usage(
           id INTEGER PRIMARY KEY CHECK(id=1),audio_bytes INTEGER NOT NULL,orphan_bytes INTEGER NOT NULL
         );",
    ).map_err(|e| e.to_string())?;
    let initialized: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_usage WHERE id=1)",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    db.execute_batch("SAVEPOINT storage_init")
        .map_err(|e| e.to_string())?;
    let result = (|| {
        if !initialized {
            db.execute_batch(
                "INSERT INTO storage_usage SELECT 1,
              (SELECT coalesce(sum(bytes),0) FROM audio_blocks),
              (SELECT coalesce(sum(bytes),0) FROM orphan_media);",
            )?;
        }
        for (table, column) in [
            ("audio_blocks", "audio_bytes"),
            ("orphan_media", "orphan_bytes"),
        ] {
            db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS {table}_usage_insert AFTER INSERT ON {table} BEGIN
                   UPDATE storage_usage SET {column}={column}+new.bytes WHERE id=1; END;
                 CREATE TRIGGER IF NOT EXISTS {table}_usage_update AFTER UPDATE OF bytes ON {table} BEGIN
                   UPDATE storage_usage SET {column}={column}+new.bytes-old.bytes WHERE id=1; END;
                 CREATE TRIGGER IF NOT EXISTS {table}_usage_delete AFTER DELETE ON {table} BEGIN
                   UPDATE storage_usage SET {column}={column}-old.bytes WHERE id=1; END;"
            ))?;
        }
        Ok::<_, rusqlite::Error>(())
    })();
    match result {
        Ok(()) => db.execute_batch("RELEASE storage_init"),
        Err(e) => {
            let _ = db.execute_batch("ROLLBACK TO storage_init; RELEASE storage_init");
            return Err(e.to_string());
        }
    }
    .map_err(|e| e.to_string())
}

/// Keep historical backfills and their marker atomic, including on SQL failure.
pub fn migrate(db: &Connection, name: &str, sql: &str) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS core_migrations(name TEXT PRIMARY KEY)")
        .map_err(|e| e.to_string())?;
    let done: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM core_migrations WHERE name=?1)",
            [name],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if done {
        return Ok(());
    }
    db.execute_batch("SAVEPOINT core_migration")
        .map_err(|e| e.to_string())?;
    let result = db.execute_batch(sql).and_then(|_| {
        db.execute("INSERT INTO core_migrations VALUES(?1)", [name])?;
        db.execute_batch("RELEASE core_migration")
    });
    if result.is_err() {
        let _ = db.execute_batch("ROLLBACK TO core_migration; RELEASE core_migration");
    }
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    // Run against the product's bundled SQLite, never the system/Python engine.
    #[test]
    #[ignore = "scale benchmark; HEAR_BENCH_ROWS defaults to 1000000"]
    fn history_scale_benchmark() {
        use std::time::Instant;
        let directory = tempfile::tempdir().unwrap();
        let mut store =
            crate::Store::new(Connection::open(directory.path().join("scale.sqlite")).unwrap())
                .unwrap();
        let rows: i64 = std::env::var("HEAR_BENCH_ROWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_000_000);
        store
            .db
            .execute_batch("INSERT INTO recording_sessions VALUES('s','microphone',0,0,1,'closed')")
            .unwrap();
        let begin = Instant::now();
        let tx = store.db.transaction().unwrap();
        {
            let mut statement=tx.prepare("INSERT INTO audio_blocks
              (id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state)
              VALUES(?1,'s','microphone',?2,?2+5000,?3,240000,100,?4,'speech','complete')").unwrap();
            for i in 0..rows {
                statement
                    .execute(params![
                        format!("b{i:010}"),
                        i * 5000,
                        i * 240000,
                        format!("media/microphone/{i}.opus")
                    ])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        let insert_ms = begin.elapsed().as_secs_f64() * 1000.;
        let queries = [
            (
                "legacy_capacity",
                "SELECT coalesce(sum(bytes),0) FROM audio_blocks",
            ),
            (
                "capacity",
                "SELECT audio_bytes+orphan_bytes FROM storage_usage WHERE id=1",
            ),
            (
                "missing_media",
                "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path='absent')",
            ),
            (
                "known_or_pending_delete",
                "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path='absent') OR EXISTS(SELECT 1 FROM media_deletion_jobs WHERE media_path='absent' AND completed_ms IS NULL)",
            ),
            (
                "empty_pending",
                "SELECT coalesce((SELECT rowid FROM audio_blocks WHERE analysis_state='pending' AND analysis_attempts<3 AND media_path IS NOT NULL ORDER BY started_ms,id LIMIT 1),0)",
            ),
            (
                "sealing",
                "SELECT count(*) FROM audio_blocks WHERE analysis_state='sealing'",
            ),
        ];
        let mut results = Vec::new();
        for (name, sql) in queries {
            let mut timings = Vec::new();
            for _ in 0..21 {
                let t = Instant::now();
                let _: i64 = store.db.query_row(sql, [], |r| r.get(0)).unwrap();
                timings.push(t.elapsed().as_secs_f64() * 1000.);
            }
            timings.sort_by(f64::total_cmp);
            let plans = store
                .db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            results.push(serde_json::json!({"query":name,"median_ms":timings[10],"p95_ms":timings[19],"plan":plans}));
        }
        let t = Instant::now();
        assert!(store.take_pending_block().unwrap().is_none());
        let empty_claim_ms = t.elapsed().as_secs_f64() * 1000.;
        store
            .db
            .execute(
                "UPDATE audio_blocks SET analysis_state='pending' WHERE id=?1",
                [format!("b{:010}", rows - 1)],
            )
            .unwrap();
        let t = Instant::now();
        let claimed = store.take_pending_block().unwrap().unwrap();
        let last_claim_ms = t.elapsed().as_secs_f64() * 1000.;
        assert_eq!(claimed.id, format!("b{:010}", rows - 1));
        assert_eq!(store.state().unwrap().audio_used_bytes, (rows * 100) as u64);
        println!(
            "{}",
            serde_json::json!({"sqlite":rusqlite::version(),"rows":rows,"insert_ms":insert_ms,"queries":results,"empty_claim_ms":empty_claim_ms,"last_claim_ms":last_claim_ms})
        );
    }

    #[test]
    fn failed_migration_rolls_back_and_retry_runs_once() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE example(value INTEGER)")
            .unwrap();
        assert!(migrate(&db, "test", "INSERT INTO example VALUES(1); INVALID").is_err());
        assert!(db.is_autocommit());
        assert_eq!(
            db.query_row("SELECT count(*) FROM example", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        migrate(&db, "test", "INSERT INTO example VALUES(2)").unwrap();
        migrate(&db, "test", "INSERT INTO example VALUES(3)").unwrap();
        assert_eq!(
            db.query_row("SELECT sum(value) FROM example", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn audio_accounting_tracks_recovery_retention_and_cleanup() {
        let store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store.db.execute_batch("INSERT INTO audio_blocks
          (id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state)
          VALUES('a','s','microphone',0,5000,0,240000,0,'media/microphone/a.opus','pending','sealing');
          UPDATE audio_blocks SET bytes=123,analysis_state='pending' WHERE id='a';").unwrap();
        assert_eq!(store.state().unwrap().audio_used_bytes, 123);
        store
            .db
            .execute_batch("UPDATE audio_blocks SET bytes=0,media_path=NULL WHERE id='a'")
            .unwrap();
        assert_eq!(store.state().unwrap().audio_used_bytes, 0);
        store.db.execute_batch("UPDATE audio_blocks SET bytes=456 WHERE id='a'; DELETE FROM audio_blocks WHERE id='a'").unwrap();
        assert_eq!(store.state().unwrap().audio_used_bytes, 0);
    }

    #[test]
    fn totals_follow_updates_deletes_and_rollbacks() {
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store
            .db
            .execute("INSERT INTO orphan_media VALUES ('a',100,1)", [])
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO orphan_media VALUES ('a',40,2)
          ON CONFLICT(media_path) DO UPDATE SET bytes=excluded.bytes",
                [],
            )
            .unwrap();
        let tx = store.db.transaction().unwrap();
        tx.execute("UPDATE orphan_media SET bytes=500", []).unwrap();
        tx.rollback().unwrap();
        assert_eq!(store.state().unwrap().audio_used_bytes, 40);
        store.db.execute("DELETE FROM orphan_media", []).unwrap();
        assert_eq!(store.state().unwrap().audio_used_bytes, 0);
        init(&store.db).unwrap();
        assert_eq!(store.state().unwrap().audio_used_bytes, 0);
    }

    #[test]
    fn existing_database_bootstraps_once_and_keeps_indexes() {
        let store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        store
            .db
            .execute("INSERT INTO orphan_media VALUES ('a',123,1)", [])
            .unwrap();
        store.db.execute("DELETE FROM storage_usage", []).unwrap();
        init(&store.db).unwrap();
        assert_eq!(store.state().unwrap().audio_used_bytes, 123);
        let plan: String = store
            .db
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id FROM audio_blocks WHERE media_path=?1",
                params!["x"],
                |r| r.get(3),
            )
            .unwrap();
        assert!(plan.contains("audio_blocks_media_path"), "{plan}");
        let plan: String = store
            .db
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id FROM audio_blocks WHERE session_id=?1
             AND sample_start<?2 ORDER BY sample_start DESC LIMIT 6",
                params!["x", 100],
                |r| r.get(3),
            )
            .unwrap();
        assert!(plan.contains("audio_blocks_session_sample"), "{plan}");
    }
}
