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
    let long = "保留具体细节。".repeat(300);
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
    assert_eq!(
        stored(&db).3["key_points"][0]["evidence_refs"][0]["segment_id"],
        "mic"
    );
    assert_eq!(stored(&db).3["key_points"].as_array().unwrap().len(), 1);
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
        stored(&db).3["key_points"]
    );
}

#[test]
fn understanding_legacy_migration_and_incomplete_continuation() {
    let mut db = db();
    speech(&db, "old", 1000, 901000, "microphone");
    let first = task(&mut db);
    publish(&mut db, &first, response(&first, Value::Null, None));
    let id = stored(&db).0;
    crate::context::remove_index_for_legacy_fixture(&db);
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
        let mut output = response(&task, structure(json!([0])), None);
        output["events"][0]["summary"] =
            json!([{"replaces":[],"text":"讨论导出验证与周五排期。","evidence_indices":[0]}]);
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

fn delta(points: Value, objects: Value) -> Value {
    json!({"topics":[],"key_points":points,"mentioned_objects":objects})
}

#[test]
fn understanding_omission_preserves_early_facts_and_explicit_correction_replaces_them() {
    let mut db = db();
    speech(&db, "early", 1000, 901000, "microphone");
    let first = task(&mut db);
    let initial = json!({"topics":["发布"],"key_points":[
        {"text":"发布日期定在周五。","evidence_indices":[0]},
        {"text":"预算十万元。","evidence_indices":[0]}],
        "mentioned_objects":[{"name":"导出","type":"feature","context":"本周上线。","evidence_indices":[0]}]});
    publish(&mut db, &first, response(&first, initial, None));
    let id = stored(&db).0;
    speech(&db, "middle", 961000, 1861000, "microphone");
    let second = task(&mut db);
    publish(
        &mut db,
        &second,
        response(
            &second,
            delta(
                json!([
        {"text":"验收还需要覆盖大文件。","evidence_indices":[0]}]),
                json!([]),
            ),
            Some(&id),
        ),
    );
    let middle = stored(&db).3;
    assert_eq!(middle["key_points"].as_array().unwrap().len(), 3);
    assert_eq!(
        middle["key_points"][0]["evidence_refs"][0]["segment_id"],
        "early"
    );
    assert_eq!(middle["mentioned_objects"][0]["context"], "本周上线。");
    speech(&db, "correction", 1921000, 2821000, "microphone");
    let third = task(&mut db);
    let input = llm_input(&db, &third).unwrap();
    let changes = delta(
        json!([
        {"text":"发布从周五改到下周一，预算仍为十万元。","evidence_indices":[0],"replaces":[0,1]}]),
        json!([{"name":"导出","type":"feature","context":"本周上线计划已撤回，改为下周一。","evidence_indices":[0],"replaces":[0]}]),
    );
    let mut result = response(&third, changes, Some(&id));
    result["events"][0]["summary"] =
        json!("讨论导出发布与十万元预算，验收需覆盖大文件；原定周五的发布已改为下周一。");
    publish_fit_with_input(&mut db, &third, &input, result.clone(), NOW + 1).unwrap();
    let snapshot = stored(&db);
    assert_eq!(snapshot.0, id);
    assert_eq!(snapshot.3["key_points"].as_array().unwrap().len(), 2);
    assert_eq!(
        snapshot.3["key_points"][0]["text"],
        "验收还需要覆盖大文件。"
    );
    assert_eq!(
        snapshot.3["key_points"][1]["evidence_refs"][0]["segment_id"],
        "correction"
    );
    assert!(!snapshot.3.to_string().contains("发布日期定在周五。"));
    assert_eq!(snapshot.3["mentioned_objects"].as_array().unwrap().len(), 1);
    assert!(!snapshot.3.to_string().contains("replaces"));
    assert!(!snapshot.3.to_string().contains("evidence_indices"));
    // Same in-flight response cannot apply local old indices a second time.
    publish_fit_with_input(&mut db, &third, &input, result, NOW + 2).unwrap();
    assert_eq!(stored(&db), snapshot);
}

#[test]
fn understanding_invalid_or_conflicting_updates_do_not_erase_old_claims() {
    for replaces in [
        json!([-1]),
        json!([1]),
        json!([0, 42]),
        json!(["0"]),
        json!([0.5]),
        Value::Null,
    ] {
        let mut db = db();
        speech(&db, "old", 1000, 901000, "microphone");
        let first = task(&mut db);
        publish(
            &mut db,
            &first,
            response(&first, structure(json!([0])), None),
        );
        let before = stored(&db);
        speech(&db, "new", 961000, 1861000, "microphone");
        let next = task(&mut db);
        let update = delta(
            json!([{"text":"修改后的日期。","evidence_indices":[0],"replaces":replaces}]),
            json!([]),
        );
        publish(&mut db, &next, response(&next, update, Some(&before.0)));
        assert_eq!(stored(&db).3, before.3, "{replaces}");
    }
    let mut db = db();
    speech(&db, "old", 1000, 901000, "microphone");
    let first = task(&mut db);
    publish(
        &mut db,
        &first,
        response(&first, structure(json!([0])), None),
    );
    let before = stored(&db);
    speech(&db, "new", 961000, 1861000, "microphone");
    let next = task(&mut db);
    let input: Value = serde_json::from_str(&llm_input(&db, &next).unwrap()).unwrap();
    let old_index = input["context"]["previous_events"][0]["understanding"]["key_points"][0]["evidence_indices"][0].clone();
    // An old anchor alone and an invalid anchor cannot authorize a replacement.
    for indices in [json!([old_index]), json!([0, 9999])] {
        let update = delta(
            json!([{"text":"无新证据的更正。","evidence_indices":indices,"replaces":[0]}]),
            json!([]),
        );
        let normalized = understanding::normalize(
            &update,
            &input,
            Some(&input["context"]["previous_events"][0]),
            1000,
            1861000,
            1,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&normalized).unwrap(),
            before.3
        );
    }
    let update = delta(
        json!([
        {"text":"周一发布。","evidence_indices":[0],"replaces":[0]},
        {"text":"周二发布。","evidence_indices":[0],"replaces":[0]}]),
        json!([]),
    );
    publish(&mut db, &next, response(&next, update, Some(&before.0)));
    let points = stored(&db).3["key_points"].clone();
    assert_eq!(points.as_array().unwrap().len(), 1);
    assert_eq!(points[0]["text"], "周一发布。");
}

