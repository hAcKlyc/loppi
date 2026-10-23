//! Synthetic event snapshots; no personal transcripts, provider calls or live DB.
use super::*;
use serde_json::{Value, json};

const NOW: i64 = 4_000_000;
fn db() -> Connection {
    let db = crate::Store::new(Connection::open_in_memory().unwrap())
        .unwrap()
        .db;
    set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
    db
}
fn speech(db: &Connection, id: &str, start: i64, end: i64, source: &str) {
    db.execute("INSERT INTO coarse_segments VALUES (?1,'block','session',?2,?3,?4,'先验证导出，再决定周五是否发布。','test')",
        params![id,source,start,end]).unwrap();
}
fn task(db: &mut Connection) -> CloudTask {
    refresh_candidates(db, NOW, true).unwrap();
    take_task(db, CloudProvider::Llm, NOW).unwrap().unwrap()
}
fn structure(indices: Value) -> Value {
    json!({"topics":["发布排期","发布排期"],"key_points":[{"text":"先验证导出，再决定是否发布。","evidence_indices":indices}],
        "mentioned_objects":[{"name":"导出","type":"feature","context":"发布前待验证的功能。","entity_id":"model-guessed-person","evidence_indices":indices}]})
}
fn response(task: &CloudTask, understanding: Value, merge: Option<&str>) -> Value {
    json!({"events":[{"start_ms":task.start,"end_ms":task.end,"title":"发布讨论","summary":"讨论导出验证与周五排期，日期尚未确定。",
        "sources":"microphone","merge_with_event_id":merge,"understanding":understanding}]})
}
fn stored(db: &Connection) -> (String, i64, Option<String>, Value) {
    db.query_row("SELECT id,revision,summary,understanding_json FROM named_events WHERE source_mask&1!=0 AND deleted_ms IS NULL ORDER BY started_ms LIMIT 1", [], |r| {
        let value: Option<String> = r.get(3)?;
        Ok((r.get(0)?,r.get(1)?,r.get(2)?,value.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)))
    }).unwrap()
}
fn publish(db: &mut Connection, task: &CloudTask, output: Value) -> usize {
    publish_fit(db, task, output, NOW + 1).unwrap()
}

#[test]
fn understanding_roundtrip_long_summary_metadata_only_update_and_noop() {
    let mut db = db();
    speech(
        &db,
        "00000000-0000-4000-8000-000000000001",
        1000,
        10000,
        "microphone",
    );
    let task = task(&mut db);
    let long = "保留具体细节。".repeat(100);
    let mut output = response(&task, structure(json!([0, 0])), None);
    output["events"][0]["summary"] = json!(long);
    assert_eq!(publish(&mut db, &task, output.clone()), 1);
    let (id, rev, summary, understanding) = stored(&db);
    assert_eq!(summary.as_deref(), Some(long.as_str()));
    assert_eq!(
        event_detail(&db, Uuid::parse_str(&id).unwrap(), 0, 10)
            .unwrap()
            .summary,
        summary
    );
    assert_eq!(understanding["schema_version"], 1);
    assert_eq!(understanding["topics"].as_array().unwrap().len(), 1);
    assert_eq!(
        understanding["key_points"][0]["evidence_refs"],
        json!([{"segment_id":"00000000-0000-4000-8000-000000000001","source":"microphone","start_ms":1000,"end_ms":10000}])
    );
    assert!(understanding["mentioned_objects"][0]["entity_id"].is_null());
    assert!(!understanding.to_string().contains("evidence_indices"));
    let count: i64 = db
        .query_row("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(publish(&mut db, &task, output.clone()), 0);
    assert_eq!(stored(&db).1, rev);
    output["events"][0]["understanding"]["topics"] = json!(["导出验证"]);
    assert_eq!(publish(&mut db, &task, output), 1);
    assert_eq!(stored(&db).1, rev + 1);
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
            .unwrap(),
        count
    );
}

