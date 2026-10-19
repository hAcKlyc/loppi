use super::*;
use crate::cloud::AsrUtterance;
use serde_json::json;

fn database() -> Connection {
    let db = crate::Store::new(Connection::open_in_memory().unwrap())
        .unwrap()
        .db;
    set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
    db
}
fn block(db: &Connection, id: &str, a: i64, b: i64, retained: bool) {
    db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active) VALUES(?1,'session','microphone',?2,?3,0,?4,100,?5,?6,'complete',?7)",params![id,a,b,(b-a)*48,retained.then(||format!("media/microphone/{id}.opus")),if retained {"retain"} else {"discard_no_speech_v2"},i64::from(retained)]).unwrap();
}
fn event(db: &mut Connection, a: i64, b: i64) -> Uuid {
    create_event(db, a, b, "会议", EventSources::Microphone, b + 1).unwrap()
}
fn task(db: &mut Connection) -> CloudTask {
    take_task(db, CloudProvider::Asr, 5_000_000)
        .unwrap()
        .unwrap()
}
fn utterance(a: i64, b: i64, text: &str) -> AsrUtterance {
    AsrUtterance {
        start_ms: a,
        end_ms: b,
        text: text.into(),
        speaker: Some("1".into()),
        words: None,
    }
}
fn count(db: &Connection) -> i64 {
    db.query_row(
        "SELECT count(*) FROM cloud_tasks WHERE provider='asr' AND state!='superseded'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}
fn auto(db: &mut Connection, a: i64, b: i64) -> Uuid {
    let id = Uuid::new_v4();
    let tx = db.transaction().unwrap();
    tx.execute("INSERT INTO named_events(id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,created_ms) VALUES(?1,?2,?3,'会议',1,1,'automatic',0,0,1,?3)",params![id.to_string(),a,b]).unwrap();
    schedule(
        &tx,
        "event",
        &id.to_string(),
        &asr_version(1, a, b),
        a,
        b,
        1,
        false,
        b,
        VERSION,
    )
    .unwrap();
    tx.commit().unwrap();
    id
}
#[test]
fn confirmed_long_silence_compacts_but_unknown_and_capture_gaps_do_not() {
    for reason in ["confirmed", "unknown", "capture", "cleared"] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        block(&db, "silence", 6000, 26000, false);
        block(&db, "b", 26000, 31000, true);
        match reason {
            "unknown" => {
                db.execute("DELETE FROM audio_blocks WHERE id='silence'", [])
                    .unwrap();
            }
            "capture" => {
                db.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES('gap','microphone',10000,11000,'capture_failure')",[]).unwrap();
            }
            "cleared" => {
                db.execute("INSERT INTO clear_ranges(from_ms,to_ms,source_mask,mode,committed_ms) VALUES(10000,11000,1,'raw_audio',32000)",[]).unwrap();
            }
            _ => {}
        }
        event(&mut db, 1000, 31000);
        assert_eq!(
            count(&db),
            if reason == "confirmed" { 1 } else { 2 },
            "{reason}"
        );
        let t = task(&mut db);
        let i = input(&db, t.id).unwrap().unwrap();
        if reason == "confirmed" {
            assert_eq!(i.duration_ms, 11000);
            assert_eq!(i.parts[1].offset, 6000);
        }
    }
}
#[test]
fn mapping_splits_only_fully_aligned_words_and_preserves_punctuation() {
    let p = |start, end, offset| Part {
        block_id: "x".into(),
        path: "x".into(),
        frames: 240000,
        block_start: start,
        start,
        end,
        offset,
    };
    let i = Input {
        parts: vec![p(1000, 6000, 0), p(26000, 31000, 6000)],
        duration_ms: 11000,
    };
    let mut u = utterance(1000, 8000, "你好，世界！");
    u.words = Some(
        json!([{"text":"你好","start_time":1000,"end_time":2000},{"text":"世界","start_time":7000,"end_time":8000}]),
    );
    let out = map(&i, &[u.clone()]).unwrap();
    assert_eq!(
        out.iter()
            .map(|u| (&*u.text, u.start_ms, u.end_ms))
            .collect::<Vec<_>>(),
        vec![("你好，", 2000, 3000), ("世界！", 27000, 28000)]
    );
    u.words = None;
    assert!(map(&i, &[u.clone()]).is_err());
    u.words = Some(json!([{"text":"你好","start_time":1000,"end_time":2000}]));
    assert!(map(&i, &[u.clone()]).is_err());
    u.start_ms = 5500;
    assert!(map(&i, &[u]).is_err());
    assert!(map(&i, &[utterance(5000, 6000, "虚构")]).is_err());
}
#[test]
fn failed_mapping_is_atomic_and_publishing_is_idempotent() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    block(&db, "s", 6000, 26000, false);
    block(&db, "b", 26000, 31000, true);
    event(&mut db, 1000, 31000);
    let t = task(&mut db);
    assert!(
        publish_asr(
            &mut db,
            &t,
            &[utterance(0, 1000, "有效"), utterance(4000, 7000, "未对齐")],
            5000001
        )
        .is_err()
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM precise_segments", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    publish_asr(&mut db, &t, &[utterance(0, 1000, "有效")], 5000002).unwrap();
    assert_eq!(
        publish_asr(&mut db, &t, &[utterance(0, 1000, "有效")], 5000003).unwrap(),
        0
    );
}
#[test]
fn extension_preserves_empty_failed_and_completed_input_without_resubmitting() {
    for status in ["complete", "empty", "failed"] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        let id = event(&mut db, 1000, 6000);
        let t = task(&mut db);
        if status == "failed" {
            task_failed(&db, &t, "provider_unmapped_timing", 5000001).unwrap();
        } else {
            let items = if status == "empty" {
                vec![]
            } else {
                vec![utterance(500, 1500, "原句")]
            };
            publish_asr(&mut db, &t, &items, 5000001).unwrap();
        }
        block(&db, "b", 6000, 11000, true);
        edit_event(&mut db, id, 1, 1000, 11000, "会议", None, 5000002).unwrap();
        let next = task(&mut db);
        let i = input(&db, next.id).unwrap().unwrap();
        assert_eq!(i.parts.len(), 1);
        assert_eq!(i.parts[0].block_id, "b");
        assert_eq!(count(&db), 2);
        if status == "complete" {
            assert_eq!(
                event_detail(&db, id, 0, 10).unwrap().precise_segments[0].text,
                "原句"
            );
        }
    }
}
#[test]
fn cropped_sentence_is_partial_and_boundary_restore_reuses_original_id() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    let id = event(&mut db, 1000, 6000);
    let t = task(&mut db);
    publish_asr(&mut db, &t, &[utterance(0, 3000, "完整句子")], 5000001).unwrap();
    let old = event_detail(&db, id, 0, 10).unwrap().precise_segments[0].id;
    edit_event(&mut db, id, 1, 2000, 6000, "会议", None, 5000002).unwrap();
    let detail = event_detail(&db, id, 0, 10).unwrap();
    assert_eq!(detail.processing_state, "partial_transcript");
    assert!(detail.precise_segments.is_empty());
    assert!(
        take_task(&mut db, CloudProvider::Asr, 5000003)
            .unwrap()
            .is_none()
    );
    edit_event(&mut db, id, 2, 1000, 6000, "会议", None, 5000004).unwrap();
    assert_eq!(
        event_detail(&db, id, 0, 10).unwrap().precise_segments[0].id,
        old
    );
    assert_eq!(count(&db), 1);
}
#[test]
fn split_and_merge_inflight_input_preserves_remote_ownership_and_coverage() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    let id = event(&mut db, 1000, 6000);
    let t = task(&mut db);
    mark_submit_intent(&db, &t, 5000001).unwrap();
    let (a, b) = split_event(&mut db, id, 1, 3500, "前", "后", 5000002).unwrap();
    assert!(task_current(&db, &t).unwrap());
    assert_eq!(count(&db), 1);
    publish_asr(
        &mut db,
        &t,
        &[utterance(0, 1000, "前句"), utterance(3000, 4000, "后句")],
        5000003,
    )
    .unwrap();
    assert_eq!(
        event_detail(&db, a, 0, 10).unwrap().precise_segments.len(),
        1
    );
    assert_eq!(
        event_detail(&db, b, 0, 10).unwrap().precise_segments.len(),
        1
    );
    let joined = merge_events(&mut db, a, 1, b, 1, "合并", 5000004).unwrap();
    assert_eq!(
        event_detail(&db, joined, 0, 10)
            .unwrap()
            .precise_segments
            .len(),
        2
    );
    assert_eq!(count(&db), 1);
}
#[test]
fn stability_waits_for_local_backlog_and_does_not_starve_later_events() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    let id = auto(&mut db, 1000, 6000);
    refresh(&mut db, 905999).unwrap();
    assert_eq!(count(&db), 0);
    assert_eq!(
        event_detail(&db, id, 0, 10).unwrap().processing_state,
        "settling_asr"
    );
    db.execute("UPDATE audio_blocks SET analysis_state='pending'", [])
        .unwrap();
    refresh(&mut db, 906000).unwrap();
    assert_eq!(count(&db), 0);
    db.execute("UPDATE audio_blocks SET analysis_state='complete'", [])
        .unwrap();
    db.execute(
        "INSERT INTO candidate_windows VALUES('fit',1000,900000,'hash','processing',NULL)",
        [],
    )
    .unwrap();
    refresh(&mut db, 906001).unwrap();
    assert_eq!(count(&db), 0);
    db.execute("UPDATE candidate_windows SET state='complete'", [])
        .unwrap();
    refresh(&mut db, 906002).unwrap();
    assert_eq!(count(&db), 1);
    for n in 0..33 {
        let a = 2_000_000 + n * 2_000_000;
        block(&db, &format!("blocked{n}"), a, a + 5000, true);
        auto(&mut db, a, a + 5000);
    }
    db.execute(
        "UPDATE audio_blocks SET analysis_state='pending' WHERE id LIKE 'blocked%'",
        [],
    )
    .unwrap();
    block(&db, "last", 80_000_000, 80_005_000, true);
    auto(&mut db, 80_000_000, 80_005_000);
    refresh(&mut db, 90_000_000).unwrap();
    assert_eq!(count(&db), 2);
}

