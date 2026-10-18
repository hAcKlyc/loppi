use super::super::tests::speaker_merge_fixture as fixture;
use super::*;
use crate::{cleanup, events};
use hear_protocol::{CleanupMode, EventSources};

fn publish_voice(
    db: &mut Connection,
    work: &VoiceWork,
    vector: &[f32],
    quality: Option<VoiceQuality>,
    now: i64,
) -> Result<(), String> {
    crate::knowledge::publish_voice(db, work, vector, quality, now)?;
    drain_enrollment(db, now)
}

fn person(db: &mut Connection) -> Uuid {
    let tx = db.transaction().unwrap();
    let p = create_person(&tx, "人工确认", 70000).unwrap();
    tx.commit().unwrap();
    p
}
fn bind(db: &mut Connection, id: Uuid, p: Uuid, entire: bool) {
    let tx = db.transaction().unwrap();
    bind_speaker(&tx, id, p, entire, 71000).unwrap();
    tx.commit().unwrap();
    drain_enrollment(db, 71001).unwrap();
}
fn vector(n: usize) -> Vec<f32> {
    let mut v = vec![0.; VOICE_DIM];
    v[0] = 0.9;
    v[n + 1] = (1.0_f32 - 0.81).sqrt();
    v
}
fn q(ms: i64) -> Option<VoiceQuality> {
    Some(VoiceQuality {
        voiced_ms: ms,
        score: ms.min(10000),
    })
}
fn count(db: &Connection, p: Uuid) -> usize {
    voice_samples(db, p).unwrap().len()
}
fn publish_next(db: &mut Connection, n: usize, ms: i64) -> VoiceWork {
    let work = take_voice(db, 72000).unwrap().unwrap();
    publish_voice(db, &work, &vector(n), q(ms), 72001).unwrap();
    work
}

#[test]
fn combines_only_confirmed_short_speech_and_tracks_every_member() {
    let (mut db, _, s) = fixture(&[
        ("A", 0, 1800),
        ("A", 2000, 3800),
        ("A", 4000, 5800),
        ("A", 6000, 7800),
    ]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    assert_eq!(work.audio_ranges().len(), 4);
    assert_eq!(
        work.audio_ranges().iter().map(|(a, b)| b - a).sum::<i64>(),
        7200
    );
    publish_voice(&mut db, &work, &vector(0), q(7200), 72001).unwrap();
    assert_eq!(count(&db, p), 1);
    let samples = voice_samples(&db, p).unwrap();
    assert_eq!(samples[0].parts.len(), 4);
    assert_eq!(
        samples[0]
            .parts
            .iter()
            .map(|part| (part.started_ms, part.ended_ms))
            .collect::<Vec<_>>(),
        work.audio_ranges()
    );
    assert_eq!(samples[0].audio_available, Some(true));
    assert_eq!(
        db.query_row("SELECT count(*) FROM voice_sample_parts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        4
    );
    let tx = db.transaction().unwrap();
    unbind_speaker(&tx, s[2].id, false, 73000).unwrap();
    tx.commit().unwrap();
    assert_eq!(count(&db, p), 0);
    publish_voice(&mut db, &work, &vector(0), q(7200), 74000).unwrap();
    assert_eq!(
        count(&db, p),
        0,
        "late/replayed job cannot restore a revoked composite"
    );
}

#[test]
fn sample_playability_tracks_retained_audio_without_deleting_voice_identity() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    publish_next(&mut db, 0, 5400);
    let before = voice_samples(&db, p).unwrap();
    assert_eq!(before[0].audio_available, Some(true));
    db.execute("UPDATE audio_blocks SET media_path=NULL", [])
        .unwrap();
    let after = voice_samples(&db, p).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].parts.len(), 3);
    assert_eq!(after[0].audio_available, Some(false));
    assert_eq!(after[0].revision, before[0].revision);
}

#[test]
fn single_sentence_confirmation_does_not_borrow_neighbors() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, false);
    assert!(take_voice(&mut db, 72000).unwrap().is_none());
    assert_eq!(count(&db, p), 0);
}