#[test]
fn understanding_three_windows_reindex_old_evidence_and_keep_event_identity() {
    let mut db = db();
    let mut id = None;
    for (n, start) in [1000, 961000, 1921000].into_iter().enumerate() {
        speech(
            &db,
            &format!("segment-{n}"),
            start,
            start + 900000,
            "microphone",
        );
        let task = task(&mut db);
        let input: Value = serde_json::from_str(&llm_input(&db, &task).unwrap()).unwrap();
        assert_eq!(input["observations"][0]["evidence_index"], 0);
        if n > 0 {
            let prior = &input["context"]["previous_events"][0];
            assert_eq!(prior["id"].as_str(), id.as_deref());
            assert!(prior["summary"].as_str().unwrap().contains("导出"));
            let i = prior["understanding"]["key_points"][0]["evidence_indices"][0]
                .as_u64()
                .unwrap() as usize;
            assert_eq!(input["evidence"][i]["segment_id"], "segment-0");
            assert_ne!(i, 0);
        }
        let indices: Vec<usize> = (0..input["evidence"].as_array().unwrap().len()).collect();
        publish(
            &mut db,
            &task,
            response(&task, structure(json!(indices)), id.as_deref()),
        );
        let snapshot = stored(&db);
        if let Some(id) = &id {
            assert_eq!(&snapshot.0, id);
        }
        id = Some(snapshot.0);
        assert_eq!(
            snapshot.3["key_points"][0]["evidence_refs"]
                .as_array()
                .unwrap()
                .len(),
            n + 1
        );
    }
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM named_events", [], |r| r.get(0))
            .unwrap(),
        1
    );
}

#[test]
fn understanding_rejects_invalid_claims_but_preserves_event_and_summary() {
    for indices in [
        json!([]),
        json!([-1]),
        json!([0, 500]),
        json!([0.5]),
        json!(["0"]),
        json!([1]),
        json!([2]),
    ] {
        let mut db = db();
        speech(&db, "mic", 1000, 10000, "microphone");
        speech(&db, "sys", 1000, 10000, "system");
        speech(&db, "later", 20000, 30000, "microphone");
        let task = task(&mut db);
        let mut output = response(&task, structure(indices.clone()), None);
        output["events"][0]["end_ms"] = json!(10000);
        publish(&mut db, &task, output);
        let snapshot = stored(&db);
        assert!(snapshot.2.is_some());
        assert_eq!(snapshot.3["key_points"], json!([]), "{indices}");
        assert_eq!(snapshot.3["mentioned_objects"], json!([]));
    }
    for value in [
        Value::Null,
        json!({}),
        json!({"topics":[],"key_points":42,"mentioned_objects":[]}),
    ] {
        let mut db = db();
        speech(&db, "mic", 1000, 10000, "microphone");
        let task = task(&mut db);
        publish(&mut db, &task, response(&task, value, None));
        assert!(stored(&db).3.is_null());
        assert!(stored(&db).2.is_some());
    }
}