#[test]
fn understanding_missing_patch_and_linked_correction_preserve_unmodified_content() {
    let mut db = db();
    speech(&db, "one", 1000, 10000, "microphone");
    let task = task(&mut db);
    publish(&mut db, &task, response(&task, structure(json!([0])), None));
    let before = stored(&db);
    for bad in [
        Value::Null,
        json!({}),
        json!({"topics":[],"key_points":42,"mentioned_objects":[]}),
    ] {
        publish(&mut db, &task, response(&task, bad, None));
        assert_eq!(stored(&db), before);
    }
    let update = delta(
        json!([{"text":"已取消本轮发布。","evidence_indices":[0],"replaces":[0]}]),
        json!([]),
    );
    publish(&mut db, &task, response(&task, update, None));
    let after = stored(&db);
    assert_eq!(after.3["key_points"][0]["text"], "已取消本轮发布。");
    assert_eq!(after.3["mentioned_objects"], before.3["mentioned_objects"]);
    assert_eq!(after.3["topics"], before.3["topics"]);
}

#[test]
fn summary_edits_keep_early_details_and_apply_later_correction_in_place() {
    let mut db = db();
    let mut id = None;
    for (n, start) in [1000, 961000, 1921000].into_iter().enumerate() {
        speech(
            &db,
            &format!("summary-{n}"),
            start,
            start + 900000,
            "microphone",
        );
        let task = task(&mut db);
        let input = llm_input(&db, &task).unwrap();
        let parsed: Value = serde_json::from_str(&input).unwrap();
        if n > 0 {
            assert_eq!(
                parsed["context"]["previous_events"][0]["summary_sentences"][0]["text"],
                "讨论导出验证与发布安排。"
            );
            assert_eq!(
                parsed["context"]["previous_events"][0]["summary_sentences"][1]["item_index"],
                1
            );
        }
        let updates = match n {
            0 => {
                json!([{"replaces":[],"text":"讨论导出验证与发布安排。 原计划周五发布，预算五万元。","evidence_indices":[0]}])
            }
            1 => json!([{"replaces":[],"text":"先灰度5%，由小林验收。","evidence_indices":[0]}]),
            _ => {
                json!([{"replaces":[1],"text":"发布改为周一，预算仍为五万元。","evidence_indices":[0]}])
            }
        };
        let mut output = response(&task, structure(json!([0])), id.as_deref());
        output["events"][0]["summary"] = updates;
        publish_fit_with_input(&mut db, &task, &input, output, NOW + 1).unwrap();
        id = Some(stored(&db).0);
    }
    assert_eq!(
        stored(&db).2.as_deref(),
        Some("讨论导出验证与发布安排。 发布改为周一，预算仍为五万元。 先灰度5%，由小林验收。")
    );
}