#[test]
fn projection_and_scheduler_share_next_event_horizon_and_source_scope() {
    for failed_at in [9_000, 10_000, 20_000] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        let first = auto(&mut db, 1000, 6000);
        auto(&mut db, 10_000, 15_000);
        db.execute(
            "INSERT INTO candidate_windows VALUES('later',?1,30000,'hash','failed',NULL)",
            [failed_at],
        )
        .unwrap();
        let expected = if failed_at < 10_000 {
            Some("fit_failed")
        } else {
            None
        };
        assert_eq!(
            settling_state(&db, &first.to_string(), 1000, 6000, 1).unwrap(),
            expected
        );
        assert_eq!(
            event_detail(&db, first, 0, 10).unwrap().processing_state,
            expected.unwrap_or("settling_asr")
        );
        refresh(&mut db, 1_000_000).unwrap();
        assert_eq!(count(&db), if failed_at < 10_000 { 0 } else { 1 });
    }
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    let first = auto(&mut db, 1000, 6000);
    auto(&mut db, 10_000, 15_000);
    block(&db, "failed", 8000, 9000, true);
    db.execute(
        "UPDATE audio_blocks SET analysis_state='failed',source='system' WHERE id='failed'",
        [],
    )
    .unwrap();
    assert_eq!(
        settling_state(&db, &first.to_string(), 1000, 6000, 1).unwrap(),
        None
    );
    db.execute(
        "UPDATE audio_blocks SET source='microphone' WHERE id='failed'",
        [],
    )
    .unwrap();
    assert_eq!(
        event_detail(&db, first, 0, 10).unwrap().processing_state,
        "analysis_failed"
    );
    refresh(&mut db, 1_000_000).unwrap();
    assert_eq!(count(&db), 0);
}

