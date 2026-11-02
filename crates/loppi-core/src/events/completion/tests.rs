use super::*;
use serde_json::{Value, json};

fn fixture() -> (Connection, Uuid) {
    let db = crate::Store::new(Connection::open_in_memory().unwrap())
        .unwrap()
        .db;
    set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
    set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
    for (a, b, text) in [
        (1000, 3000, "Discuss interface"),
        (3000, 6000, "Agree delivery"),
    ] {
        db.execute(
            "INSERT INTO coarse_segments VALUES(?1,'block','session','microphone',?2,?3,?4,'test')",
            params![Uuid::new_v4().to_string(), a, b, text],
        )
        .unwrap();
    }
    let id = Uuid::new_v4();
    db.execute("INSERT INTO named_events(id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,created_ms,summary) VALUES(?1,1000,6000,'Meeting',1,1,'automatic',0,0,1,6000,'Existing summary')",[id.to_string()]).unwrap();
    db.execute(
        "INSERT INTO event_asr_state VALUES(?1,NULL)",
        [id.to_string()],
    )
    .unwrap();
    (db, id)
}
fn block(db: &Connection, id: &str, a: i64, b: i64) {
    db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active) VALUES(?1,'session','microphone',?2,?3,0,?4,100,?5,'retain','complete',1)",params![id,a,b,(b-a)*48,format!("media/{id}.opus")]).unwrap();
}
fn count(db: &Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |r| r.get(0)).unwrap()
}
fn confirm_at(db: &mut Connection, id: Uuid, start: i64, end: i64) -> Uuid {
    let tx = db.transaction().unwrap();
    let result = confirm(&tx, Uuid::new_v4(), id, 1, start, end, 10000).unwrap();
    tx.commit().unwrap();
    result
}
fn output(id: Option<Uuid>, a: i64, b: i64) -> Value {
    json!({"event_id":id,"start_ms":a,"end_ms":b,"title":"Updated meeting","sources":"microphone","merge_with_event_id":null,"summary":"Updated full summary","understanding":{"topics":[],"key_points":[],"mentioned_objects":[]}})
}

#[test]
fn unchanged_confirmation_preserves_llm_and_asr_and_plans_late_audio_once() {
    let (mut db, id) = fixture();
    block(&db, "first", 1000, 3000);
    confirm_at(&mut db, id, 1000, 6000);
    assert_eq!(completed_at(&db, &id.to_string()).unwrap(), Some(10000));
    assert_eq!(
        event_detail(&db, id, 0, 30).unwrap().summary.as_deref(),
        Some("Existing summary")
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='llm'"),
        0
    );
    let original: String = db
        .query_row("SELECT id FROM cloud_tasks WHERE provider='asr'", [], |r| {
            r.get(0)
        })
        .unwrap();
    block(&db, "late", 3000, 6000);
    assert_eq!(
        event_detail(&db, id, 0, 30).unwrap().processing_state,
        "pending_asr"
    );
    asr::refresh(&mut db, 10001).unwrap();
    asr::refresh(&mut db, 10002).unwrap();
    assert_eq!(
        count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT state FROM cloud_tasks WHERE id=?1",
            [original],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "pending"
    );
    assert_eq!(count(&db, "SELECT count(*) FROM event_asr_refs"), 2);
}

#[test]
fn either_changed_endpoint_is_a_reference_and_all_model_outputs_are_completed() {
    for (start, end) in [(2000, 6000), (1000, 7000)] {
        let (mut db, id) = fixture();
        block(&db, "all", 1000, 6000);
        confirm_at(&mut db, id, start, end);
        assert!(completed_at(&db, &id.to_string()).unwrap().is_none());
        assert_eq!(
            task_state(&db, &id.to_string()).unwrap().as_deref(),
            Some("pending")
        );
        assert_eq!(
            count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
            0
        );
        let task = take_task(&mut db, CloudProvider::Llm, 10001)
            .unwrap()
            .unwrap();
        let input = llm_input(&db, &task).unwrap();
        let sent: Value = serde_json::from_str(&input).unwrap();
        assert_eq!(sent["user_instruction"]["times_adjusted_by_user"], true);
        assert_eq!(
            sent["user_instruction"]["completion_event_id"],
            id.to_string()
        );
        assert_eq!(sent["user_instruction"]["start_ms"], start);
        assert_eq!(sent["user_instruction"]["end_ms"], end);
        publish_fit_with_input(&mut db,&task,&input,json!({"events":[output(Some(id),1000,3000),output(None,3000,6000)],"retire_event_ids":[]}),11000).unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM named_events WHERE completed_ms=11000 AND deleted_ms IS NULL"
            ),
            2
        );
        assert_eq!(
            count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
            2
        );
        asr::refresh(&mut db, 11001).unwrap();
        assert_eq!(
            count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
            2
        );
    }
}

