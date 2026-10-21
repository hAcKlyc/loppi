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
fn long_silence_and_unknown_or_capture_gaps_split_physical_requests() {
    for reason in ["unknown", "capture", "cleared"] {
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
        assert_eq!(count(&db), 2, "{reason}");
        let t = task(&mut db);
        let i = input(&db, t.id).unwrap().unwrap();
        assert_eq!(i.duration_ms, 5000);
        assert_eq!(i.parts.len(), 1);
        assert_eq!(i.parts[0].offset, 0);
    }
}
#[test]
fn short_pauses_preserve_one_linear_clock_and_long_pauses_start_a_new_request() {
    for gap in [0, 1000, 3000, 20_000, 60_000, 60_001, 180_000] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        block(&db, "b", 6000 + gap, 11000 + gap, true);
        if gap > 0 {
            block(&db, "silence", 6000, 6000 + gap, false);
        }
        event(&mut db, 1000, 11000 + gap);
        assert_eq!(count(&db), if gap <= 60_000 { 1 } else { 2 });
        let t = task(&mut db);
        let i = input(&db, t.id).unwrap().unwrap();
        assert_eq!(runs(&i).len(), 1);
        if gap <= 60_000 {
            assert_eq!(i.duration_ms, 10000 + gap);
            let out = map(&i, &[utterance(6000 + gap, 7000 + gap, "后半句")]).unwrap();
            assert_eq!((out[0].start_ms, out[0].end_ms), (7000 + gap, 8000 + gap));
        }
    }
}

#[test]
fn silence_requires_complete_matching_source_evidence_and_no_clear_or_capture_gap() {
    for reason in [
        "known",
        "digital",
        "unknown",
        "partial",
        "other_source",
        "cleared",
        "capture",
    ] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        block(&db, "b", 26000, 31000, true);
        if reason != "unknown" {
            block(&db, "s", 6000, 26000, false);
        }
        match reason {
            "digital" => {
                db.execute("UPDATE audio_blocks SET retention_decision='discard_digital_silence_v1',analysis_state='not_needed' WHERE id='s'",[]).unwrap();
            }
            "partial" => {
                db.execute("UPDATE audio_blocks SET ended_ms=25000 WHERE id='s'", [])
                    .unwrap();
            }
            "other_source" => {
                db.execute("UPDATE audio_blocks SET source='system' WHERE id='s'", [])
                    .unwrap();
            }
            "cleared" => {
                db.execute("INSERT INTO clear_ranges(from_ms,to_ms,source_mask,mode,committed_ms) VALUES(10000,11000,1,'raw_audio',32000)",[]).unwrap();
            }
            "capture" => {
                db.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES('gap','microphone',10000,11000,'capture_failure')",[]).unwrap();
            }
            _ => {}
        }
        event(&mut db, 1000, 31000);
        assert_eq!(
            count(&db),
            if matches!(reason, "known" | "digital") {
                1
            } else {
                2
            },
            "{reason}"
        );
    }
}