#[test]
fn unresolved_retained_audio_is_recognized_without_claiming_vad_speech() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    db.execute(
        "UPDATE audio_blocks SET retention_decision='retain_unresolved_v2',speech_active=0",
        [],
    )
    .unwrap();
    auto(&mut db, 1000, 6000);
    refresh(&mut db, 1_000_000).unwrap();
    assert_eq!(count(&db), 1);
    let t = task(&mut db);
    assert_eq!(input(&db, t.id).unwrap().unwrap().duration_ms, 5000);
}

#[test]
fn mapping_failure_codes_identify_the_rejected_coordinate() {
    let i = Input {
        parts: vec![
            Part {
                block_id: "a".into(),
                path: "a".into(),
                frames: 240000,
                block_start: 1000,
                start: 1000,
                end: 6000,
                offset: 0,
            },
            Part {
                block_id: "b".into(),
                path: "b".into(),
                frames: 240000,
                block_start: 26000,
                start: 26000,
                end: 31000,
                offset: 6000,
            },
        ],
        duration_ms: 11000,
    };
    let mut u = utterance(1000, 8000, "你好世界");
    assert_eq!(
        map(&i, &[u.clone()]).unwrap_err(),
        "provider_unmapped_timing_words_missing"
    );
    u.words = Some(
        json!([{"text":"你好","start_time":1000,"end_time":2000},{"text":"世界","start_time":5500,"end_time":8000}]),
    );
    assert_eq!(
        map(&i, &[u.clone()]).unwrap_err(),
        "provider_unmapped_timing_word_in_separator"
    );
    u.words.as_mut().unwrap()[1]["start_time"] = json!(7000);
    u.start_ms = 5500;
    u.words.as_mut().unwrap()[0]["start_time"] = json!(-1);
    assert_eq!(
        map(&i, &[u]).unwrap_err(),
        "provider_unmapped_timing_word_bounds"
    );
}
#[test]
fn long_events_split_by_encoded_duration_not_wall_clock() {
    let mut db = database();
    for n in 0..125 {
        let a = 1000 + n * 20000;
        block(&db, &format!("a{n}"), a, a + 10000, true);
        block(&db, &format!("s{n}"), a + 10000, a + 20000, false);
    }
    event(&mut db, 1000, 2_491_000);
    assert_eq!(count(&db), 2);
    let mut total = 0;
    for _ in 0..2 {
        let t = task(&mut db);
        let i = input(&db, t.id).unwrap().unwrap();
        assert!(i.duration_ms <= WINDOW_MS);
        total += i.duration_ms;
    }
    assert_eq!(total, 125 * 10000 + 123 * 1000);
}