#[test]
fn no_change_model_result_also_completes_original_and_reuses_existing_audio_tasks() {
    let (mut db, id) = fixture();
    block(&db, "all", 1000, 6000);
    {
        let tx = db.transaction().unwrap();
        queue_asr(
            &tx,
            "event",
            &id.to_string(),
            &asr_version(1, 1000, 6000),
            1000,
            6000,
            1,
            true,
            8000,
        )
        .unwrap();
        tx.commit().unwrap();
    }
    confirm_at(&mut db, id, 1000, 7000);
    let task = take_task(&mut db, CloudProvider::Llm, 10001)
        .unwrap()
        .unwrap();
    publish_fit(
        &mut db,
        &task,
        json!({"events":[],"retire_event_ids":[]}),
        11000,
    )
    .unwrap();
    assert!(completed_at(&db, &id.to_string()).unwrap().is_some());
    assert_eq!(
        count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
        1
    );
}

#[test]
fn stale_or_invalid_model_result_does_not_half_publish_or_finish() {
    for stale in [false, true] {
        let (mut db, id) = fixture();
        confirm_at(&mut db, id, 1000, 7000);
        let task = take_task(&mut db, CloudProvider::Llm, 10001)
            .unwrap()
            .unwrap();
        let sent = llm_input(&db, &task).unwrap();
        if stale {
            db.execute(
                "UPDATE named_events SET title='Human correction',revision=revision+1 WHERE id=?1",
                [id.to_string()],
            )
            .unwrap();
        }
        assert!(
            publish_fit_with_input(
                &mut db,
                &task,
                &sent,
                json!({"events":[output(Some(id),6000,1000)],"retire_event_ids":[]}),
                11000
            )
            .is_err()
        );
        assert!(completed_at(&db, &id.to_string()).unwrap().is_none());
        assert_eq!(
            event_detail(&db, id, 0, 30).unwrap().summary.as_deref(),
            Some("Existing summary")
        );
        assert_eq!(
            count(&db, "SELECT count(*) FROM cloud_tasks WHERE provider='asr'"),
            0
        );
    }
}

#[test]
fn explicit_retry_accepts_latest_revision_but_rejects_old_results_and_deleted_targets() {
    for deleted in [false, true] {
        let (mut db, id) = fixture();
        confirm_at(&mut db, id, 1000, 7000);
        let original = take_task(&mut db, CloudProvider::Llm, 10001)
            .unwrap()
            .unwrap();
        let old_input = llm_input(&db, &original).unwrap();
        db.execute(
            "UPDATE named_events SET title='Human correction',revision=revision+1 WHERE id=?1",
            [id.to_string()],
        )
        .unwrap();
        let result = json!({"events":[output(Some(id),1000,6000)],"retire_event_ids":[]});
        assert_eq!(
            publish_fit_with_input(&mut db, &original, &old_input, result.clone(), 11000)
                .unwrap_err(),
            "reorganization_revision_conflict"
        );
        task_failed(&db, &original, "reorganization_revision_conflict", 11001).unwrap();
        if deleted {
            db.execute(
                "UPDATE named_events SET deleted_ms=11002,revision=revision+1 WHERE id=?1",
                [id.to_string()],
            )
            .unwrap();
            assert!(retry_task(&db, original.id, 12000).is_err());
            assert_eq!(
                task_state(&db, &id.to_string()).unwrap().as_deref(),
                Some("failed")
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT count(*) FROM named_events WHERE completed_ms IS NOT NULL"
                ),
                0
            );
            continue;
        }
        assert_eq!(
            event_detail(&db, id, 0, 30).unwrap().title.as_deref(),
            Some("Human correction")
        );
        retry_task(&db, original.id, 12000).unwrap();
        let retry = take_task(&mut db, CloudProvider::Llm, 12001)
            .unwrap()
            .unwrap();
        let latest = llm_input(&db, &retry).unwrap();
        assert_ne!(old_input, latest);
        let latest_value: Value = serde_json::from_str(&latest).unwrap();
        assert_eq!(
            latest_value["context"]["linked_events"][0]["title"],
            "Human correction"
        );
        assert_eq!(latest_value["context"]["linked_events"][0]["revision"], 2);
        assert!(
            publish_fit_with_input(&mut db, &original, &old_input, result.clone(), 12002).is_err()
        );
        assert!(completed_at(&db, &id.to_string()).unwrap().is_none());
        publish_fit_with_input(&mut db, &retry, &latest, result, 13000).unwrap();
        assert_eq!(completed_at(&db, &id.to_string()).unwrap(), Some(13000));
        assert_eq!(
            task_state(&db, &id.to_string()).unwrap().as_deref(),
            Some("complete")
        );
    }
}