#[test]
fn one_minute_silence_encodes_full_duration_and_preserves_provider_text() {
    let mut db = database();
    let dir = tempfile::tempdir().unwrap();
    for start in [1000, 66000] {
        let id = Uuid::new_v4();
        let mut encoder =
            crate::media::OpusBlock::create(&dir.path().join("media/microphone"), id).unwrap();
        encoder
            .push(
                &(0..240000)
                    .map(|i| (i as f32 * 0.02).sin() * 0.1)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        encoder.seal().unwrap();
        block(&db, &id.to_string(), start, start + 5000, true);
    }
    block(&db, "silence", 6000, 66000, false);
    event(&mut db, 1000, 71000);
    let t = task(&mut db);
    let frozen = input(&db, t.id).unwrap().unwrap();
    assert_eq!(count(&db), 1);
    assert_eq!(frozen.duration_ms, 70000);
    assert_eq!(frozen.parts[1].offset, 65000);
    let ogg = assemble(dir.path(), &frozen).unwrap();
    // Inspect the long upload incrementally; the production block decoder
    // intentionally accepts only short source blocks, not whole ASR uploads.
    let mut reader = ogg::PacketReader::new(std::io::Cursor::new(ogg));
    let head = reader.read_packet().unwrap().unwrap();
    let skip = u16::from_le_bytes([head.data[10], head.data[11]]) as u64;
    reader.read_packet().unwrap().unwrap();
    let mut decoder = opus2::Decoder::new(48000, opus2::Channels::Mono).unwrap();
    let mut frame = [0.0f32; 5760];
    let mut samples = 0;
    let mut granule = 0;
    while let Some(packet) = reader.read_packet().unwrap() {
        let len = decoder
            .decode_float(&packet.data, &mut frame, false)
            .unwrap();
        if samples > skip + 10 * 48000 && samples + (len as u64) < skip + 60 * 48000 {
            // Opus may add very low-level comfort noise to digital silence.
            let rms = (frame[..len].iter().map(|v| v * v).sum::<f32>() / len as f32).sqrt();
            assert!(rms < 0.001, "silence RMS {rms}");
        }
        samples += len as u64;
        granule = packet.absgp_page();
    }
    assert_eq!(granule - skip, 70000 * 48);
    assert_eq!(
        publish_asr(
            &mut db,
            &t,
            &[
                utterance(60000, 61000, "供应商静默内结果"),
                utterance(65000, 66000, "后段")
            ],
            5000002
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT started_ms FROM precise_segments WHERE text='后段'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        66000
    );
}

#[test]
fn silence_policy_does_not_rewrite_existing_physical_requests() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    block(&db, "b", 26000, 31000, true);
    let id = event(&mut db, 1000, 31000);
    db.execute(
        "UPDATE cloud_tasks SET processor_version='volc-event-contiguous-v2'",
        [],
    )
    .unwrap();
    let snapshot = || {
        db.prepare("SELECT task_id,specification FROM asr_inputs ORDER BY task_id")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let before = snapshot();
    block(&db, "s", 6000, 26000, false);
    let tx = db.transaction().unwrap();
    schedule(
        &tx,
        "event",
        &id.to_string(),
        &asr_version(1, 1000, 31000),
        1000,
        31000,
        1,
        true,
        32000,
        VERSION,
    )
    .unwrap();
    tx.commit().unwrap();
    let after = db
        .prepare("SELECT task_id,specification FROM asr_inputs ORDER BY task_id")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(count(&db), 2);
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
            &[
                utterance(0, 1000, "有效"),
                utterance(29000, 31000, "请求越界")
            ],
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
fn continuous_audio_keeps_twenty_minute_request_limit_without_synthetic_seams() {
    let mut db = database();
    for n in 0..125 {
        let a = 1000 + n * 10000;
        block(&db, &format!("a{n}"), a, a + 10000, true);
    }
    event(&mut db, 1000, 1_251_000);
    assert_eq!(count(&db), 2);
    let mut total = 0;
    for _ in 0..2 {
        let t = task(&mut db);
        let i = input(&db, t.id).unwrap().unwrap();
        assert!(i.duration_ms <= WINDOW_MS);
        assert_eq!(runs(&i).len(), 1);
        total += i.duration_ms;
    }
    assert_eq!(total, 125 * 10000);
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

#[test]
fn persisted_request_clock_owns_text_while_voice_requires_actual_audio() {
    let mut db = database();
    block(&db, "first", 1_000, 6_000, true);
    block(&db, "silent", 6_000, 51_000, false);
    block(&db, "last", 51_000, 56_000, true);
    event(&mut db, 1_000, 56_000);
    let task = task(&mut db);
    db.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES('stop','microphone',55000,NULL,'manual_pause')", []).unwrap();
    let items = [
        utterance(4_000, 51_000, "长停顿完整句子"),
        utterance(49_900, 50_300, "边界"),
        utterance(10_000, 11_000, "供应商静默内结果"),
        utterance(53_000, 54_900, "尾句"),
    ];
    assert!(voice_manifest(&db, "microphone", 54_000, 55_900).is_err());
    assert_eq!(publish_asr(&mut db, &task, &items, 5_000_001).unwrap(), 4);
    assert_eq!(publish_asr(&mut db, &task, &items, 5_000_002).unwrap(), 0);
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM precise_segments WHERE task_id=?1",
            [task.id.to_string()],
            |r| r.get(0)
        )
        .unwrap(),
        4
    );
}

#[test]
fn rejected_legacy_input_is_replanned_without_changing_successful_siblings() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    block(&db, "silence", 6000, 26000, false);
    block(&db, "b", 26000, 31000, true);
    block(&db, "sibling", 100000, 105000, true);
    let event_id = event(&mut db, 1000, 105000);
    let failed = task(&mut db);
    let mut old_input = input(&db, failed.id).unwrap().unwrap();
    old_input.parts[1].offset = 5500;
    old_input.duration_ms = 10500;
    let specification = serde_json::to_string(&old_input).unwrap();
    db.execute(
        "UPDATE asr_inputs SET specification=?2 WHERE task_id=?1",
        params![failed.id.to_string(), specification],
    )
    .unwrap();
    db.execute(
        "UPDATE cloud_tasks SET processor_version='volc-event-compact-v1' WHERE id=?1",
        [failed.id.to_string()],
    )
    .unwrap();
    mark_submit_intent(&db, &failed, 5000001).unwrap();
    task_failed(
        &db,
        &failed,
        "provider_unmapped_timing_word_in_separator",
        5000002,
    )
    .unwrap();
    let sibling = task(&mut db);
    publish_asr(
        &mut db,
        &sibling,
        &[utterance(0, 1000, "有效结果")],
        5000003,
    )
    .unwrap();
    let segment = event_detail(&db, event_id, 0, 10).unwrap().precise_segments[0].id;
    let tx = db.transaction().unwrap();
    crate::knowledge::correct_text(&tx, segment, "人工修订", 5000004).unwrap();
    tx.commit().unwrap();

    retry_task(&db, failed.id, 5000005).unwrap();
    let replacement = take_task(&mut db, CloudProvider::Asr, 5000006)
        .unwrap()
        .unwrap();
    assert_ne!(replacement.id, failed.id);
    assert_eq!((replacement.start, replacement.end), (1000, 31000));
    assert_eq!(replacement.submit_attempts, 0);
    let rebuilt = input(&db, replacement.id).unwrap().unwrap();
    assert_eq!(rebuilt.duration_ms, 30000);
    assert_eq!(rebuilt.parts[1].offset, 25000);
    assert_eq!(runs(&rebuilt).len(), 1);
    let history: (String,String,String,i64) = db.query_row("SELECT t.state,t.remote_id,i.specification,t.submit_attempts FROM cloud_tasks t JOIN asr_inputs i ON i.task_id=t.id WHERE t.id=?1",[failed.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(
        history,
        (
            "superseded".into(),
            failed.remote_id.to_string(),
            specification,
            1
        )
    );
    let detail = event_detail(&db, event_id, 0, 10).unwrap();
    assert_eq!(detail.precise_segments[0].id, segment);
    assert_eq!(detail.precise_segments[0].text, "人工修订");
    assert_eq!(count(&db), 2);
    assert_eq!(
        publish_asr(
            &mut db,
            &failed,
            &[utterance(0, 1000, "迟到旧结果")],
            5000007
        )
        .unwrap(),
        0
    );
    assert!(retry_task(&db, failed.id, 5000008).is_err());
    assert_eq!(count(&db), 2);
}

#[test]
fn rejected_shared_input_uses_current_cropped_memberships_and_rolls_back_if_audio_is_missing() {
    for missing in [false, true] {
        let mut db = database();
        block(&db, "a", 1000, 3500, true);
        block(&db, "b", 3500, 6000, true);
        let original = event(&mut db, 1000, 6000);
        let failed = task(&mut db);
        mark_submit_intent(&db, &failed, 5000001).unwrap();
        let (left, right) = split_event(&mut db, original, 1, 3500, "前", "后", 5000002).unwrap();
        task_failed(&db, &failed, "provider_invalid_utterance", 5000003).unwrap();
        if missing {
            db.execute("UPDATE audio_blocks SET media_path=NULL WHERE id='b'", [])
                .unwrap();
            assert_eq!(
                retry_task(&db, failed.id, 5000004).unwrap_err(),
                "audio_unavailable"
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM cloud_tasks", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                db.query_row(
                    "SELECT state FROM cloud_tasks WHERE id=?1",
                    [failed.id.to_string()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "failed"
            );
        } else {
            retry_task(&db, failed.id, 5000004).unwrap();
            assert_eq!(count(&db), 2);
            for (event_id, expected) in [(left, (1000, 3500)), (right, (3500, 6000))] {
                let span = db
                    .query_row(
                        "SELECT started_ms,ended_ms FROM transcript_tasks WHERE target_id=?1",
                        [event_id.to_string()],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                    )
                    .unwrap();
                assert_eq!(span, expected);
            }
        }
    }
}

#[test]
fn uncertain_manual_retry_keeps_original_request_and_input() {
    let mut db = database();
    block(&db, "a", 1000, 6000, true);
    event(&mut db, 1000, 6000);
    let failed = task(&mut db);
    mark_submit_intent(&db, &failed, 5000001).unwrap();
    for error in [
        "provider_transport",
        "provider_deadline",
        "provider_read_failed",
        "keychain_read_-25308",
    ] {
        db.execute(
            "UPDATE cloud_tasks SET state='failed',error_code=?2 WHERE id=?1",
            params![failed.id.to_string(), error],
        )
        .unwrap();
        retry_task(&db, failed.id, 5000002).unwrap();
        let retried = take_task(&mut db, CloudProvider::Asr, 5000003)
            .unwrap()
            .unwrap();
        assert_eq!(retried.id, failed.id);
        assert_eq!(retried.remote_id, failed.remote_id);
        assert_eq!(retried.submit_attempts, 1);
        assert_eq!(count(&db), 1);
    }
}

#[test]
fn removed_coverage_rejection_retries_and_publishes_original_result_without_new_input() {
    let mut db = database();
    block(&db, "first", 1000, 6000, true);
    block(&db, "silence", 6000, 51000, false);
    block(&db, "last", 51000, 56000, true);
    event(&mut db, 1000, 56000);
    let failed = task(&mut db);
    let original_input = serde_json::to_string(&input(&db, failed.id).unwrap()).unwrap();
    mark_submit_intent(&db, &failed, 5000001).unwrap();
    task_failed(
        &db,
        &failed,
        "provider_unmapped_timing_audio_coverage",
        5000002,
    )
    .unwrap();
    // Already paid text remains recoverable even after raw-audio retention.
    db.execute(
        "UPDATE audio_blocks SET media_path=NULL,retention_decision='raw_audio'",
        [],
    )
    .unwrap();
    retry_task(&db, failed.id, 5000003).unwrap();
    let retried = take_task(&mut db, CloudProvider::Asr, 5000004)
        .unwrap()
        .unwrap();
    assert_eq!(retried.id, failed.id);
    assert_eq!(retried.remote_id, failed.remote_id);
    assert_eq!(retried.submit_attempts, 1);
    assert_eq!(
        serde_json::to_string(&input(&db, retried.id).unwrap()).unwrap(),
        original_input
    );
    let text = [
        utterance(4000, 51000, "长停顿完整句子"),
        utterance(10000, 11000, "供应商静默内结果"),
    ];
    assert_eq!(publish_asr(&mut db, &retried, &text, 5000005).unwrap(), 2);
    assert_eq!(publish_asr(&mut db, &retried, &text, 5000006).unwrap(), 0);
    assert_eq!(
        db.query_row("SELECT count(*) FROM cloud_tasks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn rejected_manual_range_replans_once_and_does_not_change_the_range_identity() {
    for error in [
        "provider_unmapped_timing",
        "provider_unmapped_timing_word_in_separator",
        "provider_invalid_utterance",
        "provider_invalid_timing",
        "provider_missing_utterances",
    ] {
        let mut db = database();
        block(&db, "a", 1000, 6000, true);
        let range = transcribe_range(&mut db, 1000, 6000, EventSources::Microphone, 7000).unwrap();
        let failed = task(&mut db);
        mark_submit_intent(&db, &failed, 5000001).unwrap();
        task_failed(&db, &failed, error, 5000002).unwrap();
        retry_task(&db, failed.id, 5000003).unwrap();
        let new = take_task(&mut db, CloudProvider::Asr, 5000004)
            .unwrap()
            .unwrap();
        assert_ne!(new.id, failed.id);
        assert_eq!(new.target_id, range.to_string());
        assert_eq!(new.target_version, "1");
        assert_eq!(new.submit_attempts, 0);
        assert_eq!(
            transcribe_range(&mut db, 1000, 6000, EventSources::Microphone, 5000005).unwrap(),
            range
        );
        assert_eq!(count(&db), 1);
    }
}

#[test]
fn shared_recovery_skips_already_covered_member_without_blocking_the_other() {
    let mut db = database();
    block(&db, "a", 1000, 10000, true);
    let original = event(&mut db, 1000, 10000);
    let failed = task(&mut db);
    mark_submit_intent(&db, &failed, 5000001).unwrap();
    task_failed(
        &db,
        &failed,
        "provider_unmapped_timing_word_in_separator",
        5000002,
    )
    .unwrap();
    let (left, right) = split_event(&mut db, original, 1, 5000, "前", "后", 5000003).unwrap();
    edit_event(&mut db, right, 1, 5000, 7000, "后", None, 5000004).unwrap();
    let sibling_event = event(&mut db, 7000, 10000);
    let sibling = task(&mut db);
    publish_asr(
        &mut db,
        &sibling,
        &[utterance(0, 1000, "成功范围")],
        5000005,
    )
    .unwrap();
    let merged = merge_events(&mut db, right, 2, sibling_event, 1, "合并", 5000006).unwrap();
    edit_event(&mut db, merged, 1, 7000, 10000, "已覆盖", None, 5000007).unwrap();

    retry_task(&db, failed.id, 5000008).unwrap();
    assert_eq!(count(&db), 2);
    let recovered = take_task(&mut db, CloudProvider::Asr, 5000009)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.target_id, left.to_string());
    assert_eq!((recovered.start, recovered.end), (1000, 5000));
    let current_sibling: String = db
        .query_row(
            "SELECT id FROM transcript_tasks WHERE target_id=?1",
            [merged.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current_sibling, sibling.id.to_string());
    assert_eq!(
        event_detail(&db, merged, 0, 10).unwrap().precise_segments[0].text,
        "成功范围"
    );
}
