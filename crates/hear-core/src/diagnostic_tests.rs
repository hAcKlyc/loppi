//! Opt-in local regression probes. Fixtures stay outside the repository.
//! All database mutations target a temporary copy; no cloud requests are made.
use super::*;

fn fixture() -> (tempfile::TempDir, Store) {
    let source =
        std::env::var("HEAR_TEST_DB").expect("set HEAR_TEST_DB to a frozen SQLite snapshot");
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("fixture.sqlite");
    let source =
        Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    source
        .execute("VACUUM INTO ?1", [destination.to_str().unwrap()])
        .unwrap();
    let store = Store::new(Connection::open(destination).unwrap()).unwrap();
    (directory, store)
}

#[test]
#[ignore = "requires HEAR_TEST_DB; operates on a temporary copy, never the live database"]
fn snapshot_retention_and_event_projection() {
    let (_directory, mut store) = fixture();
    let count = |db: &Connection, sql| db.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    let media_before = count(
        &store.db,
        "SELECT count(*) FROM audio_blocks WHERE media_path IS NOT NULL",
    );
    let speech_before = count(
        &store.db,
        "SELECT count(*) FROM audio_blocks WHERE speech_active=1",
    );
    let pending_before = count(
        &store.db,
        "SELECT count(*) FROM audio_blocks WHERE analysis_state='complete' AND retention_decision='pending_vad_v2'",
    );
    let waiting = |db: &Connection| -> Vec<(String, i64, i64, i64)> {
        db.prepare("SELECT e.id,e.started_ms,e.ended_ms,e.source_mask FROM named_events e JOIN event_asr_state s ON e.id=s.event_id WHERE e.deleted_ms IS NULL AND coalesce(s.planned_version,'')!=printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms)").unwrap()
          .query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap().collect::<Result<_,_>>().unwrap()
    };
    let blocked = |db: &Connection| {
        waiting(db)
            .into_iter()
            .filter(|(id, a, b, m)| {
                events::asr::settling_state(db, id, *a, *b, *m)
                    .unwrap()
                    .is_some()
            })
            .count()
    };
    let blocked_before = blocked(&store.db);
    let mut settled = 0;
    loop {
        let tx = store.db.transaction().unwrap();
        let batch = retention::settle_unreachable(&tx).unwrap();
        tx.commit().unwrap();
        settled += batch;
        if batch == 0 {
            break;
        }
    }
    assert_eq!(
        media_before,
        count(
            &store.db,
            "SELECT count(*) FROM audio_blocks WHERE media_path IS NOT NULL"
        )
    );
    assert_eq!(
        speech_before,
        count(
            &store.db,
            "SELECT count(*) FROM audio_blocks WHERE speech_active=1"
        )
    );
    eprintln!(
        "retention_snapshot pending_before={pending_before} settled={settled} blocked_before={blocked_before} blocked_after={} media_preserved={media_before}",
        blocked(&store.db)
    );
    for (id, _, _, _) in waiting(&store.db) {
        let detail =
            events::event_detail(&store.db, uuid::Uuid::parse_str(&id).unwrap(), 0, 1).unwrap();
        eprintln!("event_projection {id} {}", detail.processing_state);
    }
}

#[test]
#[ignore = "requires HEAR_TEST_DB, HEAR_TEST_DATA, HEAR_TEST_WORKER and HEAR_TEST_BLOCKS (comma-separated IDs)"]
fn replay_saved_vad_through_production_decoder_and_publication() {
    let (_directory, mut store) = fixture();
    let data = std::path::PathBuf::from(std::env::var("HEAR_TEST_DATA").unwrap());
    let worker_path = std::path::PathBuf::from(std::env::var("HEAR_TEST_WORKER").unwrap());
    for id in std::env::var("HEAR_TEST_BLOCKS").unwrap().split(',') {
        let block = store.db.query_row("SELECT session_id,source,sample_start,frames,media_path FROM audio_blocks WHERE id=?1", [id], |r| Ok(PendingBlock {
            id: id.to_owned(), session_id: r.get(0)?, source: r.get(1)?, sample_start: r.get::<_,i64>(2)? as u64,
            frames: r.get::<_,i64>(3)? as usize, media_path: r.get(4)?, flush: false,
        })).unwrap();
        let mut worker = analysis::AnalysisClient::start(
            &worker_path,
            "vad",
            &model_pack::installed_path(&data),
        )
        .unwrap();
        let context = store.replay_context(&block).unwrap();
        let replay_blocks = context.len();
        for prior in context {
            let pcm = decode_saved_block(&data, &prior).unwrap();
            let replay = worker
                .analyze(
                    &prior.source,
                    &prior.session_id,
                    prior.sample_start,
                    &pcm,
                    false,
                )
                .unwrap();
            assert!(replay.error.is_none());
            let tx = store.db.transaction().unwrap();
            coarse::enqueue(&tx, &prior, replay.segments).unwrap();
            tx.commit().unwrap();
        }
        let pcm = decode_saved_block(&data, &block).unwrap();
        let result = worker
            .analyze(
                &block.source,
                &block.session_id,
                block.sample_start,
                &pcm,
                block.flush,
            )
            .unwrap();
        assert!(result.error.is_none());
        store
            .db
            .execute(
                "UPDATE audio_blocks SET analysis_state='processing' WHERE id=?1",
                [id],
            )
            .unwrap();
        for region in &result.segments {
            eprintln!(
                "vad_region {id} start={} end={} block_end={}",
                region.start_sample,
                region.end_sample,
                (block.sample_start + block.frames as u64) / 3
            );
        }
        let regions: Vec<_> = result
            .segments
            .iter()
            .map(|r| (r.start_sample, r.end_sample))
            .collect();
        let jobs_before: i64 = store
            .db
            .query_row("SELECT count(*) FROM coarse_jobs", [], |r| r.get(0))
            .unwrap();
        store.publish_analysis(&block, result).unwrap();
        for (start, end) in regions {
            let ranges = store
                .db
                .prepare(
                    "SELECT start_sample,end_sample FROM coarse_jobs
              WHERE session_id=?1 AND start_sample<?3 AND end_sample>?2 ORDER BY start_sample",
                )
                .unwrap()
                .query_map(params![block.session_id, start as i64, end as i64], |r| {
                    Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let mut cursor = start;
            for (a, b) in ranges {
                assert!(
                    a <= cursor,
                    "recovered speech left a hole before a later job"
                );
                cursor = cursor.max(b);
            }
            assert!(
                cursor >= end,
                "published success without durable speech coverage"
            );
            let missing_media: i64 = store
                .db
                .query_row(
                    "SELECT count(*) FROM audio_blocks WHERE session_id=?1
              AND sample_start<?3*3 AND sample_start+frames>?2*3 AND media_path IS NULL",
                    params![block.session_id, start as i64, end as i64],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(missing_media, 0, "recovered speech lost its original audio");
        }
        let jobs_after: i64 = store
            .db
            .query_row("SELECT count(*) FROM coarse_jobs", [], |r| r.get(0))
            .unwrap();
        eprintln!(
            "vad_replay {id} replay_blocks={replay_blocks} frames={} published=true added_jobs={} coverage_verified=true media_preserved=true",
            pcm.len(),
            jobs_after - jobs_before
        );
    }
}