#[test]
#[ignore = "paid provider probe; requires HEAR_ASR_LIVE_KEY and synthetic PCM paths"]
fn live_synthetic_compaction_keeps_phrase_timing_and_metered_duration() {
    use crate::{cloud, usage::Meter};
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };
    struct Probe {
        submitted: RefCell<Vec<i64>>,
        reported: RefCell<Vec<i64>>,
    }
    impl Meter for Probe {
        fn begin(&self, _: &str, _: Option<Uuid>, audio: Option<i64>) -> Result<String, String> {
            self.submitted.borrow_mut().push(audio.unwrap());
            Ok("probe".into())
        }
        fn asr_response(&self, _: Uuid, _: &str, v: &serde_json::Value) -> Result<(), String> {
            if let Some(n) = v["audio_info"]["duration"].as_i64() {
                self.reported.borrow_mut().push(n);
            }
            Ok(())
        }
        fn llm_response(&self, _: &str, _: &serde_json::Value) -> Result<(), String> {
            Ok(())
        }
        fn rejected(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn observation_failed(&self) {
            panic!("probe metering failed")
        }
    }
    let key = std::env::var("HEAR_ASR_LIVE_KEY").expect("explicit live key required");
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media/microphone");
    std::fs::create_dir_all(&media).unwrap();
    let mut parts = Vec::new();
    let mut actual = 0;
    for (n, name) in ["HEAR_ASR_SYNTH_A", "HEAR_ASR_SYNTH_B"].iter().enumerate() {
        let data = std::fs::read(std::env::var(name).unwrap()).unwrap();
        let mut pcm: Vec<f32> = data
            .chunks_exact(2)
            .map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0)
            .collect();
        pcm.resize(pcm.len().div_ceil(48) * 48, 0.0);
        // Retained capture blocks include ordinary silence around speech.
        pcm.splice(0..0, std::iter::repeat_n(0.0, 24_000));
        pcm.extend(std::iter::repeat_n(0.0, 24_000));
        assert!(pcm.len() < 480000);
        let id = Uuid::new_v4();
        let mut encoder = crate::media::OpusBlock::create(&media, id).unwrap();
        encoder.push(&pcm).unwrap();
        let file = encoder.seal().unwrap();
        let len = pcm.len() as i64 / 48;
        let start = if n == 0 { 0 } else { actual + 30000 };
        parts.push(Part {
            block_id: id.to_string(),
            path: file.path.strip_prefix(dir.path()).unwrap().into(),
            frames: pcm.len(),
            block_start: start,
            start,
            end: start + len,
            offset: start,
        });
        actual = start + len;
    }
    let original = Input {
        parts: parts.clone(),
        duration_ms: actual,
    };
    let a_end = parts[0].end;
    parts[1].offset = a_end + 1000;
    let compact = Input {
        duration_ms: parts[1].offset + parts[1].end - parts[1].start,
        parts,
    };
    let meter = Probe {
        submitted: RefCell::new(vec![]),
        reported: RefCell::new(vec![]),
    };
    let mut responses = Vec::new();
    for (label, input) in [("original", &original), ("compact", &compact)] {
        let bytes = assemble(dir.path(), input).unwrap();
        let request = Uuid::new_v4();
        cloud::submit_audio(
            &key,
            request,
            &bytes,
            cloud::AsrAudioFormat::Ogg,
            input.duration_ms,
            Instant::now() + Duration::from_secs(60),
            &meter,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(180);
        let items = loop {
            assert!(Instant::now() < deadline, "provider probe timeout");
            match cloud::query_audio(&key, request, deadline, &meter).unwrap() {
                cloud::AsrQuery::Complete(items) => break items,
                _ => std::thread::sleep(Duration::from_secs(2)),
            }
        };
        let mapped = map(input, &items).unwrap();
        assert!(!mapped.is_empty());
        eprintln!(
            "synthetic probe {label}: encoded_ms={} reported_ms={:?} bytes={} results={:?}",
            input.duration_ms,
            meter.reported.borrow().last(),
            bytes.len(),
            mapped
                .iter()
                .map(|u| (&u.text, u.start_ms, u.end_ms))
                .collect::<Vec<_>>()
        );
        assert!(mapped.iter().any(|u| u.start_ms >= original.parts[1].start));
        responses.push(mapped);
    }
    assert_eq!(
        *meter.submitted.borrow(),
        vec![original.duration_ms, compact.duration_ms]
    );
    assert_eq!(original.duration_ms - compact.duration_ms, 29000);
    for part in &original.parts {
        let onset = |items: &Vec<AsrUtterance>| {
            items
                .iter()
                .find(|u| u.start_ms >= part.start && u.end_ms <= part.end)
                .expect("phrase must stay within original audio")
                .start_ms
        };
        assert!(
            (onset(&responses[0]) - onset(&responses[1])).abs() <= 500,
            "provider phrase timing changed by more than 500ms"
        );
    }
}