#[test]
fn summary_compression_then_correction_preserves_constraints_and_other_views() {
    let mut db = db();
    let mut id = None;
    for (n, start) in [1000, 961000, 1921000].into_iter().enumerate() {
        speech(
            &db,
            &format!("budget-{n}"),
            start,
            start + 900000,
            "microphone",
        );
        let task = task(&mut db);
        let input = llm_input(&db, &task).unwrap();
        let parsed: Value = serde_json::from_str(&input).unwrap();
        assert_eq!(parsed["summary_budget"]["target_chars"], 900);
        let old = &parsed["context"]["previous_events"][0];
        let historical = if n == 0 {
            0
        } else {
            assert_eq!(
                old["summary_chars"].as_u64().unwrap() as usize,
                old["summary"].as_str().unwrap().chars().count()
            );
            old["understanding"]["key_points"][0]["evidence_indices"][0]
                .as_u64()
                .unwrap()
        };
        let updates = match n {
            0 => {
                json!([{"mode":"update","replaces":[],"text":"讨论连锁门店库存核对项目。 项目服务连锁门店，盘点时各门店分别核对库存。 计划周五灰度5%，预算五万元，由小林验收。 若导出验证失败则暂停发布。","evidence_indices":[0]}])
            }
            1 => json!([
                {"mode":"compress","replaces":[0,1],"text":"按门店分别核对库存。","evidence_indices":[historical]},
                {"mode":"update","replaces":[],"text":"有人主张取消灰度，另一方坚持保留，尚未形成共识。","evidence_indices":[0]}]),
            _ => json!([
                {"mode":"update","replaces":[1],"text":"改为周一灰度5%，预算八万元，仍由小林验收。","evidence_indices":[0,historical]},
                {"mode":"compress","replaces":[2],"text":"导出验证失败则暂停发布。","evidence_indices":[historical]}]),
        };
        let mut output = response(&task, structure(json!([0])), id.as_deref());
        output["events"][0]["summary"] = updates;
        publish_fit_with_input(&mut db, &task, &input, output, NOW + 1).unwrap();
        id = Some(stored(&db).0);
    }
    assert_eq!(
        stored(&db).2.as_deref(),
        Some(
            "按门店分别核对库存。 改为周一灰度5%，预算八万元，仍由小林验收。 导出验证失败则暂停发布。 有人主张取消灰度，另一方坚持保留，尚未形成共识。"
        )
    );
}

#[test]
fn summary_invalid_compression_cannot_consume_old_sentences() {
    let input = json!({"candidate":{"start_ms":100,"end_ms":200},
        "observations":[{"evidence_index":0}],
        "evidence":[{"source":"microphone","start_ms":100,"end_ms":200},
            {"source":"microphone","start_ms":1,"end_ms":99},
            {"source":"system","start_ms":1,"end_ms":99}]});
    let old = json!({"start_ms":1,"end_ms":99,"summary":"如果验证失败，周五发布就必须暂停。 预算五万元。",
        "understanding":{"key_points":[{"evidence_indices":[1,2]}],"mentioned_objects":[]}});
    for edit in [
        json!({"mode":"compress","replaces":[],"text":"暂停发布。","evidence_indices":[1]}),
        json!({"mode":"compress","replaces":[0],"text":"暂停发布。","evidence_indices":[0]}),
        json!({"mode":"compress","replaces":[0],"text":"暂停发布。","evidence_indices":[2]}),
        json!({"mode":"compress","replaces":[0,0],"text":"暂停发布。","evidence_indices":[1]}),
        json!({"mode":"compress","replaces":[9],"text":"暂停发布。","evidence_indices":[1]}),
        json!({"mode":"compress","replaces":[0],"text":"如果验证失败，周五发布就必须暂停。","evidence_indices":[1]}),
        json!({"mode":"rewrite","replaces":[0],"text":"暂停发布。","evidence_indices":[0]}),
        json!({"mode":null,"replaces":[0],"text":"暂停发布。","evidence_indices":[0]}),
    ] {
        assert_eq!(
            summary::apply(&json!([edit]), Some(&old), &input, 1, 200, 1).as_deref(),
            old["summary"].as_str()
        );
    }
    // Splitting punctuation adds spaces in storage; raw text length is not sufficient.
    let mut short_old = old.clone();
    short_old["summary"] = json!("甲乙丙丁戊己庚辛。");
    assert_eq!(summary::apply(&json!([{"mode":"compress","replaces":[0],"text":"甲。乙。丙。丁。","evidence_indices":[1]}]), Some(&short_old), &input, 1, 200, 1).as_deref(), short_old["summary"].as_str());
    let edits = json!([
        {"mode":"compress","replaces":[0],"text":"验证失败则暂停周五发布。","evidence_indices":[1]},
        {"mode":"update","replaces":[0],"text":"发布取消。","evidence_indices":[0]}]);
    assert_eq!(
        summary::apply(&edits, Some(&old), &input, 1, 200, 1).as_deref(),
        Some("验证失败则暂停周五发布。 预算五万元。")
    );
}

