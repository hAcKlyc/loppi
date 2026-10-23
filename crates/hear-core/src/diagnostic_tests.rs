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
    let mut statement = db.prepare("SELECT id,remote_id FROM cloud_tasks WHERE provider='asr' AND state='failed' AND (error_code LIKE 'provider_unmapped_timing%' OR error_code IN ('provider_invalid_utterance','provider_invalid_timing','provider_missing_utterances')) ORDER BY created_ms").unwrap();
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
#[ignore = "requires the Oct 23 frozen HEAR_TEST_DB; synthetic responses, temporary copy, no cloud calls"]
fn snapshot_partial_fit_publication_oct23() {
    use hear_protocol::CloudProvider;
    use uuid::Uuid;
    let (_directory, mut store) = fixture();
    let db = &mut store.db;
    let count = |db: &Connection, sql| db.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    let originals = (
        count(db, "SELECT count(*) FROM audio_blocks"),
        count(db, "SELECT count(*) FROM coarse_segments"),
        count(db, "SELECT count(*) FROM precise_segments"),
    );
    let paid: Vec<(String,Option<String>,i64)> = db.prepare("SELECT id,remote_id,submit_attempts FROM cloud_tasks WHERE provider='asr' AND submit_attempts>0 ORDER BY id").unwrap()
        .query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<Result<_,_>>().unwrap();
    let failed: Vec<String> = db.prepare("SELECT id FROM cloud_tasks WHERE provider='llm' AND state='failed' AND started_ms>=1792684800000 AND ended_ms<1792771200000 ORDER BY started_ms").unwrap()
        .query_map([],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    assert_eq!(failed.len(), 2);
    let now = 1792753200000;
    // The first meeting can progress even before its failed neighbour is retried.
    let first: (String,i64,i64,i64) = db.query_row("SELECT id,started_ms,ended_ms,source_mask FROM named_events WHERE started_ms=1792728949940 AND deleted_ms IS NULL",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(
        events::asr::settling_state(db, &first.0, first.1, first.2, first.3).unwrap(),
        None
    );
    events::asr::refresh(db, now).unwrap();
    let refs_before = count(db, "SELECT count(*) FROM event_asr_refs");
    for id in &failed {
        events::retry_task(db, Uuid::parse_str(id).unwrap(), now).unwrap();
        let task = events::take_task(db, CloudProvider::Llm, now)
            .unwrap()
            .unwrap();
        assert_eq!(&task.id.to_string(), id);
        let input = events::llm_input(db, &task).unwrap();
        let sent: serde_json::Value = serde_json::from_str(&input).unwrap();
        let prior = sent["context"]["previous_events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["source_mask"] == 1)
            .unwrap();
        // Deliberately corrupt optional fields and the redundant old list.
        // This is not a replay of the unavailable original provider response.
        let result = serde_json::json!({"events":[null,{"start_ms":task.start,"end_ms":task.end,
            "sources":"microphone","title":{},"summary":42,"merge_with_event_id":prior["id"]}],"unorganized":[0,0,999999]});
        assert_eq!(
            events::publish_fit_with_input(db, &task, &input, result, now).unwrap(),
            1
        );
        assert_eq!(
            db.query_row::<String, _, _>("SELECT state FROM cloud_tasks WHERE id=?1", [id], |r| r
                .get(0))
                .unwrap(),
            "complete"
        );
    }
    events::asr::refresh(db, now).unwrap();
    let refs_after = count(db, "SELECT count(*) FROM event_asr_refs");
    assert!(refs_after >= refs_before);
    assert_eq!(
        originals,
        (
            count(db, "SELECT count(*) FROM audio_blocks"),
            count(db, "SELECT count(*) FROM coarse_segments"),
            count(db, "SELECT count(*) FROM precise_segments")
        )
    );
    let still_paid: Vec<(String,Option<String>,i64)> = db.prepare("SELECT id,remote_id,submit_attempts FROM cloud_tasks WHERE provider='asr' AND submit_attempts>0 ORDER BY id").unwrap()
        .query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<Result<_,_>>().unwrap();
    assert_eq!(paid, still_paid);
    eprintln!(
        "partial_fit_snapshot windows=2 preserved_audio={} preserved_coarse={} preserved_precise={} preserved_paid_requests={} refs_before={} refs_after={}",
        originals.0,
        originals.1,
        originals.2,
        paid.len(),
        refs_before,
        refs_after
    );
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
        let block = store.db.query_row("SELECT session_id,source,sample_start,frames,media_path,started_ms,ended_ms FROM audio_blocks WHERE id=?1", [id], |r| Ok(PendingBlock {
            id: id.to_owned(), session_id: r.get(0)?, source: r.get(1)?, sample_start: r.get::<_,i64>(2)? as u64,
            frames: r.get::<_,i64>(3)? as usize, media_path: r.get(4)?, flush: false,
            started_ms: r.get(5)?, ended_ms: r.get(6)?,
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

#[test]
#[ignore = "private offline replay of existing ASR results"]
fn replay_oct21_asr_publication() {
    let (directory, mut store) = fixture();
    let response_dir = std::path::PathBuf::from(std::env::var("HEAR_ASR_RESPONSES").unwrap());
    let tasks = {
        let mut q=store.db.prepare("SELECT id,target_kind,target_id,target_version,model_id,started_ms,ended_ms,source,remote_id,attempts,submit_attempts,updated_ms FROM cloud_tasks WHERE provider='asr' AND state='failed' AND error_code='provider_unmapped_timing_audio_coverage' ORDER BY started_ms").unwrap();
        q.query_map([], |r| {
            let id: String = r.get(0)?;
            let remote: String = r.get(8)?;
            Ok(events::CloudTask {
                id: uuid::Uuid::parse_str(&id).unwrap(),
                provider: hear_protocol::CloudProvider::Asr,
                target_kind: r.get(1)?,
                target_id: r.get(2)?,
                target_version: r.get(3)?,
                model_id: r.get(4)?,
                start: r.get(5)?,
                end: r.get(6)?,
                source: r.get(7)?,
                remote_id: uuid::Uuid::parse_str(&remote).unwrap(),
                attempts: r.get(9)?,
                submit_attempts: r.get(10)?,
                updated_ms: r.get(11)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let mut successes = 0;
    let mut total = 0;
    for task in &tasks {
        let data = std::fs::read(response_dir.join(format!("{}.json", task.id))).unwrap();
        let response = serde_json::from_slice(&data).unwrap();
        let parsed = cloud::parse_asr_result(&response).unwrap();
        total += parsed.len();
        let input = events::asr::input(&store.db, task.id).unwrap().unwrap();
        let mapped = events::asr::map(&input, &parsed).unwrap();
        let before: i64 = store
            .db
            .query_row(
                "SELECT count(*) FROM precise_segments WHERE task_id=?1",
                [task.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        let result = events::publish_asr(&mut store.db, task, &parsed, task.updated_ms + 1);
        let after: i64 = store
            .db
            .query_row(
                "SELECT count(*) FROM precise_segments WHERE task_id=?1",
                [task.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        if result.is_ok() {
            successes += 1;
        }
        eprintln!(
            "offline_asr {} parsed={} result={:?} saved={}",
            task.id,
            parsed.len(),
            result,
            after - before
        );
        assert!(result.is_ok());
        assert_eq!(after - before, mapped.len() as i64);
        assert_eq!(
            events::publish_asr(&mut store.db, task, &parsed, task.updated_ms + 2).unwrap(),
            0
        );
    }
    eprintln!(
        "offline_asr_summary tasks={} successful={} parsed={}",
        tasks.len(),
        successes,
        total
    );
    assert_eq!(tasks.len(), 18);
    assert_eq!(successes, 18);
    let mut events = std::collections::BTreeSet::new();
    for task in &tasks {
        if events.insert(&task.target_id) {
            let id = uuid::Uuid::parse_str(&task.target_id).unwrap();
            let state = events::event_detail(&store.db, id, 0, 1)
                .unwrap()
                .processing_state;
            assert_ne!(state, "asr_failed");
            eprintln!("offline_event {} state={}", id, state);
        }
    }
    // The installed-version pause gap is now after the final confirmed block.
    let overlapping:bool=store.db.query_row("SELECT EXISTS(SELECT 1 FROM media_gaps g JOIN audio_blocks b ON b.session_id=g.source_session_id WHERE g.reason='manual_pause' AND g.started_ms<b.ended_ms AND coalesce(g.ended_ms,b.ended_ms)>b.started_ms)",[],|r|r.get(0)).unwrap();
    assert!(!overlapping);
    drop(directory);
}

#[test]
#[ignore = "requires HEAR_TEST_DB; replans rejected ASR on a temporary copy without submitting audio"]
fn snapshot_rejected_asr_recovery_preserves_history() {
    let (_directory, store) = fixture();
    let db = &store.db;
    let mut stmt = db.prepare("SELECT id,remote_id,specification,error_code FROM cloud_tasks t JOIN asr_inputs i ON i.task_id=t.id WHERE t.provider='asr' AND t.state='failed'").unwrap();
    let tasks = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let count = |sql| db.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    let segments = count("SELECT count(*) FROM precise_segments");
    let usage = count("SELECT count(*) FROM usage_requests");
    let mut now = count("SELECT max(updated_ms)+1 FROM cloud_tasks");
    let mut recovered = 0;
    let mut replacements = 0;
    for (id, remote, specification, error) in tasks {
        if !events::asr::rejected_result(&error) {
            continue;
        }
        let uuid = uuid::Uuid::parse_str(&id).unwrap();
        events::retry_task(db, uuid, now).unwrap();
        let history: (String,String,String,String) = db.query_row("SELECT t.state,t.remote_id,i.specification,t.error_code FROM cloud_tasks t JOIN asr_inputs i ON i.task_id=t.id WHERE t.id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(history, ("superseded".into(), remote, specification, error));
        let new: Vec<String> = db
            .prepare("SELECT id FROM cloud_tasks WHERE created_ms=?1 AND provider='asr'")
            .unwrap()
            .query_map([now], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!new.is_empty());
        for id in &new {
            let input = events::asr::input(db, uuid::Uuid::parse_str(id).unwrap())
                .unwrap()
                .unwrap();
            let start = input.parts[0].start;
            assert_eq!(input.duration_ms, input.parts.last().unwrap().end - start);
            for p in input.parts {
                assert_eq!(p.offset, p.start - start);
            }
        }
        eprintln!(
            "snapshot_asr_recovery original={id} replacements={}",
            new.len()
        );
        replacements += new.len();
        recovered += 1;
        now += 1;
    }
    assert!(recovered > 0);
    assert_eq!(segments, count("SELECT count(*) FROM precise_segments"));
    assert_eq!(usage, count("SELECT count(*) FROM usage_requests"));
    eprintln!(
        "snapshot_asr_recovery recovered={recovered} replacements={replacements} existing_segments={segments} usage_records={usage}"
    );
}

#[test]
#[ignore = "requires HEAR_TEST_DB/FROM/TO; offline replanning on a disposable snapshot copy"]
fn snapshot_pipeline_planning() {
    let (_directory, mut store) = fixture();
    let from: i64 = std::env::var("HEAR_TEST_FROM").unwrap().parse().unwrap();
    let to: i64 = std::env::var("HEAR_TEST_TO").unwrap().parse().unwrap();
    let events: Vec<(String,i64,i64,i64)> = store.db.prepare(
        "SELECT id,started_ms,ended_ms,source_mask FROM named_events WHERE deleted_ms IS NULL AND started_ms>=?1 AND started_ms<?2")
        .unwrap().query_map(params![from,to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
        .unwrap().collect::<Result<_,_>>().unwrap();
    let mut old = 0;
    let mut new = 0;
    let mut duration = 0;
    for (id, start, end, mask) in &events {
        old += store.db.query_row::<i64,_,_>("SELECT count(*) FROM transcript_tasks WHERE target_kind='event' AND target_id=?1 AND state!='superseded'",[id],|r|r.get(0)).unwrap();
        let target = uuid::Uuid::new_v4().to_string();
        let tx = store.db.transaction().unwrap();
        new += events::asr::schedule(
            &tx,
            "range",
            &target,
            "1",
            *start,
            *end,
            *mask,
            true,
            to,
            events::asr::VERSION,
        )
        .unwrap();
        tx.commit().unwrap();
        let inputs: Vec<String> = store.db.prepare("SELECT i.specification FROM asr_inputs i JOIN cloud_tasks t ON t.id=i.task_id WHERE t.target_id=?1")
            .unwrap().query_map([&target],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
        for input in inputs {
            let input: events::asr::Input = serde_json::from_str(&input).unwrap();
            duration += input.duration_ms;
            for part in &input.parts {
                assert_eq!(part.offset, part.start - input.parts[0].start);
            }
        }
    }
    let before: Vec<(String, String)> = store
        .db
        .prepare("SELECT id,input_hash FROM candidate_windows WHERE state='complete' AND started_ms>=?1 AND started_ms<?2")
        .unwrap()
         .query_map(params![from,to], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    println!(
        "snapshot_plan events={} old_requests={old} new_requests={new} encoded_ms={duration}",
        events.len()
    );
    store
        .db
        .execute("UPDATE candidate_scan_state SET from_ms=0", [])
        .unwrap();
    events::refresh_candidates(&mut store.db, to + 3_600_000, true).unwrap();
    for (id, hash) in &before {
        let actual: (String, String) = store
            .db
            .query_row(
                "SELECT input_hash,state FROM candidate_windows WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(actual, (hash.clone(), "complete".into()));
    }
    println!(
        "snapshot_pipeline events={} old_requests={old} new_requests={new} encoded_ms={duration} preserved_candidates={}",
        events.len(),
        before.len()
    );
}