#[test]
fn pauses_interruptions_overlap_and_different_sources_break_combinations() {
    for entries in [
        vec![("A", 0, 1800), ("A", 4000, 5800)],
        vec![("A", 0, 1800), ("B", 1900, 2100), ("A", 2200, 4000)],
        vec![("A", 0, 1800), ("B", 1700, 1900), ("A", 2000, 3800)],
    ] {
        let (mut db, _, s) = fixture(&entries);
        let p = person(&mut db);
        bind(&mut db, s[0].id, p, true);
        assert_eq!(
            db.query_row("SELECT count(*) FROM voice_enrollments", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800)]);
    db.execute(
        "UPDATE precise_segments SET source='system' WHERE id=?1",
        [s[1].id.to_string()],
    )
    .unwrap();
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    assert!(take_voice(&mut db, 72000).unwrap().is_none());
}

#[test]
fn quality_requires_enough_active_audio_and_rejects_clipping_and_silence() {
    assert!(enrollment_quality(&vec![0.1; 16000 * 2]).is_none());
    assert!(enrollment_quality(&vec![0.001; 16000 * 6]).is_none());
    assert!(enrollment_quality(&vec![1.; 16000 * 6]).is_none());
    let mut mostly_silent = vec![0.; 16000 * 6];
    mostly_silent[..16000 * 3].fill(0.1);
    assert!(enrollment_quality(&mostly_silent).is_none());
    let good = enrollment_quality(&vec![0.1; 16000 * 6]).unwrap();
    assert_eq!(good.voiced_ms, 6000);
    assert!(good.score > enrollment_quality(&vec![0.1; 16000 * 3]).unwrap().score);
    assert!(enrollment_quality(&[f32::NAN; 320]).is_none());
}