#[test]
fn summary_invalid_edits_and_legacy_partial_text_cannot_remove_history() {
    let input = json!({"candidate":{"start_ms":100,"end_ms":200},
        "observations":[{"evidence_index":0},{"evidence_index":1},{"evidence_index":2}],
        "evidence":[{"source":"microphone","start_ms":100,"end_ms":200},
            {"source":"system","start_ms":100,"end_ms":200},
            {"source":"microphone","start_ms":300,"end_ms":400},
            {"source":"microphone","start_ms":1,"end_ms":99}]});
    let old = json!({"start_ms":1,"end_ms":99,"summary":"预算五万元。 周五发布。"});
    for patch in [
        json!([{"text":"预算取消。","replaces":[9],"evidence_indices":[0]}]),
        json!([{"text":"预算取消。","replaces":[0,0],"evidence_indices":[0]}]),
        json!([{"text":"预算取消。","replaces":[0],"evidence_indices":[3]}]),
        json!([{"text":"预算取消。","replaces":[0],"evidence_indices":[1]}]),
        json!([{"text":"预算取消。","replaces":[0],"evidence_indices":[2]}]),
        json!([{"text":"预算取消。","replaces":[0],"evidence_indices":[999]}]),
        json!([{"text":"","replaces":[0],"evidence_indices":[0]}]),
        json!("只讨论当前窗口。"),
        Value::Null,
    ] {
        assert_eq!(
            summary::apply(&patch, Some(&old), &input, 1, 200, 1).as_deref(),
            old["summary"].as_str()
        );
    }
    let patches = json!([
        {"text":"预算八万元。","replaces":[0],"evidence_indices":[0]},
        {"text":"预算取消。","replaces":[0],"evidence_indices":[0]},
        {"text":"先测试。","replaces":[],"evidence_indices":[0]}]);
    assert_eq!(
        summary::apply(&patches, Some(&old), &input, 1, 200, 1).as_deref(),
        Some("预算八万元。 周五发布。 先测试。")
    );
    let missing = json!({"start_ms":1,"end_ms":99,"summary":null});
    assert_eq!(
        summary::apply(&patches, Some(&missing), &input, 1, 200, 1),
        None
    );
}

#[test]
fn illegal_time_keeps_published_event_and_processing_task_unchanged() {
    for bad_first in [true, false] {
        let mut db = db();
        speech(&db, "range-history", 1000, 10000, "microphone");
        let task = task(&mut db);
        let valid = response(&task, structure(json!([0])), None);
        publish(&mut db, &task, valid.clone());
        let before = stored(&db);
        db.execute(
            "UPDATE cloud_tasks SET state='processing' WHERE id=?1",
            [task.id.to_string()],
        )
        .unwrap();
        db.execute(
            "UPDATE candidate_windows SET state='processing' WHERE id=?1",
            [&task.target_id],
        )
        .unwrap();
        for (start, end) in [
            (json!(null), json!(9000)),
            (json!("1000"), json!(9000)),
            (json!(1000), json!(1000)),
            (json!(0), json!(10000)),
            (json!(1000), json!(10001)),
        ] {
            let bad = json!({"start_ms":start,"end_ms":end,"sources":"microphone"});
            let good = valid["events"][0].clone();
            let output = json!({"events":if bad_first {vec![bad,good]} else {vec![good,bad]}});
            assert!(
                publish_fit(&mut db, &task, output, NOW + 2)
                    .unwrap_err()
                    .starts_with("invalid_fit_boundary_")
            );
            assert_eq!(stored(&db), before);
            assert_eq!(
                db.query_row::<String, _, _>(
                    "SELECT state FROM cloud_tasks WHERE id=?1",
                    [task.id.to_string()],
                    |r| r.get(0)
                )
                .unwrap(),
                "processing"
            );
        }
        task_failed(&db, &task, "invalid_fit_boundary_range", NOW + 3).unwrap();
        assert_eq!(stored(&db), before);
    }
}
