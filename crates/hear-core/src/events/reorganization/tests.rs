use super::*;

fn fixture() -> (Connection, String, String) {
    let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
        .unwrap()
        .db;
    set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
    db.execute("INSERT INTO coarse_segments VALUES('00000000-0000-0000-0000-000000000001','block1','session','microphone',1000,3000,'Product meeting: design the interface.','test')",[]).unwrap();
    db.execute("INSERT INTO coarse_segments VALUES('00000000-0000-0000-0000-000000000002','block2','session','microphone',3000,6000,'Same product meeting: agree on delivery.','test')",[]).unwrap();
    let a = create_event(
        &mut db,
        1000,
        3000,
        "First half",
        EventSources::Microphone,
        8000,
    )
    .unwrap()
    .to_string();
    let b = create_event(
        &mut db,
        3000,
        6000,
        "Second half",
        EventSources::Microphone,
        8000,
    )
    .unwrap()
    .to_string();
    (db, a, b)
}
fn job(db: &mut Connection) -> CloudTask {
    let id = Uuid::new_v4();
    let tx = db.transaction().unwrap();
    enqueue(
        &tx,
        id,
        1000,
        5000,
        "Product meeting",
        EventSources::Microphone,
        10000,
    )
    .unwrap();
    tx.commit().unwrap();
    take_task(db, CloudProvider::Llm, 10001).unwrap().unwrap()
}
fn event(id: Option<&str>, start: i64, end: i64, source: &str, title: &str) -> Value {
    json!({"event_id":id,"start_ms":start,"end_ms":end,"sources":source,"title":title,"merge_with_event_id":null,"summary":"The product meeting covered design and delivery.","understanding":{"topics":["product"],"key_points":[{"text":"Agree on delivery.","evidence_indices":[1],"replaces":[]}],"mentioned_objects":[]}})
}
fn count(db: &Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn submission_keeps_existing_cards_and_provides_full_context() {
    let (mut db, _, _) = fixture();
    let task = job(&mut db);
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL"
        ),
        2
    );
    let sent: Value = serde_json::from_str(&input(&db, &task).unwrap()).unwrap();
    assert_eq!(sent["user_instruction"]["end_ms"], 5000);
    assert!(sent["candidate"]["end_ms"].as_i64().unwrap() >= 6000);
    assert_eq!(
        sent["context"]["linked_events"].as_array().unwrap().len(),
        2
    );
    assert!(sent["observations"].to_string().contains("delivery"));
}
#[test]
fn model_can_replace_manual_events_without_locks_or_suppressions() {
    let (mut db, a, b) = fixture();
    let task = job(&mut db);
    let result = json!({"events":[event(Some(&a),1000,6000,"microphone","Product meeting")],"retire_event_ids":[b]});
    publish_fit(&mut db, &task, result, 11000).unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL AND origin='automatic' AND boundary_locked=0 AND title_locked=0 AND summary IS NOT NULL"
        ),
        1
    );
    assert_eq!(count(&db, "SELECT count(*) FROM event_suppressions"), 0);
    assert_eq!(count(&db, "SELECT count(*) FROM coarse_segments"), 2);
    assert_eq!(
        count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
        0
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM cloud_tasks WHERE provider='llm' AND state='complete'"
        ),
        1
    );
    assert_eq!(
        db.query_row("SELECT ended_ms FROM named_events WHERE id=?1", [a], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        6000
    );
}
#[test]
fn omission_is_not_deletion_and_no_change_completes() {
    let (mut db, _, _) = fixture();
    let task = job(&mut db);
    publish_fit(
        &mut db,
        &task,
        json!({"events":[],"retire_event_ids":[]}),
        11000,
    )
    .unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL"
        ),
        2
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM cloud_tasks WHERE state='complete'"
        ),
        1
    );
}
#[test]
fn invalid_final_overlap_or_unknown_id_rolls_back_all_edits() {
    for alien in [false, true] {
        let (mut db, a, b) = fixture();
        let task = job(&mut db);
        let result = if alien {
            json!({"events":[],"retire_event_ids":[a,"unknown"]})
        } else {
            json!({"events":[event(Some(&a),1000,6000,"microphone","merged")],"retire_event_ids":[]})
        };
        assert!(publish_fit(&mut db, &task, result, 11000).is_err());
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL AND origin='manual'"
            ),
            2
        );
        assert_eq!(
            db.query_row("SELECT revision FROM named_events WHERE id=?1", [b], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
    }
}
#[test]
fn stale_input_does_not_overwrite_a_new_user_edit() {
    let (mut db, a, b) = fixture();
    let task = job(&mut db);
    let sent = input(&db, &task).unwrap();
    db.execute(
        "UPDATE named_events SET title='New human edit',revision=revision+1 WHERE id=?1",
        [&a],
    )
    .unwrap();
    assert_eq!(
        publish_fit_with_input(
            &mut db,
            &task,
            &sent,
            json!({"events":[event(Some(&a),1000,6000,"microphone","old")],"retire_event_ids":[b]}),
            11000
        )
        .unwrap_err(),
        "reorganization_revision_conflict"
    );
    assert_eq!(
        db.query_row("SELECT title FROM named_events WHERE id=?1", [a], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "New human edit"
    );
}
#[test]
fn selected_source_can_split_a_mixed_event_while_preserving_other_source() {
    let (mut db, a, b) = fixture();
    db.execute(
        "UPDATE named_events SET source_mask=3,summary='Original summary' WHERE id=?1",
        [&a],
    )
    .unwrap();
    let task = job(&mut db);
    let result = json!({"events":[event(Some(&a),1000,3000,"system","First half"),event(None,1000,6000,"microphone","Product meeting")],"retire_event_ids":[b]});
    publish_fit(&mut db, &task, result, 11000).unwrap();
    let preserved: (i64, String, String) = db
        .query_row(
            "SELECT source_mask,title,summary FROM named_events WHERE id=?1",
            [a],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        preserved,
        (2, "First half".into(), "Original summary".into())
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL"
        ),
        2
    );
}
#[test]
fn completed_event_and_manual_range_asr_are_reused() {
    let (mut db, a, b) = fixture();
    db.execute(
        "INSERT INTO manual_ranges VALUES('range',3000,6000,1,1)",
        [],
    )
    .unwrap();
    let tx = db.transaction().unwrap();
    insert_task(
        &tx,
        "asr",
        "event",
        &a,
        "1:1000:3000",
        1000,
        3000,
        Some("microphone"),
        true,
        8000,
        "test",
    )
    .unwrap();
    insert_task(
        &tx,
        "asr",
        "range",
        "range",
        "1",
        3000,
        6000,
        Some("microphone"),
        true,
        8000,
        "test",
    )
    .unwrap();
    tx.execute(
        "UPDATE cloud_tasks SET state='complete' WHERE provider='asr'",
        [],
    )
    .unwrap();
    tx.execute("INSERT INTO event_asr_refs SELECT ?1,id,started_ms,ended_ms FROM cloud_tasks WHERE target_id=?1",[&a]).unwrap();
    tx.execute("INSERT INTO precise_segments SELECT id,id,'microphone',started_ms,ended_ms,'Recognized text','scope','speaker' FROM cloud_tasks WHERE provider='asr'",[]).unwrap();
    let segment: String = tx
        .query_row(
            "SELECT id FROM precise_segments ORDER BY started_ms LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    crate::knowledge::correct_text(
        &tx,
        Uuid::parse_str(&segment).unwrap(),
        "Human correction",
        9000,
    )
    .unwrap();
    crate::knowledge::register_speaker(&tx, "scope", "speaker").unwrap();
    let person = crate::knowledge::create_person(&tx, "Confirmed person", 9000).unwrap();
    crate::knowledge::bind_speaker(&tx, Uuid::parse_str(&segment).unwrap(), person, false, 9000)
        .unwrap();
    crate::moments::add(
        &tx,
        hear_protocol::MomentKind::Comment,
        1500,
        Some("Keep this note"),
        9000,
    )
    .unwrap();
    tx.commit().unwrap();
    let task = job(&mut db);
    publish_fit(&mut db,&task,json!({"events":[event(None,1000,6000,"microphone","Product meeting")],"retire_event_ids":[a,b]}),11000).unwrap();
    assert_eq!(
        count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
        2
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM transcript_tasks WHERE target_kind='event' AND state='complete'"
        ),
        2
    );
    assert_eq!(count(&db, "SELECT count(*) FROM precise_segments"), 2);
    assert_eq!(count(&db, "SELECT count(*) FROM manual_ranges"), 1);
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM text_corrections WHERE state='active' AND text='Human correction'"
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM speaker_bindings WHERE state='active' AND origin='manual'"
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM moments WHERE text='Keep this note'"
        ),
        1
    );
    let merged: String = db
        .query_row(
            "SELECT id FROM named_events WHERE deleted_ms IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let detail = event_detail(&db, Uuid::parse_str(&merged).unwrap(), 0, 100).unwrap();
    assert_eq!(detail.precise_segments.len(), 2);
    assert!(
        detail
            .precise_segments
            .iter()
            .any(|s| s.text == "Human correction")
    );
}
#[test]
fn rejects_missing_text_or_future_without_creating_a_request() {
    let (mut db, _, _) = fixture();
    let tx = db.transaction().unwrap();
    assert_eq!(
        enqueue(
            &tx,
            Uuid::new_v4(),
            1000,
            12000,
            "test",
            EventSources::Microphone,
            10000
        )
        .unwrap_err(),
        "reorganization_future_range"
    );
    assert_eq!(
        enqueue(
            &tx,
            Uuid::new_v4(),
            1000,
            5000,
            "test",
            EventSources::System,
            10000
        )
        .unwrap_err(),
        "no_coarse_text"
    );
    assert_eq!(
        count(&tx, "SELECT count(*) FROM event_reorganization_requests"),
        0
    );
}
#[test]
fn cleanup_invalidates_request_and_its_late_result() {
    let (mut db, _, _) = fixture();
    let task = job(&mut db);
    let sent = input(&db, &task).unwrap();
    let tx = db.transaction().unwrap();
    clear(&tx, 4000, 6000, 1, 11000).unwrap();
    tx.commit().unwrap();
    assert!(!task_current(&db, &task).unwrap());
    publish_fit_with_input(&mut db, &task, &sent, json!({"events":[]}), 12000).unwrap();
    assert_eq!(
        count(&db, "SELECT count(*) FROM event_reorganization_requests"),
        0
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL"
        ),
        2
    );
}
#[test]
fn suppression_release_preserves_unselected_time_and_source() {
    let (mut db, _, _) = fixture();
    let tx = db.transaction().unwrap();
    tx.execute("INSERT INTO event_suppressions VALUES('s',0,10000,3,0)", [])
        .unwrap();
    release_suppressions(&tx, 1000, 6000, 1).unwrap();
    assert!(!suppressed(&tx, 1000, 6000, 1).unwrap());
    assert!(suppressed(&tx, 1000, 6000, 2).unwrap());
    assert!(suppressed(&tx, 0, 1000, 1).unwrap());
    assert!(suppressed(&tx, 6000, 10000, 1).unwrap());
}