#[test]
fn understanding_cannot_borrow_inherited_refs_from_other_parallel_event() {
    let mut db = db();
    speech(&db, "mic", 1000, 901000, "microphone");
    speech(&db, "sys", 1000, 901000, "system");
    let first = task(&mut db);
    let mut output = response(&first, structure(json!([0])), None);
    let mut sys = output["events"][0].clone();
    sys["sources"] = json!("system");
    sys["understanding"] = structure(json!([1]));
    output["events"].as_array_mut().unwrap().push(sys);
    publish(&mut db, &first, output);
    let mic = stored(&db).0;
    speech(&db, "new", 961000, 1861000, "microphone");
    let next = task(&mut db);
    let input: Value = serde_json::from_str(&llm_input(&db, &next).unwrap()).unwrap();
    let foreign = input["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .position(|v| v["segment_id"] == "sys")
        .unwrap();
    let output = response(&next, structure(json!([foreign])), Some(&mic));
    publish_fit_with_input(&mut db, &next, &input.to_string(), output, NOW + 1).unwrap();
    assert_eq!(stored(&db).3["key_points"], json!([]));
    // Direct normalization isolates ownership even when times/source masks permit it.
    let prior = input["context"]["previous_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == mic)
        .unwrap();
    let normalized = understanding::normalize(
        &structure(json!([foreign])),
        &input,
        Some(prior),
        1000,
        1861000,
        3,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&normalized).unwrap()["key_points"],
        json!([])
    );
}

#[test]
fn understanding_legacy_migration_and_incomplete_continuation() {
    let mut db = db();
    speech(&db, "old", 1000, 901000, "microphone");
    let first = task(&mut db);
    publish(&mut db, &first, response(&first, Value::Null, None));
    let id = stored(&db).0;
    db.execute_batch("ALTER TABLE named_events DROP COLUMN understanding_json; DELETE FROM core_migrations WHERE name='event_understanding_v1'").unwrap();
    init(&db).unwrap();
    init(&db).unwrap();
    assert_eq!(refresh_candidates(&mut db, NOW, true).unwrap(), 0);
    assert!(
        take_task(&mut db, CloudProvider::Llm, NOW)
            .unwrap()
            .is_none()
    );
    speech(&db, "new", 961000, 1861000, "microphone");
    let next = task(&mut db);
    publish(
        &mut db,
        &next,
        response(&next, structure(json!([0])), Some(&id)),
    );
    assert_eq!(stored(&db).0, id);
    assert!(stored(&db).3.is_null());
    assert!(stored(&db).2.is_some());
}

#[test]
fn understanding_invalidates_on_scope_change_and_late_input_changes() {
    for change in [
        "boundary",
        "source",
        "title",
        "new_snapshot",
        "invalidate",
        "text",
        "delete_ref",
        "task",
    ] {
        let mut db = db();
        speech(&db, "mic", 1000, 10000, "microphone");
        let task = task(&mut db);
        let output = response(&task, structure(json!([0])), None);
        publish(&mut db, &task, output.clone());
        let (id, rev, _, _) = stored(&db);
        let input = llm_input(&db, &task).unwrap();
        match change {
            "boundary" => edit_event(
                &mut db,
                Uuid::parse_str(&id).unwrap(),
                rev,
                2000,
                10000,
                "发布讨论",
                None,
                NOW + 2,
            )
            .unwrap(),
            "source" => edit_event(
                &mut db,
                Uuid::parse_str(&id).unwrap(),
                rev,
                1000,
                10000,
                "发布讨论",
                Some(EventSources::Both),
                NOW + 2,
            )
            .unwrap(),
            "title" => {
                edit_event(
                    &mut db,
                    Uuid::parse_str(&id).unwrap(),
                    rev,
                    1000,
                    10000,
                    "人工标题",
                    None,
                    NOW + 2,
                )
                .unwrap();
                assert!(!stored(&db).3.is_null());
            }
            "new_snapshot" => {
                let mut newer = output.clone();
                newer["events"][0]["understanding"]["topics"] = json!(["更新后的有效主题"]);
                publish(&mut db, &task, newer);
            }
            "invalidate" => invalidate_event_understanding(&db, 2000, 3000, 1).unwrap(),
            "text" => {
                db.execute(
                    "UPDATE coarse_segments SET text='新内容' WHERE id='mic'",
                    [],
                )
                .unwrap();
            }
            "delete_ref" => {
                db.execute("DELETE FROM coarse_segments WHERE id='mic'", [])
                    .unwrap();
                let current: Value = serde_json::from_str(&llm_input(&db, &task).unwrap()).unwrap();
                assert!(current["context"]["linked_events"][0]["understanding"].is_null());
                // Keep speech evidence so an otherwise valid event can still publish.
                speech(&db, "replacement", 1000, 10000, "microphone");
            }
            "task" => {
                supersede_task(&db, &task, NOW + 2).unwrap();
                invalidate_event_understanding(&db, 2000, 3000, 1).unwrap();
            }
            _ => unreachable!(),
        }
        let before_late = stored(&db);
        publish_fit_with_input(&mut db, &task, &input, output, NOW + 3).unwrap();
        if matches!(change, "title" | "new_snapshot") {
            assert_eq!(stored(&db), before_late, "{change}");
            assert!(!stored(&db).3.is_null());
        } else {
            assert!(stored(&db).3.is_null(), "{change}");
            assert!(stored(&db).2.is_none(), "{change}");
        }
    }
}

#[test]
fn understanding_corrected_scope_cannot_regenerate_from_coarse() {
    for state in ["active", "pending_review"] {
        let mut db = db();
        speech(&db, "mic", 1000, 10000, "microphone");
        let task = task(&mut db);
        db.execute("INSERT INTO text_corrections (id,original_segment_id,source,started_ms,ended_ms,text,state,revision,created_ms,updated_ms) VALUES ('fix','segment','microphone',2000,3000,'人工纠正',?1,1,0,0)",[state]).unwrap();
        publish(&mut db, &task, response(&task, structure(json!([0])), None));
        assert!(stored(&db).2.is_none());
        assert!(stored(&db).3.is_null());
    }
}

#[test]
fn understanding_survives_reopening_without_new_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hear.sqlite");
    let mut db = crate::Store::new(Connection::open(&path).unwrap())
        .unwrap()
        .db;
    set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
    speech(&db, "persistent", 1000, 10000, "microphone");
    let task = task(&mut db);
    publish(&mut db, &task, response(&task, structure(json!([0])), None));
    let expected = stored(&db);
    let jobs: i64 = db
        .query_row("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
        .unwrap();
    drop(db);
    let mut db = crate::Store::new(Connection::open(&path).unwrap())
        .unwrap()
        .db;
    assert_eq!(stored(&db), expected);
    let input: Value = serde_json::from_str(&llm_input(&db, &task).unwrap()).unwrap();
    assert_eq!(
        input["context"]["linked_events"][0]["understanding"]["key_points"][0]["evidence_indices"],
        json!([0])
    );
    assert_eq!(refresh_candidates(&mut db, NOW, true).unwrap(), 0);
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
            .unwrap(),
        jobs
    );
}