#[test]
fn automatic_names_never_enroll_but_manual_existing_person_does() {
    let (mut db, _, s) = fixture(&[("A", 0, 4000), ("B", 6000, 10000), ("C", 12000, 16000)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, false);
    publish_next(&mut db, 0, 4000);
    publish_next(&mut db, 1, 4000);
    while match_one_ready(&mut db, 73000).unwrap() {}
    assert_eq!(
        count(&db, p),
        1,
        "automatic recognition does not create training data"
    );
    bind(&mut db, s[1].id, p, false);
    assert_eq!(
        count(&db, p),
        2,
        "explicitly confirming existing person adds diverse quality sample"
    );
    publish_next(&mut db, 0, 4000);
    bind(&mut db, s[2].id, p, false);
    assert_eq!(
        count(&db, p),
        2,
        "identical embedding is not a useful third sample"
    );
}

#[test]
fn better_duplicate_replaces_lower_quality_and_scene_cap_keeps_best() {
    let (mut db, _, s) = fixture(&[
        ("A", 0, 4000),
        ("A", 5000, 11000),
        ("A", 12000, 18000),
        ("A", 19000, 25000),
        ("A", 26000, 34000),
    ]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    publish_next(&mut db, 0, 4000);
    publish_next(&mut db, 0, 6000);
    assert_eq!(count(&db, p), 1);
    publish_next(&mut db, 1, 6000);
    publish_next(&mut db, 2, 6000);
    publish_next(&mut db, 3, 8000);
    assert_eq!(count(&db, p), 3);
    assert!(
        voice_samples(&db, p)
            .unwrap()
            .iter()
            .any(|v| v.started_ms == 27000)
    );
}

#[test]
fn inconsistent_voice_and_other_person_conflict_cannot_poison_library() {
    let (mut db, _, s) = fixture(&[
        ("A", 0, 4000),
        ("B", 5000, 9000),
        ("C", 10000, 14000),
        ("D", 15000, 19000),
    ]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, false);
    publish_next(&mut db, 0, 4000);
    let other = person(&mut db);
    bind(&mut db, s[1].id, other, false);
    publish_next(&mut db, 0, 4000);
    assert_eq!(
        count(&db, other),
        0,
        "same voice conflicts with a different named identity"
    );
    bind(&mut db, s[2].id, p, false);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    let mut foreign = vec![0.; VOICE_DIM];
    foreign[100] = 1.;
    publish_voice(&mut db, &work, &foreign, q(4000), 72001).unwrap();
    assert_eq!(
        count(&db, p),
        1,
        "inconsistent samples are skipped, name remains confirmed"
    );
    assert_eq!(
        db.query_row(
            "SELECT person_id FROM speaker_bindings WHERE current_segment_id=?1 AND state='active'",
            [s[2].id.to_string()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        p.to_string()
    );
}

#[test]
fn late_composite_rechecks_binding_retranscription_deletion_and_restart() {
    for mutation in ["unbind", "rebind", "retranscribe", "delete"] {
        let (mut db, event, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
        let p = person(&mut db);
        bind(&mut db, s[0].id, p, true);
        let work = take_voice(&mut db, 72000).unwrap().unwrap();
        recover_voice(&db).unwrap();
        let resumed = take_voice(&mut db, 73000).unwrap().unwrap();
        assert_eq!(work.segment_id, resumed.segment_id);
        let tx = db.transaction().unwrap();
        match mutation {
            "unbind" => unbind_speaker(&tx, s[1].id, false, 74000).unwrap(),
            "rebind" => {
                let other = create_person(&tx, "纠正", 74000).unwrap();
                bind_speaker(&tx, s[1].id, other, false, 74000).unwrap();
            }
            "retranscribe" => pending_event_corrections(&tx, event, 74000).unwrap(),
            _ => delete_person(&tx, p).unwrap(),
        }
        tx.commit().unwrap();
        publish_voice(&mut db, &resumed, &vector(0), q(5400), 75000).unwrap();
        assert_eq!(count(&db, p), 0, "{mutation}");
    }
}

#[test]
fn raw_cleanup_preserves_samples_but_full_cleanup_removes_composite_evidence() {
    for mode in [CleanupMode::RawAudio, CleanupMode::AllData] {
        let (mut db, _, s) = fixture(&[
            ("A", 0, 1800),
            ("A", 2000, 3800),
            ("A", 4000, 5800),
            ("A", 9000, 10800),
            ("A", 11000, 12800),
        ]);
        let p = person(&mut db);
        bind(&mut db, s[0].id, p, true);
        publish_next(&mut db, 0, 5400);
        assert_eq!(count(&db, p), 1);
        // No physical files are touched in this isolated logical-cleanup regression.
        let preview =
            cleanup::preview(&db, 3100, 3200, EventSources::Microphone, mode, 73000).unwrap();
        let scope = cleanup::verified_scope(&db, preview.token, true, 73000).unwrap();
        let prepared = cleanup::PreparedMedia {
            originals: vec![],
            replacements: vec![],
        };
        let tx = db.transaction().unwrap();
        cleanup::apply(&tx, &scope, &prepared, 73001).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            count(&db, p),
            if mode == CleanupMode::RawAudio { 1 } else { 0 }
        );
    }
}

#[test]
fn user_removed_composite_is_not_recreated_by_replay() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = publish_next(&mut db, 0, 5400);
    let sample = voice_samples(&db, p).unwrap()[0].id;
    let tx = db.transaction().unwrap();
    remove_voice_sample(&tx, sample, 73000).unwrap();
    tx.commit().unwrap();
    publish_voice(&mut db, &work, &vector(0), q(5400), 74000).unwrap();
    assert_eq!(count(&db, p), 0);
}

fn add_scene(db: &mut Connection, start: i64) -> Vec<hear_protocol::PreciseSegment> {
    let block = Uuid::new_v4().to_string();
    db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
      VALUES(?1,'test','microphone',?2,?3,0,2880000,100,'media/microphone/test.opus','retain','complete',1)",params![block,start,start+60000]).unwrap();
    let event = events::create_event(
        db,
        start,
        start + 60000,
        "补充场次",
        EventSources::Microphone,
        61000,
    )
    .unwrap();
    let task = events::take_task(db, hear_protocol::CloudProvider::Asr, 62000)
        .unwrap()
        .unwrap();
    let utterances = (0..3)
        .map(|i| crate::cloud::AsrUtterance {
            words: None,
            start_ms: i * 5000,
            end_ms: i * 5000 + 4000,
            text: "合格的人工样例".into(),
            speaker: Some("A".into()),
        })
        .collect::<Vec<_>>();
    events::publish_asr(db, &task, &utterances, 62001).unwrap();
    events::event_detail(db, event, 0, 20)
        .unwrap()
        .precise_segments
}

#[test]
fn total_sample_count_is_bounded_across_scenes_and_model_scoped() {
    let (mut db, _, s) = fixture(&[("A", 0, 4000), ("A", 5000, 9000), ("A", 10000, 14000)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    for n in 0..3 {
        publish_next(&mut db, n, 4000);
    }
    for scene in 1..10 {
        let s = add_scene(&mut db, 100000 * scene);
        bind(&mut db, s[0].id, p, true);
        for n in 0..3 {
            publish_next(&mut db, (scene * 3 + n) as usize, 4000);
        }
    }
    assert_eq!(count(&db, p), 24);
    let largest:i64=db.query_row("SELECT max(n) FROM(SELECT count(*) n FROM voice_samples s JOIN voice_sample_quality q ON q.sample_id=s.id WHERE revoked_ms IS NULL GROUP BY person_id,model_version,source,bucket)",[],|r|r.get(0)).unwrap();
    assert_eq!(largest, 3);
}

#[test]
fn legacy_library_migration_is_atomic_idempotent_and_deduplicates() {
    let (mut db, _, s) = fixture(&[("A", 0, 4000), ("A", 5000, 11000), ("A", 12000, 16000)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    for (i, segment) in s.iter().enumerate() {
        db.execute(
            "INSERT INTO voice_samples VALUES(?1,?2,?3,'microphone',?4,?5,?6,?7,70000,NULL)",
            params![
                Uuid::new_v4().to_string(),
                p.to_string(),
                segment.id.to_string(),
                segment.started_ms,
                segment.ended_ms,
                encode_vector(&vector(0)).unwrap(),
                VOICE_MODEL_VERSION
            ],
        )
        .unwrap();
        if i == 2 {
            db.execute(
                "UPDATE speaker_bindings SET origin='automatic' WHERE current_segment_id=?1",
                [segment.id.to_string()],
            )
            .unwrap();
        }
    }
    db.execute(
        "DELETE FROM core_migrations WHERE name='voice_enrollment_v1'",
        [],
    )
    .unwrap();
    init(&db).unwrap();
    assert_eq!(count(&db, p), 1);
    assert_eq!(
        voice_samples(&db, p).unwrap()[0].started_ms,
        s[1].started_ms
    );
    let generation: i64 = db
        .query_row("SELECT generation FROM identity_state", [], |r| r.get(0))
        .unwrap();
    init(&db).unwrap();
    assert_eq!(
        generation,
        db.query_row("SELECT generation FROM identity_state", [], |r| r
            .get::<_, i64>(0))
            .unwrap()
    );
}

#[test]
fn pending_composite_is_cancelled_by_raw_cleanup_and_not_resurrected() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    let preview = cleanup::preview(
        &db,
        3100,
        3200,
        EventSources::Microphone,
        CleanupMode::RawAudio,
        73000,
    )
    .unwrap();
    assert_eq!(preview.voice_extractions_awaiting_audio, 1);
    let scope = cleanup::verified_scope(&db, preview.token, false, 73000).unwrap();
    let tx = db.transaction().unwrap();
    cleanup::apply(
        &tx,
        &scope,
        &cleanup::PreparedMedia {
            originals: vec![],
            replacements: vec![],
        },
        73001,
    )
    .unwrap();
    tx.commit().unwrap();
    publish_voice(&mut db, &work, &vector(0), q(5400), 74000).unwrap();
    assert_eq!(count(&db, p), 0);
}

#[test]
fn composite_retries_are_durable_and_bounded() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    fail_voice(&db, work.segment_id, "resource_deferred", 72001).unwrap();
    assert_eq!(
        db.query_row("SELECT attempts FROM voice_enrollments", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let mut when = 100000;
    for _ in 0..4 {
        let work = take_voice(&mut db, when).unwrap().unwrap();
        fail_voice(&db, work.segment_id, "worker_timeout", when).unwrap();
        when += 400000;
    }
    assert!(take_voice(&mut db, when).unwrap().is_none());
    assert_eq!(
        db.query_row("SELECT state FROM voice_enrollments", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "unavailable"
    );
}

fn composite_pcm(
    db: &Connection,
    work: &VoiceWork,
    input: &[f32],
    directory: &std::path::Path,
) -> Vec<f32> {
    let id = Uuid::new_v4();
    let path = directory.join("media/microphone");
    let mut block = crate::media::OpusBlock::create(&path, id).unwrap();
    let mut audio = vec![0.; 48000 * 8];
    let selected = input.len().min(audio.len());
    audio[..selected].copy_from_slice(&input[..selected]);
    block.push(&audio).unwrap();
    block.seal().unwrap();
    db.execute(
        "UPDATE audio_blocks SET media_path=?1,frames=?2,ended_ms=9000 WHERE id='speaker-test'",
        params![format!("media/microphone/{id}.opus"), audio.len() as i64],
    )
    .unwrap();
    let manifests = work
        .audio_ranges()
        .iter()
        .map(|(a, b)| events::voice_manifest(db, &work.source, *a, *b).unwrap())
        .collect::<Vec<_>>();
    events::voice_input_16k(directory, &manifests, true).unwrap()
}

#[test]
fn real_opus_assembly_excludes_gaps_and_preserves_selected_sample_duration() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = (0..48000 * 6)
        .map(|n| 0.2 * (n as f32 * 220.0 * std::f32::consts::TAU / 48000.0).sin())
        .collect::<Vec<_>>();
    let pcm = composite_pcm(&db, &work, &input, dir.path());
    assert_eq!(pcm.len(), 16000 * 5400 / 1000);
    assert!(enrollment_quality(&pcm).is_some());
}

#[test]
#[ignore = "explicit local model smoke: HEAR_ENROLLMENT_PCM (48k mono f32le synthetic speech), HEAR_ENROLLMENT_MODEL and HEAR_ENROLLMENT_WORKER"]
fn local_model_composite_enrollment_smoke() {
    let input = std::fs::read(std::env::var("HEAR_ENROLLMENT_PCM").unwrap()).unwrap();
    let pcm = input
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect::<Vec<_>>();
    let (mut db, _, s) = fixture(&[
        ("A", 0, 1800),
        ("A", 2000, 3800),
        ("A", 4000, 5800),
        ("A", 6000, 7800),
    ]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let audio = composite_pcm(&db, &work, &pcm, dir.path());
    let quality = enrollment_quality(&audio).expect("synthetic speech passes enrollment gate");
    let mut worker = crate::analysis::AnalysisClient::start(
        std::path::Path::new(&std::env::var("HEAR_ENROLLMENT_WORKER").unwrap()),
        "embed",
        std::path::Path::new(&std::env::var("HEAR_ENROLLMENT_MODEL").unwrap()),
    )
    .unwrap();
    let vector: Vec<f32> = worker
        .request(
            "microphone",
            "voice",
            0,
            &audio,
            true,
            std::time::Duration::from_secs(30),
            &|| false,
        )
        .unwrap();
    assert_eq!(vector.len(), VOICE_DIM);
    publish_voice(&mut db, &work, &vector, Some(quality), 72001).unwrap();
    assert_eq!(count(&db, p), 1);
    eprintln!(
        "local PCM -> Opus -> selected short ranges -> 16k -> CampPlus -> human-evidence enrollment PASS; {} voiced ms",
        quality.voiced_ms
    );
}

#[test]
fn unlabeled_short_utterance_preserves_new_and_existing_person_confirmation() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800)]);
    db.execute(
        "UPDATE precise_segments SET speaker_label=NULL WHERE id=?1",
        [s[0].id.to_string()],
    )
    .unwrap();
    let tx = db.transaction().unwrap();
    let p = create_person(&tx, "无标签也保留姓名", 70000).unwrap();
    bind_speaker(&tx, s[0].id, p, false, 70001).unwrap();
    tx.commit().unwrap();
    bind(&mut db, s[0].id, p, false);
    assert_eq!(people(&db).unwrap()[0].id, p);
    assert_eq!(count(&db, p), 0);
    assert_eq!(
        db.query_row(
            "SELECT person_id FROM speaker_bindings WHERE current_segment_id=?1 AND state='active'",
            [s[0].id.to_string()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        p.to_string()
    );
    assert!(take_voice(&mut db, 72000).unwrap().is_none());
}

#[test]
fn late_composite_rejects_group_split_when_cleanup_removes_external_acoustic_support() {
    let (mut db, event, s) = fixture(&[
        ("A", 0, 1800),
        ("B", 2000, 3800),
        ("A", 10000, 14000),
        ("B", 15000, 19000),
        ("A", 20000, 24000),
        ("B", 25000, 29000),
    ]);
    for _ in 0..4 {
        publish_next(&mut db, 0, 4000);
    }
    let detail = events::event_detail(&db, event, 0, 20).unwrap();
    assert_eq!(
        detail.precise_segments[0].speaker,
        detail.precise_segments[1].speaker
    );
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    assert_eq!(work.audio_ranges().len(), 2);
    let preview = cleanup::preview(
        &db,
        11000,
        30000,
        EventSources::Microphone,
        CleanupMode::AllData,
        73000,
    )
    .unwrap();
    let scope = cleanup::verified_scope(&db, preview.token, true, 73000).unwrap();
    let tx = db.transaction().unwrap();
    cleanup::apply(
        &tx,
        &scope,
        &cleanup::PreparedMedia {
            originals: vec![],
            replacements: vec![],
        },
        73001,
    )
    .unwrap();
    tx.commit().unwrap();
    let detail = events::event_detail(&db, event, 0, 20).unwrap();
    assert_ne!(
        detail.precise_segments[0].speaker,
        detail.precise_segments[1].speaker
    );
    publish_voice(&mut db, &work, &vector(0), q(3600), 74000).unwrap();
    assert_eq!(count(&db, p), 0);
}

#[test]
fn candidate_scope_never_crosses_group_target_or_version() {
    for boundary in ["group", "target", "version"] {
        let (mut db, _, s) = fixture(&[("A", 0, 1800), ("B", 2000, 3800)]);
        let p = person(&mut db);
        bind(&mut db, s[0].id, p, false);
        bind(&mut db, s[1].id, p, false);
        if boundary != "group" {
            // Explicit stable local group makes the target/version check decisive.
            for segment in &s {
                db.execute(
                    "INSERT INTO local_speaker_assignments VALUES(?1,'test-shared',70000)",
                    [segment.id.to_string()],
                )
                .unwrap();
            }
            let task: String = db
                .query_row(
                    "SELECT task_id FROM precise_segments WHERE id=?1",
                    [s[1].id.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
            // Clone a completed cloud task using its current schema, then change only its target.
            db.execute_batch("CREATE TEMP TABLE task_copy AS SELECT * FROM cloud_tasks WHERE 0;")
                .unwrap();
            db.execute(
                "INSERT INTO task_copy SELECT * FROM cloud_tasks WHERE id=?1",
                [task],
            )
            .unwrap();
            let new_task = Uuid::new_v4().to_string();
            db.execute("UPDATE task_copy SET id=?1", [&new_task])
                .unwrap();
            if boundary == "target" {
                let existing: String = db
                    .query_row("SELECT target_id FROM task_copy", [], |r| r.get(0))
                    .unwrap();
                db.execute_batch(
                    "CREATE TEMP TABLE event_copy AS SELECT * FROM named_events WHERE 0;",
                )
                .unwrap();
                db.execute(
                    "INSERT INTO event_copy SELECT * FROM named_events WHERE id=?1",
                    [existing],
                )
                .unwrap();
                let event = Uuid::new_v4().to_string();
                db.execute("UPDATE event_copy SET id=?1", [&event]).unwrap();
                db.execute_batch("INSERT INTO named_events SELECT * FROM event_copy;")
                    .unwrap();
                db.execute("UPDATE task_copy SET target_id=?1", [event])
                    .unwrap();
            } else {
                // A historical version is not eligible even if labels and binding are identical.
                db.execute(
                    "UPDATE task_copy SET target_version='historical-version'",
                    [],
                )
                .unwrap();
            }
            db.execute_batch("INSERT INTO cloud_tasks SELECT * FROM task_copy;")
                .unwrap();
            db.execute(
                "UPDATE precise_segments SET task_id=?1 WHERE id=?2",
                params![new_task, s[1].id.to_string()],
            )
            .unwrap();
        }
        let tx = db.transaction().unwrap();
        queue_short_samples(&tx, &[s[0].id, s[1].id], p, 71000).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM voice_enrollments", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "{boundary}"
        );
    }
}

#[test]
fn stale_global_screening_cannot_admit_after_library_changes() {
    let (mut db, _, s) = fixture(&[("A", 0, 4000), ("A", 5000, 9000)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    publish_next(&mut db, 0, 4000);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    crate::knowledge::publish_voice(&mut db, &work, &vector(1), q(4000), 72001).unwrap();
    let input = prepare_enrollment(&db).unwrap().unwrap();
    let allowed = VoiceMatcher::default()
        .allows_enrollment(&input.matching, &input.person, |after| {
            match_sample_page(&db, &input.matching, after)
        })
        .unwrap();
    let sample = voice_samples(&db, p).unwrap()[0].id;
    let tx = db.transaction().unwrap();
    remove_voice_sample(&tx, sample, 73000).unwrap();
    tx.commit().unwrap();
    assert!(!publish_enrollment(&mut db, &input, allowed, 74000).unwrap());
    assert_eq!(count(&db, p), 0);
    drain_enrollment(&mut db, 75000).unwrap();
    assert_eq!(
        count(&db, p),
        1,
        "new snapshot can screen the still-confirmed different sentence"
    );
}

#[test]
fn enrollment_global_scoring_uses_bounded_pages_outside_store_lock() {
    let (mut db, _, s) = fixture(&[("A", 0, 4000)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, false);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    crate::knowledge::publish_voice(&mut db, &work, &vector(0), q(4000), 72001).unwrap();
    let mut different = vec![0.; VOICE_DIM];
    different[100] = 1.;
    for n in 0..300 {
        let other = Uuid::new_v4().to_string();
        db.execute(
            "INSERT INTO people VALUES(?1,'Synthetic person',1,1)",
            [&other],
        )
        .unwrap();
        db.execute(
            "INSERT INTO voice_samples VALUES(?1,?2,'unrelated','system',0,4000,?3,?4,1,NULL)",
            params![
                format!("outside-{n:04}"),
                other,
                encode_vector(&different).unwrap(),
                VOICE_MODEL_VERSION
            ],
        )
        .unwrap();
    }
    let store = std::sync::Mutex::new(db);
    let input = prepare_enrollment(&store.lock().unwrap()).unwrap().unwrap();
    let mut reads = 0;
    let allowed = VoiceMatcher::default()
        .allows_enrollment(&input.matching, &input.person, |after| {
            // The same owner mutex is available for each bounded page, never held by scoring.
            let guard = store
                .try_lock()
                .expect("global scoring must not hold Store");
            let page = match_sample_page(&guard, &input.matching, after)?;
            assert!(page.len() <= 256);
            reads += 1;
            Ok(page)
        })
        .unwrap();
    assert_eq!(reads, 2);
    assert!(allowed);
    assert!(publish_enrollment(&mut store.lock().unwrap(), &input, allowed, 73000).unwrap());
    assert_eq!(count(&store.lock().unwrap(), p), 1);
}

#[test]
fn extracted_candidate_survives_restart_and_raw_cleanup_without_requiring_model() {
    let (mut db, _, s) = fixture(&[("A", 0, 1800), ("A", 2000, 3800), ("A", 4000, 5800)]);
    let p = person(&mut db);
    bind(&mut db, s[0].id, p, true);
    let work = take_voice(&mut db, 72000).unwrap().unwrap();
    crate::knowledge::publish_voice(&mut db, &work, &vector(0), q(5400), 72001).unwrap();
    assert_eq!(
        count(&db, p),
        0,
        "identity is saved before background library screening"
    );
    let preview = cleanup::preview(
        &db,
        3100,
        3200,
        EventSources::Microphone,
        CleanupMode::RawAudio,
        73000,
    )
    .unwrap();
    assert_eq!(preview.voice_extractions_awaiting_audio, 0);
    let scope = cleanup::verified_scope(&db, preview.token, false, 73000).unwrap();
    let tx = db.transaction().unwrap();
    cleanup::apply(
        &tx,
        &scope,
        &cleanup::PreparedMedia {
            originals: vec![],
            replacements: vec![],
        },
        73001,
    )
    .unwrap();
    tx.commit().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hear.sqlite");
    db.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    drop(db);
    let mut reopened = crate::Store::new(Connection::open(path).unwrap())
        .unwrap()
        .db;
    recover_voice(&reopened).unwrap();
    drain_enrollment(&mut reopened, 74000).unwrap();
    assert_eq!(count(&reopened, p), 1);
}