#[test]
fn upgrade_preserves_legacy_submitted_and_completed_but_replans_unsent_inputs() {
    let mut db = database();
    let mut ids = Vec::new();
    for (n, state) in ["complete", "submitted", "pending"].iter().enumerate() {
        let a = 1000 + n as i64 * 20000;
        block(&db, &format!("old{n}"), a, a + 5000, true);
        let id = event(&mut db, a, a + 5000);
        let t: String = db
            .query_row(
                "SELECT id FROM cloud_tasks WHERE target_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        db.execute("UPDATE cloud_tasks SET state=?2,submit_attempts=?3,processor_version='volc-legacy-v1' WHERE id=?1",params![t,state,i64::from(*state!="pending")]).unwrap();
        ids.push((id, t));
    }
    db.execute_batch("DROP VIEW transcript_tasks;DROP TABLE event_asr_refs;DROP TABLE event_asr_state;DROP TABLE asr_inputs;DELETE FROM core_migrations WHERE name='event_asr_inputs_v1'").unwrap();
    init(&db).unwrap();
    init(&db).unwrap();
    assert_eq!(count(&db), 2);
    let old_id = Uuid::parse_str(&ids[1].1).unwrap();
    let submitted = task(&mut db);
    assert_eq!(submitted.id, old_id);
    assert!(input(&db, old_id).unwrap().is_none());
    recover_tasks(&mut db, 5000001).unwrap();
    let resumed = take_task(&mut db, CloudProvider::Asr, 5060002)
        .unwrap()
        .unwrap();
    assert_eq!(resumed.id, old_id);
    assert_eq!(resumed.remote_id, submitted.remote_id);
    assert_eq!(resumed.submit_attempts, 1);
    publish_asr(
        &mut db,
        &resumed,
        &[utterance(500, 1500, "旧请求返回")],
        5000003,
    )
    .unwrap();
    assert_eq!(
        event_detail(&db, ids[1].0, 0, 10).unwrap().precise_segments[0].started_ms,
        21500
    );
    let replanned = task(&mut db);
    assert_eq!(replanned.target_id, ids[2].0.to_string());
    assert!(input(&db, replanned.id).unwrap().is_some());
    assert_eq!(count(&db), 3);
}
#[test]
fn independent_event_can_close_stability_early_and_failures_stay_visible() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    let first = auto(&mut db, 1000, 6000);
    block(&db, "b", 10000, 15000, true);
    auto(&mut db, 10000, 15000);
    refresh(&mut db, 15000).unwrap();
    assert_eq!(count(&db), 1);
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    let id = auto(&mut db, 1000, 6000);
    db.execute("UPDATE audio_blocks SET analysis_state='failed'", [])
        .unwrap();
    refresh(&mut db, 1_000_000).unwrap();
    assert_eq!(count(&db), 0);
    assert_eq!(
        event_detail(&db, id, 0, 10).unwrap().processing_state,
        "analysis_failed"
    );
    let _ = first;
}