#[test]
fn automatic_old_slot_and_continuation_cannot_change_completed_event_but_tail_can_be_named() {
    let (mut db, id) = fixture();
    refresh_candidates(&mut db, 100000, true).unwrap();
    let task = take_task(&mut db, CloudProvider::Llm, 100001)
        .unwrap()
        .unwrap();
    db.execute(
        "INSERT INTO event_candidate_links VALUES(?1,0,?2)",
        params![task.target_id, id.to_string()],
    )
    .unwrap();
    let sent = llm_input(&db, &task).unwrap();
    confirm_at(&mut db, id, 1000, 6000);
    publish_fit_with_input(&mut db,&task,&sent,json!({"events":[{"start_ms":1000,"end_ms":6000,"title":"Late overwrite","sources":"microphone","summary":"Wrong overwrite","merge_with_event_id":id}]}),11000).unwrap();
    let event = event_detail(&db, id, 0, 30).unwrap();
    assert_eq!(event.title.as_deref(), Some("Meeting"));
    assert_eq!(event.summary.as_deref(), Some("Existing summary"));
    db.execute("INSERT INTO coarse_segments VALUES(?1,'b','session','microphone',6000,9000,'A separate next activity','test')",[Uuid::new_v4().to_string()]).unwrap();
    refresh_candidates(&mut db, 100000, true).unwrap();
    let task = take_task(&mut db, CloudProvider::Llm, 100001)
        .unwrap()
        .unwrap();
    let input = llm_input(&db, &task).unwrap();
    let sent: Value = serde_json::from_str(&input).unwrap();
    assert!(
        sent["context"]["previous_events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        sent["context"]["linked_events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        sent["observations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["start_ms"].as_i64().unwrap() >= 6000)
    );
    publish_fit_with_input(&mut db,&task,&input,json!({"events":[{"start_ms":6000,"end_ms":9000,"title":"Next activity","sources":"microphone","summary":null,"merge_with_event_id":id}]}),12000).unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL"
        ),
        2
    );
    assert_eq!(event_detail(&db, id, 0, 30).unwrap().ended_ms, 6000);
}

#[test]
fn duplicate_receipt_restart_and_invalid_times_follow_existing_transaction_contract() {
    let (db, id) = fixture();
    let mut store = crate::Store::new(db).unwrap();
    let request = Uuid::new_v4();
    for _ in 0..2 {
        store
            .mutate_response(request, "same completion", |tx| {
                let object_id = confirm(tx, request, id, 1, 1000, 7000, 10000)?;
                Ok(loppi_protocol::Response::Accepted {
                    id: request,
                    object_id,
                })
            })
            .unwrap();
    }
    assert_eq!(
        count(
            &store.db,
            "SELECT count(*) FROM event_reorganization_requests"
        ),
        1
    );
    let mut db = crate::Store::new(store.db).unwrap().db;
    assert_eq!(
        task_state(&db, &id.to_string()).unwrap().as_deref(),
        Some("pending")
    );
    let tx = db.transaction().unwrap();
    assert!(confirm(&tx, Uuid::new_v4(), id, 1, 1000, 11000, 10000).is_err());
    assert!(confirm(&tx, Uuid::new_v4(), id, 99, 1000, 6000, 10000).is_err());
}

#[test]
fn completion_reorganization_cannot_modify_another_completed_event() {
    let (mut db, id) = fixture();
    let other = create_event(
        &mut db,
        6000,
        9000,
        "Already confirmed",
        EventSources::Microphone,
        10000,
    )
    .unwrap();
    {
        let tx = db.transaction().unwrap();
        finish(&tx, &other.to_string(), 10000).unwrap();
        tx.commit().unwrap();
    }
    confirm_at(&mut db, id, 1000, 7000);
    let task = take_task(&mut db, CloudProvider::Llm, 10001)
        .unwrap()
        .unwrap();
    assert!(
        publish_fit(
            &mut db,
            &task,
            json!({"events":[output(Some(other),6000,9000)],"retire_event_ids":[]}),
            11000
        )
        .is_err()
    );
    assert!(completed_at(&db, &id.to_string()).unwrap().is_none());
    assert_eq!(
        event_detail(&db, other, 0, 30).unwrap().title.as_deref(),
        Some("Already confirmed")
    );
}
