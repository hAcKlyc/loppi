//! Activity granularity regressions. Live model checks use only synthetic speech
//! and an in-memory event database, never the user's transcripts or event writes.
use super::*;
use serde_json::{Value, json};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;

fn db() -> Connection {
    let db = crate::Store::new(Connection::open_in_memory().unwrap())
        .unwrap()
        .db;
    set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
    db
}

fn speech(db: &Connection, start: i64, end: i64, source: &str, text: &str) {
    db.execute(
        "INSERT INTO coarse_segments VALUES (?1,'block','session',?2,?3,?4,?5,'test')",
        params![Uuid::new_v4().to_string(), source, start, end, text],
    )
    .unwrap();
}

fn publish_one(db: &mut Connection, task: &CloudTask, merge: Option<&str>, now: i64) -> String {
    let input = llm_input(db, task).unwrap();
    publish_fit_with_input(db, task, &input, json!({"events":[{
        "start_ms":task.start,"end_ms":task.end,"title":"产品评审会","summary":"讨论产品方案和排期。",
        "sources":"microphone","merge_with_event_id":merge}], "unorganized":
        serde_json::from_str::<Value>(&input).unwrap()["observations"].as_array().unwrap().iter().enumerate().filter(|(_, v)| v["source"] == "system").map(|(i,_)| i).collect::<Vec<_>>() }), now).unwrap();
    db.query_row(
        "SELECT event_id FROM event_candidate_links WHERE candidate_id=?1 AND slot=0",
        [&task.target_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn local_job(db: &Connection, id: &str, start: i64, end: i64, state: &str) {
    db.execute("INSERT INTO coarse_jobs(id,block_id,session_id,source,start_sample,end_sample,started_ms,ended_ms,state)
        VALUES(?1,'block',?1,'microphone',0,16000,?2,?3,?4)",
        params![id,start,end,state]).unwrap();
}

#[test]
fn automatic_fit_coalesces_local_progress_and_failed_text_does_not_block() {
    let mut db = db();
    speech(&db, 0, 1000, "microphone", "开会");
    speech(&db, 5 * MINUTE, 5 * MINUTE + 1000, "microphone", "结束");
    local_job(&db, "late-a", 1000, 2000, "pending");
    local_job(&db, "late-b", 2000, 3000, "processing");
    assert_eq!(refresh_candidates(&mut db, HOUR, false).unwrap(), 0);
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM candidate_windows WHERE state='pending'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );
    assert!(
        take_task(&mut db, CloudProvider::Llm, HOUR)
            .unwrap()
            .is_none()
    );
    speech(&db, 1000, 2000, "microphone", "补齐第一段");
    db.execute(
        "UPDATE coarse_jobs SET state='complete' WHERE id='late-a'",
        [],
    )
    .unwrap();
    assert_eq!(
        refresh_candidates(&mut db, HOUR + 15_000, false).unwrap(),
        0
    );
    db.execute(
        "UPDATE coarse_jobs SET state='failed' WHERE id='late-b'",
        [],
    )
    .unwrap();
    assert_eq!(
        refresh_candidates(&mut db, HOUR + 30_000, false).unwrap(),
        1
    );
    let task = take_task(&mut db, CloudProvider::Llm, HOUR + 30_000)
        .unwrap()
        .unwrap();
    assert!(llm_input(&db, &task).unwrap().contains("补齐第一段"));
    publish_one(&mut db, &task, None, HOUR + 30_001);
    assert_eq!(
        refresh_candidates(&mut db, HOUR + 45_000, false).unwrap(),
        0
    );
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM cloud_tasks WHERE provider='llm'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn aged_deferred_fit_remains_discoverable_after_local_work_or_explicit_force() {
    for outcome in ["failed", "complete", "force"] {
        let mut db = db();
        speech(&db, 0, 1000, "microphone", "讨论");
        local_job(&db, "late", 0, 1000, "pending");
        assert_eq!(refresh_candidates(&mut db, HOUR, false).unwrap(), 0);
        let id: String = db
            .query_row("SELECT id FROM candidate_windows", [], |r| r.get(0))
            .unwrap();
        assert_eq!(refresh_candidates(&mut db, 4 * HOUR, false).unwrap(), 0);
        if outcome != "force" {
            // Neither terminal failure nor an empty result adds new evidence.
            db.execute("UPDATE coarse_jobs SET state=?1", [outcome])
                .unwrap();
        }
        let now = 4 * HOUR + 15_000;
        assert_eq!(
            refresh_candidates(&mut db, now, outcome == "force").unwrap(),
            1,
            "{outcome}"
        );
        let task = take_task(&mut db, CloudProvider::Llm, now)
            .unwrap()
            .unwrap();
        assert_eq!(task.target_id, id);
        publish_one(&mut db, &task, None, now + 1);
        assert_eq!(refresh_candidates(&mut db, now + 15_000, false).unwrap(), 0);
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
                .unwrap(),
            1
        );
    }
}

#[test]
fn newly_pending_local_work_holds_queued_fit_but_explicit_force_can_run() {
    let mut db = db();
    speech(&db, 0, 1000, "microphone", "讨论");
    assert_eq!(refresh_candidates(&mut db, HOUR, false).unwrap(), 1);
    local_job(&db, "late", 0, 1000, "pending");
    assert!(
        take_task(&mut db, CloudProvider::Llm, HOUR)
            .unwrap()
            .is_none()
    );
    assert_eq!(refresh_candidates(&mut db, HOUR, true).unwrap(), 0);
    assert!(
        take_task(&mut db, CloudProvider::Llm, HOUR)
            .unwrap()
            .is_some()
    );
}

#[test]
fn unanalysed_tail_without_any_text_delays_fit_but_terminal_failure_does_not() {
    let mut db = db();
    speech(&db, 0, 1000, "microphone", "已识别的开头");
    db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state)
        VALUES('tail','session','microphone',600000,605000,0,240000,100,'media/microphone/tail.opus','pending_vad_v2','pending')",[]).unwrap();
    // No speech_active flag yet: this block is absent from LLM observations.
    assert_eq!(evidence(&db, 0, HOUR).unwrap().len(), 1);
    assert_eq!(refresh_candidates(&mut db, HOUR, false).unwrap(), 0);
    assert!(
        take_task(&mut db, CloudProvider::Llm, HOUR)
            .unwrap()
            .is_none()
    );
    assert_eq!(refresh_candidates(&mut db, 4 * HOUR, false).unwrap(), 0);
    db.execute(
        "UPDATE audio_blocks SET analysis_state='failed' WHERE id='tail'",
        [],
    )
    .unwrap();
    assert_eq!(
        refresh_candidates(&mut db, 4 * HOUR + 15000, false).unwrap(),
        1
    );
    assert!(
        take_task(&mut db, CloudProvider::Llm, 4 * HOUR + 15000)
            .unwrap()
            .is_some()
    );
}

#[test]
fn local_waiting_predecessor_blocks_adjacent_fit_without_blocking_distant_work() {
    let mut db = db();
    speech(&db, 0, 1000, "microphone", "第一项");
    speech(&db, 20 * MINUTE, 20 * MINUTE + 1000, "microphone", "第二项");
    speech(
        &db,
        2 * HOUR,
        2 * HOUR + 1000,
        "microphone",
        "很晚的独立活动",
    );
    local_job(&db, "late", 0, 1000, "pending");
    assert_eq!(refresh_candidates(&mut db, 4 * HOUR, false).unwrap(), 2);
    let task = take_task(&mut db, CloudProvider::Llm, 4 * HOUR)
        .unwrap()
        .unwrap();
    assert_eq!(task.start, 2 * HOUR);
}

#[test]
fn parallel_activities_keep_both_ids_across_windows_and_respect_locked_predecessor() {
    for lock in [false, true] {
        let mut db = db();
        for minute in (0..30).step_by(2) {
            speech(
                &db,
                minute * MINUTE,
                minute * MINUTE + 25_000,
                "microphone",
                "家庭聊天",
            );
            speech(
                &db,
                minute * MINUTE,
                minute * MINUTE + 25_000,
                "system",
                "天文课程",
            );
        }
        assert_eq!(refresh_candidates(&mut db, 2 * HOUR, true).unwrap(), 2);
        let first = take_task(&mut db, CloudProvider::Llm, 2 * HOUR)
            .unwrap()
            .unwrap();
        let response = |task: &CloudTask, mic: Option<&str>, sys: Option<&str>| {
            json!({"events":[
                {"start_ms":task.start,"end_ms":task.end,"title":"家庭聊天","summary":"聊家务。","sources":"microphone","merge_with_event_id":mic},
                {"start_ms":task.start,"end_ms":task.end,"title":"天文课程","summary":"介绍恒星。","sources":"system","merge_with_event_id":sys}
            ]})
        };
        let input = llm_input(&db, &first).unwrap();
        publish_fit_with_input(
            &mut db,
            &first,
            &input,
            response(&first, None, None),
            2 * HOUR,
        )
        .unwrap();
        let id_for = |mask| {
            db.query_row::<String, _, _>(
                "SELECT id FROM named_events WHERE source_mask=?1",
                [mask],
                |r| r.get(0),
            )
            .unwrap()
        };
        let mic = id_for(1);
        let sys = id_for(2);
        if lock {
            db.execute(
                "UPDATE named_events SET boundary_locked=1,revision=revision+1 WHERE id=?1",
                [&mic],
            )
            .unwrap();
        }
        let next = take_task(&mut db, CloudProvider::Llm, 2 * HOUR)
            .unwrap()
            .unwrap();
        let input = llm_input(&db, &next).unwrap();
        let context: Value = serde_json::from_str(&input).unwrap();
        assert_eq!(
            context["context"]["previous_events"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let duplicate = response(&next, Some(&sys), Some(&sys));
        assert_eq!(
            publish_fit_with_input(&mut db, &next, &input, duplicate, 2 * HOUR).unwrap_err(),
            "invalid_fit_boundary_duplicate_continuation"
        );
        // The publisher also refuses a locked target even if a model ignores the prompt.
        publish_fit_with_input(
            &mut db,
            &next,
            &input,
            response(&next, Some(&mic), Some(&sys)),
            2 * HOUR,
        )
        .unwrap();
        let end_for = |id: &str| {
            db.query_row::<i64, _, _>("SELECT ended_ms FROM named_events WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(end_for(&sys), next.end);
        assert_eq!(end_for(&mic), if lock { first.end } else { next.end });
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM named_events", [], |r| r.get(0))
                .unwrap(),
            if lock { 3 } else { 2 }
        );
        if !lock {
            // The microphone activity stops; the computer course continues
            // through a third window without absorbing or extending it.
            for minute in (32..47).step_by(2) {
                speech(
                    &db,
                    minute * MINUTE,
                    minute * MINUTE + 25_000,
                    "system",
                    "天文课程继续介绍恒星演化",
                );
            }
            assert_eq!(refresh_candidates(&mut db, 2 * HOUR, true).unwrap(), 1);
            let third = take_task(&mut db, CloudProvider::Llm, 2 * HOUR)
                .unwrap()
                .unwrap();
            let input = llm_input(&db, &third).unwrap();
            publish_fit_with_input(&mut db,&third,&input,json!({"events":[{
                "start_ms":third.start,"end_ms":third.end,"title":"天文课程","summary":"继续介绍恒星演化。",
                "sources":"system","merge_with_event_id":sys
            }]}),2*HOUR).unwrap();
            assert_eq!(
                db.query_row::<i64, _, _>(
                    "SELECT ended_ms FROM named_events WHERE id=?1",
                    [&mic],
                    |r| r.get(0)
                )
                .unwrap(),
                next.end
            );
            assert_eq!(
                db.query_row::<i64, _, _>(
                    "SELECT ended_ms FROM named_events WHERE id=?1",
                    [&sys],
                    |r| r.get(0)
                )
                .unwrap(),
                third.end
            );
        }
    }
}

#[test]
fn new_window_closes_after_fifteen_minutes_or_five_minutes_quiet() {
    for continuous in [false, true] {
        let mut db = db();
        for minute in (0..if continuous { 15 } else { 3 }).step_by(2) {
            speech(
                &db,
                1_000 + minute * MINUTE,
                30_000 + minute * MINUTE,
                "microphone",
                "继续讨论方案",
            );
        }
        let deadline = if continuous {
            1_000 + 15 * MINUTE
        } else {
            30_000 + 7 * MINUTE
        };
        assert_eq!(refresh_candidates(&mut db, deadline - 1, false).unwrap(), 0);
        assert_eq!(refresh_candidates(&mut db, deadline, false).unwrap(), 1);
    }
}

#[test]
fn stored_half_hour_windows_are_not_split_or_requeued_by_shorter_policy() {
    let mut db = db();
    for minute in (0..60).step_by(2) {
        speech(
            &db,
            minute * MINUTE,
            minute * MINUTE + 30_000,
            "microphone",
            "原有会议内容",
        );
    }
    let all = evidence(&db, 0, HOUR).unwrap();
    let cut = all.partition_point(|item| item.start < 30 * MINUTE);
    let old = build_candidates_with_cuts(&all, &[], &[(0, 30 * MINUTE), (cut, HOUR)]);
    assert_eq!(old.len(), 2);
    for candidate in &old {
        db.execute(
            "INSERT INTO candidate_windows VALUES (?1,?2,?3,?4,'complete',NULL)",
            params![candidate.id, candidate.start, candidate.end, candidate.hash],
        )
        .unwrap();
    }
    assert_eq!(refresh_candidates(&mut db, 2 * HOUR, true).unwrap(), 0);
    for minute in (60..120).step_by(2) {
        speech(
            &db,
            minute * MINUTE,
            minute * MINUTE + 30_000,
            "microphone",
            "新的连续讨论",
        );
    }
    // The trailing existing window may grow with new evidence, but never
    // consumes the next persisted candidate identity.
    refresh_candidates(&mut db, 3 * HOUR, true).unwrap();
    for candidate in &old {
        let start: i64 = db
            .query_row(
                "SELECT started_ms FROM candidate_windows WHERE id=?1",
                [&candidate.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(start, candidate.start);
    }
}

#[test]
fn legacy_cross_source_cut_at_the_same_start_keeps_exact_evidence_membership() {
    let mut db = db();
    speech(&db, 0, 1_000, "microphone", "开始讨论");
    speech(&db, 900_000, 901_000, "microphone", "继续讨论");
    speech(&db, 1_799_000, 1_800_000, "microphone", "本地回应");
    speech(&db, 1_799_000, 1_801_000, "system", "远端发言");
    speech(&db, 2_100_000, 2_101_000, "system", "继续发言");
    let all = evidence(&db, 0, HOUR).unwrap();
    // Actual v1 (30-minute) membership: the shorter same-start utterance
    // fits the first window; the longer one starts the second. Construct the
    // released hashes directly, not via the generator under test.
    for group in [&all[..3], &all[3..]] {
        let mut hash = Sha256::new();
        hash.update(b"candidate-v1");
        hash.update(b"fit-v3");
        for item in group {
            hash.update(item.id.as_bytes());
            hash.update(item.start.to_le_bytes());
            hash.update(item.end.to_le_bytes());
            hash.update(item.source.as_bytes());
            hash.update(item.text.as_ref().unwrap().as_bytes());
        }
        db.execute(
            "INSERT INTO candidate_windows VALUES (?1,?2,?3,?4,'complete',NULL)",
            params![
                group[0].id,
                group[0].start,
                group.iter().map(|e| e.end).max().unwrap(),
                format!("{:x}", hash.finalize())
            ],
        )
        .unwrap();
    }
    assert_eq!(refresh_candidates(&mut db, 2 * HOUR, true).unwrap(), 0);
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn replaced_vad_evidence_keeps_the_persisted_candidate_time_cut() {
    let db = db();
    speech(&db, 0, MINUTE, "microphone", "早先发言");
    speech(&db, 5 * MINUTE, 6 * MINUTE, "microphone", "中间持续发言");
    speech(
        &db,
        10 * MINUTE,
        11 * MINUTE,
        "microphone",
        "替代原 VAD 标记的粗转",
    );
    let all = evidence(&db, 0, HOUR).unwrap();
    assert_eq!(build_candidates_with_cuts(&all, &[], &[]).len(), 1);
    for old_id in ["removed-vad-block".to_owned(), all[0].id.clone()] {
        // Missing original ID, or an ID reused after a boundary correction:
        // neither may pull the new window back to unrelated earlier speech.
        let cuts = candidate_cut_positions(&all, &[(old_id, 10 * MINUTE, 11 * MINUTE)]);
        let windows = build_candidates_with_cuts(&all, &[], &cuts);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[1].start, 10 * MINUTE);
    }
}

#[test]
fn late_shorter_same_start_speech_does_not_split_the_first_candidate() {
    let mut db = db();
    speech(&db, 0, 1_000, "microphone", "原有发言");
    refresh_candidates(&mut db, HOUR, true).unwrap();
    let first = take_task(&mut db, CloudProvider::Llm, HOUR)
        .unwrap()
        .unwrap();
    publish_one(&mut db, &first, None, HOUR);
    speech(&db, 0, 500, "system", "晚到的另一音源短句");
    assert_eq!(refresh_candidates(&mut db, HOUR + 1, true).unwrap(), 1);
    let revised = take_task(&mut db, CloudProvider::Llm, HOUR + 1)
        .unwrap()
        .unwrap();
    assert_eq!(revised.target_id, first.target_id);
    assert_ne!(revised.target_version, first.target_version);
    let all = evidence(&db, 0, HOUR).unwrap();
    let cuts = candidate_cut_positions(&all, &[(first.target_id, 0, first.end)]);
    assert_eq!(build_candidates_with_cuts(&all, &[], &cuts).len(), 1);
}

#[test]
fn fitting_keeps_predecessors_first_including_backoff_but_allows_distant_work() {
    let mut db = db();
    for minute in (0..180).step_by(2) {
        speech(
            &db,
            minute * MINUTE,
            minute * MINUTE + 30_000,
            "microphone",
            "会议讨论",
        );
    }
    speech(
        &db,
        5 * HOUR,
        5 * HOUR + MINUTE,
        "microphone",
        "另一场独立活动",
    );
    assert_eq!(refresh_candidates(&mut db, 6 * HOUR, true).unwrap(), 13);
    let first = take_task_fair(&mut db, CloudProvider::Llm, 6 * HOUR, 1)
        .unwrap()
        .unwrap();
    assert_eq!(first.start, 0); // Former newest-first turn.
    defer_paused(&mut db, &first, 6 * HOUR).unwrap();
    let distant = take_task_fair(&mut db, CloudProvider::Llm, 6 * HOUR, 1)
        .unwrap()
        .unwrap();
    assert_eq!(distant.start, 5 * HOUR);
    assert!(
        take_task_fair(&mut db, CloudProvider::Llm, 6 * HOUR, 2)
            .unwrap()
            .is_none()
    );
    let retry = take_task_fair(&mut db, CloudProvider::Llm, 6 * HOUR + 1_001, 2)
        .unwrap()
        .unwrap();
    assert_eq!(retry.id, first.id);
    let id = publish_one(&mut db, &retry, None, 6 * HOUR + 1_002);
    let second = take_task_fair(&mut db, CloudProvider::Llm, 6 * HOUR + 1_003, 1)
        .unwrap()
        .unwrap();
    assert_eq!(second.start, 16 * MINUTE);
    let input: Value = serde_json::from_str(&llm_input(&db, &second).unwrap()).unwrap();
    assert_eq!(input["context"]["previous_events"][0]["id"], id);
}

#[test]
fn boundary_excerpt_is_bounded_source_scoped_and_not_published_outside_candidate() {
    let mut db = db();
    for index in 0..40 {
        speech(
            &db,
            index * 5_000,
            index * 5_000 + 4_000,
            "microphone",
            &"甲".repeat(500),
        );
        speech(
            &db,
            index * 5_000,
            index * 5_000 + 4_000,
            "system",
            "独立的电脑节目",
        );
    }
    refresh_candidates(&mut db, HOUR, true).unwrap();
    let first = take_task(&mut db, CloudProvider::Llm, HOUR)
        .unwrap()
        .unwrap();
    let id = publish_one(&mut db, &first, None, HOUR);
    speech(&db, 20 * MINUTE, 21 * MINUTE, "microphone", "会议继续");
    refresh_candidates(&mut db, HOUR, true).unwrap();
    let second = take_task(&mut db, CloudProvider::Llm, HOUR)
        .unwrap()
        .unwrap();
    let input: Value = serde_json::from_str(&llm_input(&db, &second).unwrap()).unwrap();
    let tail = input["context"]["previous_events"][0]["tail"]
        .as_array()
        .unwrap();
    assert_eq!(tail.len(), FIT_CONTEXT_MAX_LINES);
    assert_eq!(input["context"]["previous_events"][0]["id"], id);
    for line in tail {
        assert_eq!(line["source"], "microphone");
        assert_eq!(
            line["text"].as_str().unwrap().chars().count(),
            FIT_CONTEXT_TEXT_CHARS
        );
        assert_eq!(line["text_truncated"], true);
        assert!(line["start_ms"].as_i64().unwrap() >= first.end - FIT_CONTEXT_TAIL_MS);
        assert!(line["end_ms"].as_i64().unwrap() <= first.end);
    }
    assert!(
        tail.windows(2)
            .all(|p| p[0]["start_ms"].as_i64() < p[1]["start_ms"].as_i64())
    );
    // Existing boundary checks remain authoritative despite richer context.
    let bad = json!({"events":[{"start_ms":first.start,"end_ms":second.end,
        "title":"会议","sources":"microphone","summary":null,"merge_with_event_id":id}]});
    assert_eq!(
        publish_fit_with_input(&mut db, &second, &input.to_string(), bad, HOUR).unwrap_err(),
        "invalid_fit_boundary_outside_window"
    );
    db.execute(
        "UPDATE named_events SET boundary_locked=1 WHERE id=?1",
        [&id],
    )
    .unwrap();
    let input: Value = serde_json::from_str(&llm_input(&db, &second).unwrap()).unwrap();
    assert_eq!(
        input["context"]["previous_events"][0]["boundary_locked"],
        true
    );
    let other = publish_one(&mut db, &second, Some(&id), HOUR);
    assert_ne!(id, other);
}

#[test]
fn a_new_activity_after_a_continuation_stays_separate() {
    let mut db = db();
    for minute in (0..30).step_by(2) {
        speech(
            &db,
            minute * MINUTE,
            minute * MINUTE + 30_000,
            "microphone",
            "持续发言",
        );
    }
    refresh_candidates(&mut db, 3 * HOUR, true).unwrap();
    let first = take_task(&mut db, CloudProvider::Llm, 3 * HOUR)
        .unwrap()
        .unwrap();
    let id = publish_one(&mut db, &first, None, 3 * HOUR);
    let second = take_task(&mut db, CloudProvider::Llm, 3 * HOUR)
        .unwrap()
        .unwrap();
    publish_fit(&mut db, &second, json!({"events":[
        {"start_ms":second.start,"end_ms":22*MINUTE+30_000,"title":"产品评审会","summary":null,"sources":"microphone","merge_with_event_id":id},
        {"start_ms":24*MINUTE,"end_ms":second.end,"title":"单独电话","summary":null,"sources":"microphone","merge_with_event_id":null}
    ]}), 3 * HOUR).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let end: i64 = db
        .query_row(
            "SELECT ended_ms FROM named_events WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(end, 22 * MINUTE + 30_000);
}

#[test]
fn fit_requires_complete_source_scoped_coverage_or_explicit_unorganized() {
    for (ranges, ignored, expected) in [
        (
            vec![(1000, 2000), (3000, 5000)],
            vec![],
            Some("invalid_fit_boundary_uncovered_speech"),
        ),
        (vec![(1000, 2500), (2500, 5000)], vec![1], None),
        (
            vec![(1000, 5000)],
            vec![0, 1],
            Some("invalid_fit_boundary_unorganized"),
        ),
        (
            vec![(1000, 5000)],
            vec![1, 1],
            Some("invalid_fit_boundary_unorganized"),
        ),
        (
            vec![(1000, 5000)],
            vec![2],
            Some("invalid_fit_boundary_unorganized"),
        ),
        (
            vec![(1000, 5000)],
            vec![],
            Some("invalid_fit_boundary_uncovered_speech"),
        ),
    ] {
        let mut db = db();
        speech(&db, 1000, 5000, "microphone", "一个不能截去开头的完整问题");
        speech(&db, 1001, 4999, "system", "嗯");
        refresh_candidates(&mut db, HOUR, true).unwrap();
        let task = take_task(&mut db, CloudProvider::Llm, HOUR)
            .unwrap()
            .unwrap();
        let output = json!({"events":ranges.into_iter().map(|(a,b)|json!({
            "start_ms":a,"end_ms":b,"title":"讨论","summary":null,"sources":"microphone"
        })).collect::<Vec<_>>(),"unorganized":ignored});
        let result = publish_fit(&mut db, &task, output, HOUR);
        if let Some(error) = expected {
            assert_eq!(result.unwrap_err(), error);
            assert_eq!(
                db.query_row::<i64, _, _>("SELECT count(*) FROM named_events", [], |r| r.get(0))
                    .unwrap(),
                0
            );
        } else {
            assert_eq!(result.unwrap(), 2);
        }
    }
}

fn overlapping_meeting(db: &Connection) {
    for start in (0..=840_000).step_by(60_000) {
        speech(
            db,
            start,
            start + 5_000,
            "microphone",
            "线上产品评审继续，请远端同事确认搜索入口布局。",
        );
    }
    speech(
        db,
        889_000,
        895_000,
        "microphone",
        "远端听得到吗，接着刚才的评审，请确认这个入口方案。",
    );
    speech(
        db,
        890_000,
        910_000,
        "system",
        "听得到，回应刚才的问题，我们赞同这个搜索入口方案，继续讨论预算和排期。",
    );
    for start in (920_000..=1_460_000).step_by(60_000) {
        speech(
            db,
            start,
            start + 5_000,
            "microphone",
            "收到远端的建议，我们继续这场评审，讨论同一方案的预算和排期。",
        );
    }
}

#[test]
fn overlapping_compute_windows_continue_and_never_complete_a_discarded_proposal() {
    for extended_prior in [false, true] {
        let mut db = db();
        overlapping_meeting(&db);
        assert_eq!(refresh_candidates(&mut db, HOUR, true).unwrap(), 2);
        let first = take_task(&mut db, CloudProvider::Llm, HOUR)
            .unwrap()
            .unwrap();
        assert_eq!((first.start, first.end), (0, 895_000));
        let output = |t: &CloudTask, merge: Option<&str>| {
            json!({"events":[{
            "start_ms":t.start,"end_ms":t.end,"title":"线上评审","summary":"讨论入口、预算与排期。",
            "sources":"both","merge_with_event_id":merge}],"unorganized":[]})
        };
        publish_fit(&mut db, &first, output(&first, None), HOUR).unwrap();
        let id: String = db
            .query_row("SELECT id FROM named_events", [], |r| r.get(0))
            .unwrap();
        let second = take_task(&mut db, CloudProvider::Llm, HOUR)
            .unwrap()
            .unwrap();
        assert_eq!((second.start, second.end), (890_000, 1_465_000));
        if extended_prior {
            // A later fit may already have extended the shared automatic event.
            db.execute(
                "UPDATE named_events SET ended_ms=?1,revision=revision+1",
                [second.end + 1000],
            )
            .unwrap();
        }
        let input = llm_input(&db, &second).unwrap();
        let sent: Value = serde_json::from_str(&input).unwrap();
        assert_eq!(sent["context"]["previous_events"][0]["id"], id);
        assert_eq!(
            publish_fit_with_input(&mut db, &second, &input, output(&second, None), HOUR)
                .unwrap_err(),
            "invalid_fit_boundary_existing_overlap"
        );
        assert_eq!(
            db.query_row::<String, _, _>(
                "SELECT state FROM cloud_tasks WHERE id=?1",
                [second.id.to_string()],
                |r| r.get(0)
            )
            .unwrap(),
            "processing"
        );
        publish_fit_with_input(&mut db, &second, &input, output(&second, Some(&id)), HOUR).unwrap();
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM named_events", [], |r| r.get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT ended_ms FROM named_events", [], |r| r.get(0))
                .unwrap(),
            second.end + if extended_prior { 1000 } else { 0 }
        );
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM event_candidate_links", [], |r| r
                .get(0))
                .unwrap(),
            2
        );
    }
}

fn synthetic_activity(db: &Connection, case: &str) -> usize {
    let meeting = [
        "今天先看用户反馈，入口的位置有几种方案，大家把意见说一下。",
        "这个页面上最常用的是搜索，可以放在上面；导出平时用得少。",
        "我担心按钮太挤，可以试一下把文字缩短。这个方案先记下来。",
        "刚才那个布局还要考虑小屏幕，内容多了以后会不会挡住列表。",
        "接下来看看成本，存储量按现有使用情况估算，别只看峰值。",
        "预算里还得考虑网络请求，有些数据可以复用，不需要再处理。",
        "刚才说的人员安排也影响进度，测试这边目前还有别的任务。",
        "我们把时间排一下，先做入口，再验证导出，最后统一验收。",
        "先别急着定具体日期，接口那边的依赖还需要确认。",
        "这个地方可以做个小样本测试，看看实际体验再决定。",
        "前面几个意见我记下了，等会儿把需要确认的项列出来。",
        "还有一个相关的问题，发布以后怎么判断用户有没有用到这个入口。",
    ];
    let family = [
        "你把那个小杯子递过来吧，宝宝刚才想喝水。水已经不烫了。",
        "好，喝完我们把桌子收一下。那个玩具是不是掉到沙发下面了？",
        "今天公司里挺忙的，早上那份材料改了好几遍，下午才弄完。",
        "嗯，我下午也没来得及买面包，明天早上出门顺路买一点吧。",
        "你看他又盯着那个小灯了。先把积木放盒子里，别踩到了。",
        "空调是不是有点冷？我把温度调高一点，毯子放旁边就好。",
        "刚才说到旅行，那个地方如果带孩子去，路上可能得停几次。",
        "是啊，先不急着订，我们有空再看看路线。这个衣服你帮我拿一下。",
        "这个扫地机今天又绕了半天，可能是地上东西太多，收完再开吧。",
        "他好像困了，我们小声一点。你今天看那个动画后来怎么样了？",
        "后面那集还没看完，等他睡了再说。现在先把这个枕头放回去。",
        "嗯，奶瓶洗好了放这边，明天要用的东西也顺手收在一起。",
    ];
    match case {
        "overlapping_compute_windows" => {
            overlapping_meeting(db);
            1
        }
        "two_hour_meeting"
        | "meeting_with_short_break"
        | "meeting_with_wrapup_phrases"
        | "continuous_daily_chat" => {
            let topics = if case == "continuous_daily_chat" {
                &family
            } else {
                &meeting
            };
            for i in 0..60 {
                if case == "meeting_with_short_break" && (27..31).contains(&i) {
                    continue;
                }
                let text = if case == "meeting_with_wrapup_phrases" && i % 8 == 7 {
                    "我没别的了。还有一个事，刚才说流量没有带来增长，这指的是啥？我们再看一下。"
                } else if case == "meeting_with_short_break" && i == 26 {
                    "大家休息几分钟，一会儿回来继续看后面的排期，会议还没结束。"
                } else if case == "meeting_with_short_break" && i == 31 {
                    "大家回来了，我们接着刚才那场评审，继续看测试和发布安排。"
                } else {
                    topics[i % topics.len()]
                };
                speech(
                    db,
                    i as i64 * 2 * MINUTE,
                    i as i64 * 2 * MINUTE + 25_000,
                    "microphone",
                    text,
                );
            }
            1
        }
        "meeting_then_same_project_call" | "back_to_back_meetings" => {
            for i in 0..10 {
                speech(
                    db,
                    i * 2 * MINUTE,
                    i * 2 * MINUTE + 25_000,
                    "microphone",
                    meeting[i as usize % meeting.len()],
                );
            }
            speech(
                db,
                20 * MINUTE,
                20 * MINUTE + 25_000,
                "microphone",
                "今天产品评审到这里结束，大家散会吧，我先退出会议了。",
            );
            let next = if case == "meeting_then_same_project_call" {
                [
                    "喂，是供应商那边吗？刚散会，现在单独给你打个电话，确认一下这个项目的合同。",
                    "我们这边主要想确认报价是不是包含售后，和刚才内部评审是两件事。",
                    "好的，你把合同发来，我们再核对付款条款。",
                    "那今天电话就先这样，谢谢，再见。",
                ]
            } else {
                [
                    "下午的客户沟通会现在开始，刚才内部评审的同事已经离开，请客户这边介绍需求。",
                    "我们这次主要想了解交付以后如何做培训，使用部门有很多人。",
                    "培训可以安排分批进行，我们需要先确认账号和资料准备。",
                    "好的，这场客户会就到这里，下次再确认培训时间。",
                ]
            };
            for (i, text) in next.iter().enumerate() {
                speech(
                    db,
                    (22 + i as i64 * 4) * MINUTE,
                    (22 + i as i64 * 4) * MINUTE + 25_000,
                    "microphone",
                    text,
                );
            }
            2
        }
        "different_activity_at_hour_boundary" => {
            for i in 0..60 {
                let text = match i {
                    29 => "产品评审到这里正式结束，大家散会。我先退出这个会议。",
                    30 => {
                        "喂，供应商你好。刚刚内部会议结束了，现在单独打电话和你谈同一项目的合同。"
                    }
                    31..=59 => [
                        "这个项目的付款条款，我们需要按交付节点来谈。",
                        "电话里先确认报价包含哪些服务，合同附件里还需要补上。",
                        "这个供应商合同的售后期限和结算方式，我们逐条核对。",
                        "刚才电话里说的那条我记下来了，请把修改后的合同发过来。",
                    ][i % 4],
                    _ => meeting[i % meeting.len()],
                };
                speech(
                    db,
                    i as i64 * 2 * MINUTE,
                    i as i64 * 2 * MINUTE + 25_000,
                    "microphone",
                    text,
                );
            }
            2
        }
        "independent_parallel_sources" | "independent_parallel_two_hours" => {
            let astronomy = [
                "欢迎收听天文科普节目，今天介绍恒星的形成，首先从星际气体云讲起。",
                "星云中的气体在引力作用下聚集，密度逐渐增加，形成更致密的区域。",
                "气体坍缩时温度升高，中心逐渐形成原恒星，外面还包围着尘埃。",
                "当核心温度足够高时，氢核聚变开始，恒星进入相对稳定的阶段。",
                "恒星的颜色与表面温度有关，蓝色恒星通常比红色恒星更热。",
                "质量更大的恒星消耗燃料更快，因此它们的寿命反而更短。",
                "我们通过光谱分析恒星成分，不需要直接取得恒星表面的样品。",
                "恒星的亮度和温度在赫罗图上呈现规律，主序带是其中重要的区域。",
                "太阳也是一颗恒星，目前处于主序阶段，能量主要来自核心核聚变。",
                "当核心的氢逐渐耗尽，恒星的结构会变化，外层可能显著膨胀。",
                "大质量恒星在生命末期可能发生超新星爆发，将元素抛向星际空间。",
                "这期节目介绍了恒星从形成到演化的主要过程，下期继续讲星系。",
            ];
            let length = if case == "independent_parallel_two_hours" {
                60
            } else {
                12
            };
            for i in 0..length {
                speech(
                    db,
                    i as i64 * 2 * MINUTE,
                    i as i64 * 2 * MINUTE + 25_000,
                    "microphone",
                    family[i % family.len()],
                );
                speech(
                    db,
                    i as i64 * 2 * MINUTE,
                    i as i64 * 2 * MINUTE + 25_000,
                    "system",
                    astronomy[if length > 12 {
                        i % (astronomy.len() - 1)
                    } else {
                        i
                    }],
                );
            }
            2
        }
        "shared_online_meeting" => {
            for i in 0..60 {
                let (question, answer) = [
                    (
                        "线上评审继续看这个入口，屏幕上的搜索按钮位置大家能看到吗？",
                        "能看到你共享的页面，我同意搜索放上面，导出放菜单里。",
                    ),
                    (
                        "刚才远端同事提到成本，那我们在这场评审里继续核对存储预算吧。",
                        "好的，回应刚才的问题，存储按日常使用估算，网络请求还可以复用。",
                    ),
                    (
                        "我想接着你刚才说的进度问一下，这个入口和导出谁来测试？",
                        "这两个都由我们测试组负责，先测入口，再测导出，结果发到同一份评审纪要。",
                    ),
                ][i % 3];
                speech(
                    db,
                    i as i64 * 2 * MINUTE,
                    i as i64 * 2 * MINUTE + 10_000,
                    "microphone",
                    question,
                );
                speech(
                    db,
                    i as i64 * 2 * MINUTE + 11_000,
                    i as i64 * 2 * MINUTE + 25_000,
                    "system",
                    answer,
                );
            }
            1
        }
        "isolated_low_information" => {
            speech(db, 0, 1_000, "microphone", "嗯");
            speech(db, 8 * MINUTE, 8 * MINUTE + 1_000, "microphone", "Yeah");
            speech(db, 16 * MINUTE, 16 * MINUTE + 1_000, "microphone", "松手");
            0
        }
        _ => panic!("unknown synthetic case"),
    }
}

// Opt-in, paid external check. Only reads the configured model/credential
// reference; never reads production audio/text or opens its DB for writing.
// HEAR_FIT_EVAL_CONFIG=/absolute/path/hear.sqlite cargo test -p hear-core
// live_activity_granularity -- --ignored --nocapture --test-threads=1
#[test]
#[ignore = "requires explicit configured credential and paid LLM calls with synthetic data"]
fn live_activity_granularity() {
    use std::cell::Cell;
    use std::time::{Duration, Instant};
    struct EvalMeter(Cell<u64>);
    impl crate::usage::Meter for EvalMeter {
        fn begin(&self, _: &str, _: Option<Uuid>, _: Option<i64>) -> Result<String, String> {
            Ok(Uuid::new_v4().to_string())
        }
        fn llm_response(&self, _: &str, response: &Value) -> Result<(), String> {
            self.0
                .set(self.0.get() + response["usage"]["total_tokens"].as_u64().unwrap_or(0));
            Ok(())
        }
        fn asr_response(&self, _: Uuid, _: &str, _: &Value) -> Result<(), String> {
            unreachable!()
        }
        fn rejected(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn observation_failed(&self) {}
    }
    let path = std::env::var("HEAR_FIT_EVAL_CONFIG")
        .expect("explicit read-only configuration DB required");
    let config =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let model: String = config
        .query_row(
            "SELECT model FROM cloud_queues WHERE provider='llm' AND configured=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // A development test key avoids asking the OS to grant an unsigned test
    // binary access to the installed app's Keychain entry.
    let key = std::env::var("HEAR_FIT_EVAL_KEY").unwrap_or_else(|_| {
        let reference = credential_reference(&config, CloudProvider::Llm)
            .unwrap()
            .unwrap();
        crate::cloud::credential(CloudProvider::Llm, &reference)
            .unwrap()
            .expect("credential unavailable")
    });
    drop(config);
    let repeats = std::env::var("HEAR_FIT_EVAL_REPEATS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2);
    assert!((1..=5).contains(&repeats));
    let meter = EvalMeter(Cell::new(0));
    let mut failures = Vec::new();
    for run in 0..repeats {
        for case in [
            "overlapping_compute_windows",
            "two_hour_meeting",
            "meeting_with_short_break",
            "meeting_with_wrapup_phrases",
            "continuous_daily_chat",
            "meeting_then_same_project_call",
            "back_to_back_meetings",
            "different_activity_at_hour_boundary",
            "independent_parallel_sources",
            "independent_parallel_two_hours",
            "shared_online_meeting",
            "isolated_low_information",
        ] {
            let mut db = db();
            set_configured(&db, CloudProvider::Llm, &model).unwrap();
            let expected = synthetic_activity(&db, case);
            let now = 4 * HOUR;
            let windows = refresh_candidates(&mut db, now, true).unwrap();
            let mut details = Vec::new();
            for turn in 0..windows {
                let task = take_task_fair(&mut db, CloudProvider::Llm, now, turn as u64)
                    .unwrap()
                    .unwrap();
                let input = llm_input(&db, &task).unwrap();
                match crate::cloud::chat(
                    &key,
                    &model,
                    FIT_SYSTEM,
                    &input,
                    Instant::now() + Duration::from_secs(90),
                    &meter,
                ) {
                    Ok(output) => {
                        details.push(output.clone());
                        if let Err(error) =
                            publish_fit_with_input(&mut db, &task, &input, output, now)
                        {
                            failures.push(format!("{case} run {run}: publication {error}"));
                            break;
                        }
                    }
                    Err(error) => {
                        failures.push(format!("{case} run {run}: network {error:?}"));
                        break;
                    }
                }
            }
            let mut stmt = db.prepare("SELECT id,started_ms,ended_ms,source_mask FROM named_events WHERE deleted_ms IS NULL ORDER BY started_ms,id").unwrap();
            let events: Vec<(String, i64, i64, i64)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let mut valid = events.len() == expected;
            if expected == 1 {
                valid &= events.first().is_some_and(|e| {
                    e.1 <= MINUTE
                        && e.2
                            >= if case == "overlapping_compute_windows" {
                                1_465_000
                            } else {
                                118 * MINUTE
                            }
                });
                if valid {
                    let links: i64 = db
                        .query_row(
                            "SELECT count(*) FROM event_candidate_links WHERE event_id=?1",
                            [&events[0].0],
                            |r| r.get(0),
                        )
                        .unwrap();
                    valid &= links == windows as i64;
                }
                if case == "shared_online_meeting" {
                    valid &= events.first().is_some_and(|e| e.3 == 3);
                }
            } else if case == "different_activity_at_hour_boundary" && events.len() == 2 {
                valid &= events[0].2 <= HOUR && events[1].1 >= HOUR && events[1].2 >= 118 * MINUTE;
            } else if matches!(
                case,
                "independent_parallel_sources" | "independent_parallel_two_hours"
            ) && events.len() == 2
            {
                if case == "independent_parallel_two_hours" {
                    valid &= events.iter().all(|e| e.1 <= MINUTE && e.2 >= 118 * MINUTE);
                    for e in &events {
                        let links: i64 = db
                            .query_row(
                                "SELECT count(*) FROM event_candidate_links WHERE event_id=?1",
                                [&e.0],
                                |r| r.get(0),
                            )
                            .unwrap();
                        valid &= links == windows as i64;
                    }
                }
                valid &= events[0].3 & events[1].3 == 0
                    && events[0].1 < events[1].2
                    && events[1].1 < events[0].2;
            } else if expected == 2 && events.len() == 2 {
                valid &= events[0].2 <= 22 * MINUTE
                    && events[1].1 >= 20 * MINUTE
                    && events[0].2 <= events[1].1
                    && events[1].2 >= 34 * MINUTE;
            }
            println!(
                "{}",
                json!({"case":case,"run":run+1,"model":model,"windows":windows,
                "expected_events":expected,"actual_events":events.len(),"pass":valid,"synthetic_outputs":details})
            );
            if !valid {
                failures.push(format!("{case} run {}: unexpected grouping", run + 1));
            }
        }
    }
    println!("Synthetic evaluation total tokens: {}", meter.0.get());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