#[test]
fn raw_clear_blocks_unsent_upload_but_keeps_paid_results_and_completed_coverage() {
    for submitted in [false, true] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        let id = event(&mut db, 1000, 6000);
        let mut t = task(&mut db);
        if submitted {
            mark_submit_intent(&db, &t, 5000001).unwrap();
            t.submit_attempts = 1;
        }
        db.execute(
            "INSERT INTO clear_ranges VALUES('clear',1000,6000,1,'raw_audio',5000002)",
            [],
        )
        .unwrap();
        db.execute(
            "UPDATE audio_blocks SET media_path=NULL,retention_decision='raw_audio' WHERE id='a'",
            [],
        )
        .unwrap();
        assert_eq!(task_current(&db, &t).unwrap(), submitted);
        if submitted {
            publish_asr(&mut db, &t, &[utterance(0, 1000, "保留文字")], 5000003).unwrap();
            block(&db, "b", 6000, 11000, true);
            edit_event(&mut db, id, 1, 1000, 11000, "会议", None, 5000004).unwrap();
            assert_eq!(
                event_detail(&db, id, 0, 10).unwrap().precise_segments[0].text,
                "保留文字"
            );
            let next = task(&mut db);
            assert_eq!(input(&db, next.id).unwrap().unwrap().parts[0].block_id, "b");
        }
    }
}