#[test]
fn migrating_prior_task_constraint_preserves_retry_history_and_references() {
    let (db, _, _) = fixture();
    let schema: String = db
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='cloud_tasks'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let old = Connection::open_in_memory().unwrap();
    old.execute_batch(&schema.replace(",'reorganization'", ""))
        .unwrap();
    old.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE asr_inputs(task_id TEXT REFERENCES cloud_tasks(id), specification TEXT); CREATE VIEW old_tasks AS SELECT * FROM cloud_tasks; INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,state,created_ms,updated_ms,retry_failures) VALUES('paid','asr','event','event','v1','m','p',1,2,'complete',3,4,7); INSERT INTO asr_inputs VALUES('paid','unchanged');").unwrap();
    init_cloud_tasks(&old).unwrap();
    assert_eq!(
        count(&old, "SELECT retry_failures FROM old_tasks WHERE id='paid'"),
        7
    );
    assert_eq!(
        count(
            &old,
            "SELECT count(*) FROM asr_inputs JOIN cloud_tasks ON cloud_tasks.id=asr_inputs.task_id"
        ),
        1
    );
    assert_eq!(
        count(&old, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    init_cloud_tasks(&old).unwrap();
}

#[test]
#[ignore = "explicit synthetic paid LLM acceptance; reads only configured credential metadata"]
fn real_model_reorganization() {
    use std::{
        cell::RefCell,
        fs,
        path::PathBuf,
        time::{Duration, Instant},
    };
    assert_eq!(
        std::env::var("HEAR_REAL_REORGANIZATION").as_deref(),
        Ok("1")
    );
    let source = Connection::open_with_flags(
        std::env::var("HEAR_REAL_REORGANIZATION_DB").unwrap(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let reference = credential_reference(&source, CloudProvider::Llm)
        .unwrap()
        .unwrap();
    let model = model(&source, CloudProvider::Llm).unwrap().unwrap();
    let key = crate::cloud::credential(CloudProvider::Llm, &reference)
        .unwrap()
        .expect("configured credential unavailable");
    let out = PathBuf::from(std::env::var("HEAR_REAL_REORGANIZATION_OUTPUT").unwrap());
    fs::create_dir_all(&out).unwrap();
    struct Meter(RefCell<Vec<Value>>);
    impl crate::usage::Meter for Meter {
        fn begin(&self, _: &str, _: Option<Uuid>, _: Option<i64>) -> Result<String, String> {
            Ok(Uuid::new_v4().to_string())
        }
        fn llm_response(&self, _: &str, v: &Value) -> Result<(), String> {
            self.0.borrow_mut().push(v.clone());
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
    for case in [
        "same-meeting",
        "separate-tail",
        "single-event",
        "unchanged",
        "partial-source",
    ] {
        let (mut db, a, b) = fixture();
        if case == "separate-tail" {
            db.execute("UPDATE coarse_segments SET text='The product meeting is over. Now I call a friend about dinner, an independent activity.' WHERE id='00000000-0000-0000-0000-000000000002'",[]).unwrap();
        }
        if case == "single-event" || case == "unchanged" {
            db.execute("UPDATE named_events SET deleted_ms=8000 WHERE id=?1", [&b])
                .unwrap();
            db.execute(
                "UPDATE named_events SET ended_ms=6000,title='Product meeting' WHERE id=?1",
                [&a],
            )
            .unwrap();
        }
        if case == "partial-source" {
            db.execute("UPDATE named_events SET source_mask=3 WHERE id=?1", [&a])
                .unwrap();
        }
        let task = job(&mut db);
        let sent = input(&db, &task).unwrap();
        let meter = Meter(RefCell::new(Vec::new()));
        fs::write(out.join(format!("{case}-input.json")), &sent).unwrap();
        let result = crate::cloud::chat(
            &key,
            &model,
            FIT_SYSTEM,
            &sent,
            Instant::now() + Duration::from_secs(180),
            &meter,
        )
        .unwrap();
        fs::write(
            out.join(format!("{case}-provider.json")),
            serde_json::to_vec_pretty(&*meter.0.borrow()).unwrap(),
        )
        .unwrap();
        fs::write(
            out.join(format!("{case}-output.json")),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        publish_fit_with_input(&mut db, &task, &sent, result, 11000).unwrap();
        let saved:Vec<Value>=db.prepare("SELECT id,title,started_ms,ended_ms,source_mask,origin,boundary_locked,title_locked,summary FROM named_events WHERE deleted_ms IS NULL ORDER BY started_ms").unwrap().query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"start_ms":r.get::<_,i64>(2)?,"end_ms":r.get::<_,i64>(3)?,"source_mask":r.get::<_,i64>(4)?,"origin":r.get::<_,String>(5)?,"boundary_locked":r.get::<_,bool>(6)?,"title_locked":r.get::<_,bool>(7)?,"summary":r.get::<_,Option<String>>(8)?}))).unwrap().collect::<Result<_,_>>().unwrap();
        fs::write(
            out.join(format!("{case}-saved.json")),
            serde_json::to_vec_pretty(&saved).unwrap(),
        )
        .unwrap();
        assert!(!saved.is_empty(), "{case}");
        assert_eq!(count(&db, "SELECT count(*) FROM coarse_segments"), 2);
        eprintln!(
            "real_reorganization case={case} events={} PASS",
            saved.len()
        );
    }
}

#[test]
fn same_submission_receipt_enqueues_only_once() {
    let (db, _, _) = fixture();
    let mut store = crate::Store::new(db).unwrap();
    let id = Uuid::new_v4();
    let mut returned = Vec::new();
    for _ in 0..2 {
        let response = store
            .mutate_response(id, "same-payload", |tx| {
                let task_id = enqueue(
                    tx,
                    id,
                    1000,
                    5000,
                    "Product meeting",
                    EventSources::Microphone,
                    10000,
                )?;
                Ok(hear_protocol::Response::Accepted {
                    id,
                    object_id: task_id,
                })
            })
            .unwrap();
        let hear_protocol::Response::Accepted { object_id, .. } = response else {
            panic!("expected task receipt")
        };
        returned.push(object_id);
    }
    assert_eq!(returned[0], returned[1]);
    assert_eq!(
        store
            .db
            .query_row(
                "SELECT id FROM cloud_tasks WHERE target_kind='reorganization'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        returned[0].to_string()
    );
    assert_eq!(
        count(
            &store.db,
            "SELECT count(*) FROM cloud_tasks WHERE target_kind='reorganization'"
        ),
        1
    );
    assert_eq!(
        count(
            &store.db,
            "SELECT count(*) FROM event_reorganization_requests"
        ),
        1
    );
}

#[test]
fn cleanup_in_the_full_old_event_tail_invalidates_request() {
    let (mut db, a, _) = fixture();
    db.execute("UPDATE named_events SET ended_ms=10000000 WHERE id=?1", [a])
        .unwrap();
    let task = job(&mut db);
    let tx = db.transaction().unwrap();
    clear(&tx, 9000000, 10000000, 1, 11000).unwrap();
    tx.commit().unwrap();
    assert!(!task_current(&db, &task).unwrap());
}

#[test]
fn accepted_id_identifies_retryable_task_and_sources_are_projected() {
    for (sources, label) in [
        (EventSources::Microphone, "microphone"),
        (EventSources::System, "system"),
        (EventSources::Both, "both"),
    ] {
        let (mut db, _, _) = fixture();
        db.execute("INSERT INTO coarse_segments VALUES('00000000-0000-0000-0000-000000000003','block3','session','system',1000,6000,'Product meeting from the computer.','test')",[]).unwrap();
        let request = Uuid::new_v4();
        let tx = db.transaction().unwrap();
        let task = enqueue(&tx, request, 1000, 5000, "Product meeting", sources, 10000).unwrap();
        tx.commit().unwrap();
        let views = task_views(&db, CloudProvider::Llm, 10).unwrap();
        assert_eq!(views[0].id, task);
        assert_eq!(views[0].target_id, request.to_string());
        assert_eq!(serde_json::to_value(views[0].sources).unwrap(), label);
        db.execute(
            "UPDATE cloud_tasks SET state='failed' WHERE id=?1",
            [task.to_string()],
        )
        .unwrap();
        retry_task(&db, task, 11000).unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM cloud_tasks WHERE target_kind='reorganization' AND state='pending'"
            ),
            1
        );
    }
}

#[test]
fn delayed_request_uses_extended_current_event_and_preserves_other_source() {
    let (mut db, a, b) = fixture();
    db.execute("UPDATE named_events SET source_mask=3 WHERE id=?1", [&a])
        .unwrap();
    let task = job(&mut db);
    // Normal organization advances after submission, before execution/retry.
    db.execute("UPDATE named_events SET deleted_ms=12000 WHERE id=?1", [b])
        .unwrap();
    db.execute(
        "UPDATE named_events SET ended_ms=16000,revision=revision+1 WHERE id=?1",
        [&a],
    )
    .unwrap();
    db.execute("INSERT INTO coarse_segments VALUES('00000000-0000-0000-0000-000000000004','block4','session','microphone',6000,16000,'Later product meeting content, recorded after the user submitted.','test')",[]).unwrap();
    let sent = input(&db, &task).unwrap();
    assert_eq!(
        sent,
        input(&db, &task).unwrap(),
        "time alone must not stale the input"
    );
    let parsed: Value = serde_json::from_str(&sent).unwrap();
    assert!(parsed["candidate"]["end_ms"].as_i64().unwrap() >= 16000);
    assert!(sent.contains("Later product meeting content"));
    let result = json!({"events":[event(Some(&a),1000,16000,"system","First half"),event(None,1000,16000,"microphone","Product meeting")],"retire_event_ids":[]});
    publish_fit_with_input(&mut db, &task, &sent, result, 20000).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT ended_ms FROM named_events WHERE id=?1 AND source_mask=2",
            [a],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        16000
    );
}
