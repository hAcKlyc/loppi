//! Opt-in local regression probes. Fixtures stay outside the repository.
//! All database mutations target a temporary copy. The separately opted-in
//! remote probe queries existing IDs only; it cannot submit audio.
use super::*;

#[test]
#[ignore = "explicit query of existing failed remote IDs; requires a trusted Core signature, HEAR_TEST_DB and HEAR_TEST_RESULTS"]
fn query_original_failed_asr_without_resubmission() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use uuid::Uuid;
    struct QueryOnly(std::path::PathBuf);
    impl usage::Meter for QueryOnly {
        fn begin(&self, _: &str, _: Option<Uuid>, _: Option<i64>) -> Result<String, String> {
            Err("query_only_no_submission".into())
        }
        fn llm_response(&self, _: &str, _: &serde_json::Value) -> Result<(), String> {
            unreachable!("query only")
        }
        fn asr_response(&self, id: Uuid, _: &str, body: &serde_json::Value) -> Result<(), String> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(self.0.join(format!("{id}.json")))
                .map_err(|e| e.to_string())?;
            file.write_all(body.to_string().as_bytes())
                .map_err(|e| e.to_string())
        }
        fn rejected(&self, _: &str) -> Result<(), String> {
            unreachable!("query only")
        }
        fn observation_failed(&self) {
            panic!("private result could not be saved")
        }
    }
    assert_eq!(std::env::var("HEAR_QUERY_ORIGINAL_ASR").as_deref(), Ok("1"));
    let db = Connection::open_with_flags(
        std::env::var("HEAR_TEST_DB").unwrap(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let output = std::path::PathBuf::from(std::env::var("HEAR_TEST_RESULTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let meter = QueryOnly(output);
    let reference = events::credential_reference(&db, hear_protocol::CloudProvider::Asr)
        .unwrap()
        .unwrap();
    let key = cloud::credential(hear_protocol::CloudProvider::Asr, &reference)
        .unwrap()
        .unwrap();
    let mut statement = db.prepare("SELECT id,remote_id FROM cloud_tasks WHERE provider='asr' AND state='failed' AND error_code='provider_unmapped_timing' ORDER BY created_ms").unwrap();
    let tasks = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for (id, remote) in tasks {
        let id = Uuid::parse_str(&id).unwrap();
        let remote = Uuid::parse_str(&remote).unwrap();
        let input = events::asr::input(&db, id).unwrap().unwrap();
        match cloud::query_audio(
            &key,
            remote,
            Instant::now() + Duration::from_secs(30),
            &meter,
        ) {
            Ok(cloud::AsrQuery::Complete(items)) => {
                let result = events::asr::map(&input, &items);
                eprintln!(
                    "original_asr {id} remote={remote} utterances={} map_error={:?}",
                    items.len(),
                    result.err()
                );
            }
            Ok(cloud::AsrQuery::Missing) => eprintln!("original_asr {id} missing"),
            Ok(cloud::AsrQuery::Processing) => eprintln!("original_asr {id} processing"),
            Err(error) => eprintln!("original_asr {id} query_error={}", error.code),
        }
    }
}

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