#[test]
fn partial_cleanup_relocates_unaffected_immutable_input_after_original_unlink() {
    use crate::{cleanup, media};
    use hear_protocol::CleanupMode;
    for mode in [CleanupMode::RawAudio, CleanupMode::AllData] {
        let mut db = database();
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let mut encoder =
            media::OpusBlock::create(&dir.path().join("media/microphone"), id).unwrap();
        encoder
            .push(
                &(0..480000)
                    .map(|i| (i as f32 * 0.02).sin() * 0.1)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let file = encoder.seal().unwrap();
        block(&db, &id.to_string(), 0, 10000, true);
        event(&mut db, 1000, 2000);
        let t = task(&mut db);
        let frozen = input(&db, t.id).unwrap().unwrap();
        let before = assemble(dir.path(), &frozen).unwrap();
        assert!(!before.is_empty());
        let preview =
            cleanup::preview(&db, 5000, 6000, EventSources::Microphone, mode, 5000001).unwrap();
        let scope = cleanup::verified_scope(&db, preview.token, true, 5000001).unwrap();
        let prepared =
            cleanup::prepare_media(dir.path(), &scope, cleanup::originals(&db, &scope).unwrap())
                .unwrap();
        let tx = db.transaction().unwrap();
        cleanup::apply(&tx, &scope, &prepared, 5000002).unwrap();
        tx.commit().unwrap();
        cleanup::drain_deletions(&db, dir.path(), 5000003).unwrap();
        assert!(!file.path.exists());
        assert!(task_current(&db, &t).unwrap());
        let resolved = resolve_media(&frozen, &audio_manifest(&db, &t).unwrap()).unwrap();
        assert_eq!(resolved.duration_ms, frozen.duration_ms);
        assert_ne!(resolved.parts[0].path, frozen.parts[0].path);
        let encoded = assemble(dir.path(), &resolved).unwrap();
        let decoded = media::decode_reader(std::io::Cursor::new(encoded), 48000).unwrap();
        assert_eq!(decoded.len(), 48000);
        assert_eq!(
            map(&frozen, &[utterance(100, 900, "原音")]).unwrap()[0].start_ms,
            1100
        );
        assert_eq!(
            serde_json::to_string(&input(&db, t.id).unwrap().unwrap()).unwrap(),
            serde_json::to_string(&frozen).unwrap()
        );
    }
}
#[test]
fn split_survivor_extension_reuses_all_completed_parent_inputs() {
    for separate in [false, true] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        let end = if separate { 16000 } else { 6000 };
        if separate {
            block(&db, "b", 11000, 16000, true);
        }
        let id = event(&mut db, 1000, end);
        let first = task(&mut db);
        let items = if separate {
            vec![utterance(0, 2000, "前句")]
        } else {
            vec![utterance(0, 2000, "前句"), utterance(3000, 5000, "后句")]
        };
        publish_asr(&mut db, &first, &items, 5000001).unwrap();
        if separate {
            let second = task(&mut db);
            publish_asr(&mut db, &second, &[utterance(0, 2000, "后句")], 5000002).unwrap();
        }
        let original = event_detail(&db, id, 0, 10).unwrap().precise_segments;
        let tx = db.transaction().unwrap();
        crate::knowledge::correct_text(&tx, original[0].id, "保留人工修改", 5000003).unwrap();
        tx.commit().unwrap();
        let original_count = count(&db);
        let (left, right) = split_event(&mut db, id, 1, 3500, "前", "后", 5000004).unwrap();
        let tx = db.transaction().unwrap();
        delete_event_in(&tx, right, 1, 5000005).unwrap();
        tx.commit().unwrap();
        edit_event(&mut db, left, 1, 1000, end, "恢复范围", None, 5000006).unwrap();
        assert_eq!(count(&db), original_count);
        assert!(
            take_task(&mut db, CloudProvider::Asr, 5000007)
                .unwrap()
                .is_none()
        );
        let restored = event_detail(&db, left, 0, 10).unwrap().precise_segments;
        assert_eq!(
            restored.iter().map(|p| p.id).collect::<Vec<_>>(),
            original.iter().map(|p| p.id).collect::<Vec<_>>()
        );
        assert_eq!(restored[0].text, "保留人工修改");
        assert_eq!(
            db.query_row("SELECT count(*) FROM voice_observations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
