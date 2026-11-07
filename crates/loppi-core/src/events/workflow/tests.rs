use super::*;
use loppi_protocol::workflow::Tag;

const START: i64 = 1_000;
const END: i64 = 6_000;
const NOW: i64 = 20_000;

fn database() -> Connection {
    let db = crate::Store::new(Connection::open_in_memory().unwrap())
        .unwrap()
        .db;
    set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
    set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
    db
}

fn set(db: &Connection, config: Config, rev: i64, add: bool) -> Value {
    configure(db, config, rev, false, add).unwrap()
}

fn settings(db: &Connection, input: Input, tagged: bool) -> (Uuid, Uuid) {
    assert_eq!(
        set(
            db,
            Config {
                tags: vec![Tag {
                    id: Uuid::nil(),
                    name: "工作会议".into(),
                    criteria: "明确讨论项目安排".into()
                }],
                ..Config::default()
            },
            0,
            false
        )["ok"],
        true
    );
    let (_, mut config) = configuration(db).unwrap();
    let tag = config.tags[0].id;
    config.auto_asr = AutoAsr::Tags { tag_ids: vec![tag] };
    config.strategies.push(Strategy {
        id: Uuid::nil(),
        name: "纪要".into(),
        prompt: "提取待办".into(),
        input,
        tag_ids: if tagged { vec![tag] } else { vec![] },
    });
    assert_eq!(set(db, config, 1, false)["ok"], true);
    (tag, configuration(db).unwrap().1.strategies[0].id)
}

fn block(db: &Connection) {
    db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
                VALUES('block','session','microphone',?1,?2,0,?3,100,'media/block.opus','retain','complete',1)",
        params![START,END,(END-START)*48]).unwrap();
    db.execute("INSERT INTO coarse_segments VALUES(?1,'block','session','microphone',?2,?3,'确认周四交付','test')",
        params![Uuid::new_v4().to_string(),START,END]).unwrap();
}

fn event(db: &mut Connection, tags: Value) -> Uuid {
    event_with_sources(db, tags, 1)
}

fn event_with_sources(db: &mut Connection, tags: Value, mask: i64) -> Uuid {
    let tx = db.transaction().unwrap();
    let id = Uuid::new_v4();
    insert_generated_event(
        &tx,
        &id.to_string(),
        START,
        END,
        mask,
        "会议",
        None,
        None,
        END,
    )
    .unwrap();
    let input = json!({"classification":classification_context(&tx).unwrap()});
    observe(&tx, &id.to_string(), &input, &json!({"tag_ids":tags})).unwrap();
    tx.commit().unwrap();
    id
}

fn change(db: &mut Connection, action: Action, now: i64) -> Value {
    let tx = db.transaction().unwrap();
    let value = write(&tx, action, Uuid::new_v4(), now).unwrap();
    if value["ok"] == true {
        tx.commit().unwrap();
    }
    value
}

fn complete(db: &mut Connection, id: Uuid) {
    assert_eq!(
        change(
            db,
            Action::Complete {
                event_id: id,
                expected_revision: 1,
                started_ms: None,
                ended_ms: None
            },
            NOW
        )["ok"],
        true
    );
}

fn run(id: Uuid, strategy: Uuid, input: ManualInput) -> Action {
    Action::Run {
        event_id: id,
        strategy_id: strategy,
        expected_revision: 2,
        config_revision: 2,
        input,
    }
}

fn counts(db: &Connection) -> (i64, i64) {
    db.query_row(
        "SELECT sum(provider='asr'),sum(provider='llm' AND target_kind='workflow')
        FROM cloud_tasks WHERE state!='superseded'",
        [],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
            ))
        },
    )
    .unwrap()
}

fn confirm_historical(db: &mut Connection) -> Uuid {
    let result = change(
        db,
        Action::ConfirmRange {
            started_ms: START,
            ended_ms: END,
            sources: EventSources::Microphone,
            title: None,
        },
        NOW,
    );
    assert_eq!(result["data"]["state"]["phase"], "judging");
    Uuid::parse_str(result["data"]["event_id"].as_str().unwrap()).unwrap()
}

#[test]
fn historical_confirmation_organizes_even_without_custom_tags() {
    let mut db = database();
    block(&db);
    let id = confirm_historical(&mut db);
    let task = take_task(&mut db, CloudProvider::Llm, NOW + 1)
        .unwrap()
        .unwrap();
    let sent = rebuild::input(&db, &task).unwrap();
    let input: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(input["user_can_name"], true);
    assert_eq!(task.target_id, id.to_string());
    let output = json!({"events":[{"start_ms":START,"end_ms":END,"title":"交付排期",
        "summary":"确认周四交付。","understanding":{"topics":[],"key_points":[],"mentioned_objects":[]}}]});
    assert_eq!(
        rebuild::publish(&mut db, &task, &sent, output, NOW + 2).unwrap(),
        1
    );
    let title: String = db
        .query_row(
            "SELECT title FROM named_events WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "交付排期");
    let state = status(&db, &id.to_string(), NOW + 2).unwrap().unwrap();
    assert_eq!(state.phase, "decided");
    assert_eq!(state.labels[0].id, OTHER_TAG);
    assert_eq!(counts(&db), (1, 0));
}

#[test]
fn changing_classifier_during_historical_organization_preserves_summary_and_retries_tags() {
    let mut db = database();
    let (tag, _) = settings(&db, Input::Coarse, true);
    block(&db);
    let id = confirm_historical(&mut db);
    let task = take_task(&mut db, CloudProvider::Llm, NOW + 1)
        .unwrap()
        .unwrap();
    let sent = rebuild::input(&db, &task).unwrap();
    let (revision, mut config) = configuration(&db).unwrap();
    config.tags[0].criteria = "讨论交付安排".into();
    assert_eq!(set(&db, config, revision, false)["ok"], true);
    let output = json!({"events":[{"start_ms":START,"end_ms":END,"title":"交付排期",
        "summary":"确认周四交付。","tag_ids":[tag],
        "understanding":{"topics":[],"key_points":[],"mentioned_objects":[]}}]});
    assert_eq!(
        rebuild::publish(&mut db, &task, &sent, output.clone(), NOW + 2).unwrap(),
        1
    );
    let summary: String = db
        .query_row(
            "SELECT summary FROM named_events WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(summary, "确认周四交付。");
    assert_eq!(counts(&db), (0, 0));
    let state = status(&db, &id.to_string(), NOW + 2).unwrap().unwrap();
    assert_eq!(state.tag_state, "pending");
    assert!(state.labels.is_empty());
    let replacement = take_task(&mut db, CloudProvider::Llm, NOW + 3)
        .unwrap()
        .unwrap();
    assert_ne!(task.id, replacement.id);
    let sent = rebuild::input(&db, &replacement).unwrap();
    assert_eq!(
        rebuild::publish(&mut db, &replacement, &sent, output, NOW + 4).unwrap(),
        1
    );
    assert_eq!(counts(&db), (1, 1));
}

#[test]
fn configuration_save_and_reverse_validation_are_atomic() {
    let db = database();
    let (tag, _) = settings(&db, Input::Precise, true);
    let (revision, mut config) = configuration(&db).unwrap();
    config.auto_asr = AutoAsr::Tags {
        tag_ids: vec![OTHER_TAG],
    };
    let denied = set(&db, config.clone(), revision, false);
    assert_eq!(denied["error"]["code"], "asr_rule_conflict");
    assert_eq!(
        denied["error"]["details"]["affected"][0]["tag_ids"],
        json!([tag])
    );
    assert_eq!(configuration(&db).unwrap().0, revision);
    assert_eq!(set(&db, config, revision, true)["ok"], true);
    assert_eq!(
        configuration(&db).unwrap().1.auto_asr,
        AutoAsr::Tags {
            tag_ids: vec![OTHER_TAG, tag]
        }
    );
}

#[test]
fn model_tags_follow_the_mode_and_keep_manual_labels() {
    let mut db = database();
    let (first, _) = settings(&db, Input::Coarse, true);
    let (revision, mut config) = configuration(&db).unwrap();
    config.tags.push(Tag {
        id: Uuid::nil(),
        name: "规划".into(),
        criteria: "确定交付计划".into(),
    });
    assert_eq!(set(&db, config, revision, false)["ok"], true);
    let (revision, mut config) = configuration(&db).unwrap();
    let second = config.tags[1].id;
    let wrong = event(&mut db, json!([first, second]));
    assert_eq!(
        status(&db, &wrong.to_string(), NOW)
            .unwrap()
            .unwrap()
            .tag_state,
        "failed"
    );
    config.mode = TagMode::Multiple;
    config.strategies[0].tag_ids.push(second);
    assert_eq!(set(&db, config, revision, false)["ok"], true);
    let context = json!({"classification":classification_context(&db).unwrap()});
    assert!(classification_system("整理", &context).contains("确定交付计划"));
    let id = event(&mut db, json!([first, second]));
    block(&db);
    complete(&mut db, id);
    assert_eq!(counts(&db), (1, 1));
    let action = |plan| Action::Tags {
        event_id: id,
        expected_revision: 2,
        tag_ids: vec![OTHER_TAG],
        plan,
    };
    let preview = read(&db, &action(None), NOW).unwrap();
    assert_eq!(
        change(
            &mut db,
            action(Some(preview["data"]["plan"].as_str().unwrap().into())),
            NOW
        )["ok"],
        true
    );
    let tx = db.transaction().unwrap();
    observe(
        &tx,
        &id.to_string(),
        &context,
        &json!({"tag_ids":"格式错误"}),
    )
    .unwrap();
    tx.commit().unwrap();
    let state = status(&db, &id.to_string(), NOW).unwrap().unwrap();
    assert!(state.manual);
    assert_eq!(state.labels[0].id, OTHER_TAG);
}

#[test]
fn confirmed_and_naturally_settled_events_use_the_same_label_rules() {
    for explicit in [true, false] {
        let mut db = database();
        let (tag, _) = settings(&db, Input::Coarse, true);
        block(&db);
        let id = event(&mut db, json!([tag]));
        assert_eq!(
            status(&db, &id.to_string(), NOW).unwrap().unwrap().phase,
            "undetermined"
        );
        if explicit {
            complete(&mut db, id);
        } else {
            asr::refresh(&mut db, 5_000_000).unwrap();
        }
        assert_eq!(counts(&db), (1, 1));
        assert_eq!(
            status(&db, &id.to_string(), 5_000_001)
                .unwrap()
                .unwrap()
                .phase,
            "decided"
        );
        assert_eq!(
            db.query_row(
                "SELECT manual FROM cloud_tasks WHERE provider='asr'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
            false
        );
    }
}

#[test]
fn two_manual_inputs_share_one_asr_and_keep_result_versions() {
    let mut db = database();
    let (_, strategy) = settings(&db, Input::Precise, false);
    block(&db);
    let id = event(&mut db, json!([OTHER_TAG]));
    complete(&mut db, id);
    assert_eq!(counts(&db), (0, 0));
    assert_eq!(
        change(&mut db, run(id, strategy, ManualInput::Configured), NOW)["error"]["code"],
        "precise_input_unavailable"
    );
    let coarse = change(&mut db, run(id, strategy, ManualInput::Coarse), NOW + 1);
    let coarse_id = coarse["data"]["run_id"].as_str().unwrap();
    let task = take_task(&mut db, CloudProvider::Llm, NOW + 2)
        .unwrap()
        .unwrap();
    let (_, input) = task_input(&db, &task).unwrap();
    assert!(input.contains("确认周四交付"));
    assert_eq!(
        publish(
            &mut db,
            &task,
            json!({"body_markdown":"无效输出","actions":["execute"]}),
            NOW + 3
        )
        .unwrap_err(),
        "invalid_workflow_output"
    );
    publish(&mut db, &task, json!({"body_markdown":"粗转稿"}), NOW + 3).unwrap();
    let precise = change(
        &mut db,
        run(id, strategy, ManualInput::PreparePrecise),
        NOW + 4,
    );
    let repeat = change(
        &mut db,
        run(id, strategy, ManualInput::PreparePrecise),
        NOW + 5,
    );
    assert_eq!(precise["data"]["run_id"], repeat["data"]["run_id"]);
    assert_eq!(repeat["data"]["reused"], true);
    assert_eq!(counts(&db), (1, 1));

    let asr_task = take_task(&mut db, CloudProvider::Asr, NOW + 6)
        .unwrap()
        .unwrap();
    publish_asr(
        &mut db,
        &asr_task,
        &[crate::cloud::AsrUtterance {
            start_ms: 0,
            end_ms: 1000,
            text: "确认周四交付，负责人是李明".into(),
            speaker: None,
            words: None,
        }],
        NOW + 7,
    )
    .unwrap();
    let task = take_task(&mut db, CloudProvider::Llm, NOW + 8)
        .unwrap()
        .unwrap();
    assert!(task_input(&db, &task).unwrap().1.contains("负责人是李明"));
    publish(&mut db, &task, json!({"body_markdown":"精转稿"}), NOW + 9).unwrap();
    let scope = scope(&db, &id.to_string()).unwrap().unwrap();
    assert_eq!(
        read_result(&db, &scope, strategy, None, None).unwrap()["body_markdown"],
        "精转稿"
    );
    assert_eq!(
        read_result(
            &db,
            &scope,
            strategy,
            Some(Uuid::parse_str(coarse_id).unwrap()),
            None
        )
        .unwrap()["body_markdown"],
        "粗转稿"
    );
    let tx = db.transaction().unwrap();
    clear(&tx, START, END, 1, NOW + 10).unwrap();
    tx.commit().unwrap();
    assert!(read_result(&db, &scope, strategy, None, None).is_err());
    publish(&mut db, &task, json!({"body_markdown":"迟到"}), NOW + 11).unwrap();
    assert!(read_result(&db, &scope, strategy, None, None).is_err());
}

#[test]
fn late_audio_invalidates_generation_until_precise_coverage_is_current() {
    for asr_completes_first in [false, true] {
        verify_late_audio(asr_completes_first);
    }
}

fn verify_late_audio(asr_completes_first: bool) {
    const MID: i64 = 3_500;
    let mut db = database();
    let (_, strategy) = settings(&db, Input::Precise, false);
    block(&db);
    db.execute(
        "UPDATE audio_blocks SET ended_ms=?1,frames=?2 WHERE id='block'",
        params![MID, (MID - START) * 48],
    )
    .unwrap();
    let id = event(&mut db, json!([OTHER_TAG]));
    complete(&mut db, id);
    let run = change(
        &mut db,
        run(id, strategy, ManualInput::PreparePrecise),
        NOW + 1,
    );
    let run_id = run["data"]["run_id"].as_str().unwrap();
    let words = |text: &str| crate::cloud::AsrUtterance {
        start_ms: 0,
        end_ms: 1_000,
        text: text.into(),
        speaker: None,
        words: None,
    };
    let first = take_task(&mut db, CloudProvider::Asr, NOW + 2)
        .unwrap()
        .unwrap();
    publish_asr(&mut db, &first, &[words("第一段")], NOW + 3).unwrap();
    let original = take_task(&mut db, CloudProvider::Llm, NOW + 4)
        .unwrap()
        .unwrap();
    assert_eq!(original.target_id, run_id);
    db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
                VALUES('late','session','microphone',?1,?2,?3,?4,100,'media/late.opus','retain','complete',1)",
        params![MID,END,(MID-START)*48,(END-MID)*48]).unwrap();
    assert!(!task_current(&db, &original).unwrap());
    let late = take_task(&mut db, CloudProvider::Asr, NOW + 5)
        .unwrap()
        .unwrap();
    if asr_completes_first {
        publish_asr(&mut db, &late, &[words("第二段")], NOW + 6).unwrap();
        assert!(!task_current(&db, &original).unwrap());
    }
    publish(
        &mut db,
        &original,
        json!({"body_markdown":"旧覆盖"}),
        NOW + 7,
    )
    .unwrap();
    let scope = scope(&db, &id.to_string()).unwrap().unwrap();
    assert!(read_result(&db, &scope, strategy, None, None).is_err());
    assert_eq!(
        run_metadata(&db, &scope, run_id).unwrap()["state"],
        "waiting_precise"
    );
    if !asr_completes_first {
        publish_asr(&mut db, &late, &[words("第二段")], NOW + 7).unwrap();
    }
    let current = take_task(&mut db, CloudProvider::Llm, NOW + 8)
        .unwrap()
        .unwrap();
    assert_eq!(current.target_id, run_id);
    let input = task_input(&db, &current).unwrap().1;
    assert!(input.contains("第一段") && input.contains("第二段"));
    publish(
        &mut db,
        &current,
        json!({"body_markdown":"两段都到齐"}),
        NOW + 9,
    )
    .unwrap();
    assert_eq!(
        read_result(&db, &scope, strategy, None, None).unwrap()["body_markdown"],
        "两段都到齐"
    );
}

#[test]
fn a_fixed_manual_range_requires_its_whole_visible_scope() {
    let mut db = database();
    block(&db);
    let range = transcribe_range(&mut db, START, END, EventSources::Microphone, NOW).unwrap();
    assert!(scope(&db, &range.to_string()).unwrap().is_some());
    let clipped = range_detail(&db, START + 100, END, EventSources::Microphone, 0, 10).unwrap();
    assert_eq!(clipped.manual_range_id, Some(range));
    assert!(clipped.workflow.is_none());
    create_event(
        &mut db,
        2_000,
        4_000,
        "中间事件",
        EventSources::Microphone,
        NOW + 1,
    )
    .unwrap();
    assert!(scope(&db, &range.to_string()).unwrap().is_none());
    let visible = range_detail(&db, 4_000, END, EventSources::Microphone, 0, 10).unwrap();
    assert_eq!(visible.manual_range_id, Some(range));
    assert!(visible.workflow.is_none());
}

#[test]
fn partial_precise_only_waits_for_a_recoverable_missing_source() {
    for clipped in [false, true] {
        let mut db = database();
        let (_, strategy) = settings(&db, Input::Precise, false);
        block(&db);
        db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
            VALUES('sys','session','system',?1,?2,0,?3,100,'media/sys.opus','retain','complete',1)", params![START,END,(END-START)*48]).unwrap();
        let id = event_with_sources(&mut db, json!([OTHER_TAG]), 3);
        complete(&mut db, id);
        change(
            &mut db,
            run(id, strategy, ManualInput::PreparePrecise),
            NOW + 1,
        );
        for index in 0..2 {
            let task = take_task(&mut db, CloudProvider::Asr, NOW + 2 + index * 2)
                .unwrap()
                .unwrap();
            let has_text =
                task.source.as_deref() == Some(if clipped { "system" } else { "microphone" });
            let mut utterances = if clipped || has_text {
                vec![crate::cloud::AsrUtterance {
                    start_ms: 0,
                    end_ms: 1_000,
                    text: if has_text {
                        "可用文字"
                    } else {
                        "跨边界"
                    }
                    .into(),
                    speaker: None,
                    words: None,
                }]
            } else {
                Vec::new()
            };
            if clipped && !has_text {
                utterances.push(crate::cloud::AsrUtterance {
                    start_ms: 1_600,
                    end_ms: 2_400,
                    text: "同一任务的有效内段".into(),
                    speaker: None,
                    words: None,
                });
            }
            publish_asr(&mut db, &task, &utterances, NOW + 3 + index * 2).unwrap();
            if clipped && !has_text {
                db.execute(
                    "UPDATE event_asr_refs SET started_ms=?2 WHERE task_id=?1",
                    params![task.id.to_string(), START + 500],
                )
                .unwrap();
            }
        }
        let scope = scope(&db, &id.to_string()).unwrap().unwrap();
        assert!(!ready_precise(&db, &scope).unwrap());
        db.execute("UPDATE audio_blocks SET media_path=NULL WHERE id='sys'", [])
            .unwrap();
        if clipped {
            db.execute("UPDATE audio_blocks SET ended_ms=2500,frames=(2500-started_ms)*48 WHERE id='block'",[]).unwrap();
            assert!(!ready_precise(&db, &scope).unwrap());
            assert_eq!(
                change(
                    &mut db,
                    run(id, strategy, ManualInput::PreparePrecise),
                    NOW + 7
                )["ok"],
                true
            );
            let preserved = transcript(&db, &scope, Input::Precise).unwrap();
            assert!(preserved.contains("可用文字"));
            assert!(preserved.contains("同一任务的有效内段"));
            assert!(!preserved.contains("跨边界"));
            let next = take_task(&mut db, CloudProvider::Asr, NOW + 8)
                .unwrap()
                .unwrap();
            assert_eq!(next.source.as_deref(), Some("microphone"));
        } else {
            assert!(ready_precise(&db, &scope).unwrap());
            let next = take_task(&mut db, CloudProvider::Llm, NOW + 7)
                .unwrap()
                .unwrap();
            assert!(task_input(&db, &next).unwrap().1.contains("可用文字"));
        }
    }
}

#[test]
fn stale_label_selection_does_not_apply_when_the_catalog_changes() {
    let mut db = database();
    let (tag, _) = settings(&db, Input::Coarse, false);
    let id = event(&mut db, json!([OTHER_TAG]));
    complete(&mut db, id);
    let action = |plan| Action::Tags {
        event_id: id,
        expected_revision: 2,
        tag_ids: vec![tag],
        plan,
    };
    let preview = read(&db, &action(None), NOW + 1).unwrap();
    let (rev, mut config) = configuration(&db).unwrap();
    config.tags.clear();
    config.auto_asr = AutoAsr::All;
    assert_eq!(set(&db, config, rev, false)["ok"], true);
    let denied = change(
        &mut db,
        action(Some(preview["data"]["plan"].as_str().unwrap().to_owned())),
        NOW + 2,
    );
    assert_eq!(denied["error"]["code"], "tag_selection_stale");
    assert_eq!(denied["error"]["details"]["revision"], 2);
    assert!(
        !status(&db, &id.to_string(), NOW + 2)
            .unwrap()
            .unwrap()
            .manual
    );
}

#[test]
fn legacy_event_previews_new_asr_before_saving_tags() {
    let mut db = database();
    block(&db);
    let id = Uuid::new_v4();
    let tx = db.transaction().unwrap();
    insert_generated_event(
        &tx,
        &id.to_string(),
        START,
        END,
        1,
        "旧会议",
        None,
        None,
        END,
    )
    .unwrap();
    tx.execute(
        "UPDATE named_events SET completed_ms=?2,boundary_locked=1 WHERE id=?1",
        params![id.to_string(), END],
    )
    .unwrap();
    tx.commit().unwrap();
    let action = |plan| Action::Tags {
        event_id: id,
        expected_revision: 1,
        tag_ids: vec![OTHER_TAG],
        plan,
    };
    let preview = read(&db, &action(None), NOW).unwrap();
    assert_eq!(
        preview["data"]["actions"],
        json!([{"kind":"asr","reuse":false,"may_add":true}])
    );
    assert_eq!(counts(&db), (0, 0));
    let applied = change(
        &mut db,
        action(Some(preview["data"]["plan"].as_str().unwrap().into())),
        NOW + 1,
    );
    assert_eq!(applied["data"]["actions"], preview["data"]["actions"]);
    assert_eq!(counts(&db), (1, 0));
}

#[test]
fn partial_asr_coverage_is_not_previewed_as_reuse_only() {
    let mut db = database();
    block(&db);
    db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
        VALUES('sys','session','system',?1,?2,0,?3,100,'media/sys.opus','retain','complete',1)", params![START,END,(END-START)*48]).unwrap();
    let id = Uuid::new_v4();
    let tx = db.transaction().unwrap();
    insert_generated_event(
        &tx,
        &id.to_string(),
        START,
        END,
        3,
        "双声源旧会议",
        None,
        None,
        END,
    )
    .unwrap();
    tx.execute(
        "UPDATE named_events SET completed_ms=?2,boundary_locked=1 WHERE id=?1",
        params![id.to_string(), END],
    )
    .unwrap();
    let version = asr_version(3, START, END);
    asr::schedule(
        &tx,
        "event",
        &id.to_string(),
        &version,
        START,
        END,
        1,
        false,
        NOW,
        asr::VERSION,
    )
    .unwrap();
    asr::plan_event(
        &tx,
        &id.to_string(),
        &version,
        START,
        END,
        1,
        false,
        NOW,
        asr::VERSION,
    )
    .unwrap();
    tx.commit().unwrap();
    let action = |plan| Action::Tags {
        event_id: id,
        expected_revision: 1,
        tag_ids: vec![OTHER_TAG],
        plan,
    };
    let preview = read(&db, &action(None), NOW).unwrap();
    assert_eq!(
        preview["data"]["actions"],
        json!([{"kind":"asr","reuse":true,"may_add":true}])
    );
    assert_eq!(counts(&db), (1, 0));
    let applied = change(
        &mut db,
        action(Some(preview["data"]["plan"].as_str().unwrap().into())),
        NOW + 1,
    );
    assert_eq!(applied["data"]["actions"], preview["data"]["actions"]);
    assert_eq!(counts(&db), (2, 0));
}

#[test]
fn clipped_precise_without_text_or_audio_is_a_terminal_failure() {
    let mut db = database();
    let (_, strategy) = settings(&db, Input::Precise, false);
    block(&db);
    let id = event(&mut db, json!([OTHER_TAG]));
    complete(&mut db, id);
    let waiting = change(
        &mut db,
        run(id, strategy, ManualInput::PreparePrecise),
        NOW + 1,
    );
    let run_id = waiting["data"]["run_id"].as_str().unwrap();
    let asr = take_task(&mut db, CloudProvider::Asr, NOW + 2)
        .unwrap()
        .unwrap();
    publish_asr(
        &mut db,
        &asr,
        &[crate::cloud::AsrUtterance {
            start_ms: 0,
            end_ms: 1_000,
            text: "跨界句".into(),
            speaker: None,
            words: None,
        }],
        NOW + 3,
    )
    .unwrap();
    db.execute(
        "UPDATE event_asr_refs SET started_ms=?2 WHERE task_id=?1",
        params![asr.id.to_string(), START + 500],
    )
    .unwrap();
    db.execute("UPDATE audio_blocks SET media_path=NULL", [])
        .unwrap();
    let scope = scope(&db, &id.to_string()).unwrap().unwrap();
    assert_eq!(precise_state(&db, &scope).unwrap(), "partial_transcript");
    let state = run_metadata(&db, &scope, run_id).unwrap();
    assert_eq!(state["state"], "failed");
    assert_eq!(state["reason"], "no_transcript");
    assert!(
        take_task(&mut db, CloudProvider::Llm, NOW + 4)
            .unwrap()
            .is_none()
    );
}

#[test]
fn unsettled_new_events_do_not_starve_an_eligible_older_event() {
    let mut db = database();
    block(&db);
    let older = event(&mut db, Value::Null);
    let tx = db.transaction().unwrap();
    for index in 0..32 {
        let id = Uuid::new_v4().to_string();
        let start = 2_000_000 + index * 10_000;
        insert_generated_event(
            &tx,
            &id,
            start,
            start + 5_000,
            1,
            "待整理",
            None,
            None,
            start,
        )
        .unwrap();
        track(&tx, &id).unwrap();
        tx.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
                    VALUES(?1,'new','microphone',?2,?3,?4,240000,100,'media/pending.opus','retain','processing',1)",params![id,start,start+5_000,index*480_000]).unwrap();
    }
    judge_due(&tx, 5_000_000).unwrap();
    assert_eq!(
        status(&tx, &older.to_string(), 5_000_000)
            .unwrap()
            .unwrap()
            .phase,
        "decided"
    );
    tx.commit().unwrap();
}

#[test]
fn correction_previews_then_adds_and_cancels_only_unsubmitted_actions() {
    let mut db = database();
    let (tag, strategy) = settings(&db, Input::Precise, true);
    block(&db);
    let id = event(&mut db, json!([Uuid::new_v4()]));
    assert_eq!(
        status(&db, &id.to_string(), NOW)
            .unwrap()
            .unwrap()
            .tag_state,
        "failed"
    );
    complete(&mut db, id);
    let preview = read(
        &db,
        &Action::Tags {
            event_id: id,
            expected_revision: 2,
            tag_ids: vec![tag],
            plan: None,
        },
        NOW,
    )
    .unwrap();
    assert_eq!(preview["data"]["actions"].as_array().unwrap().len(), 2);
    assert_eq!(
        preview["data"]["actions"][1]["strategy_id"],
        json!(strategy)
    );
    assert_eq!(
        change(
            &mut db,
            Action::Tags {
                event_id: id,
                expected_revision: 2,
                tag_ids: vec![tag],
                plan: Some(preview["data"]["plan"].as_str().unwrap().to_owned())
            },
            NOW + 1
        )["ok"],
        true
    );
    assert_eq!(counts(&db), (1, 0));
    let preview = read(
        &db,
        &Action::Tags {
            event_id: id,
            expected_revision: 3,
            tag_ids: vec![OTHER_TAG],
            plan: None,
        },
        NOW + 1,
    )
    .unwrap();
    assert_eq!(preview["data"]["cancel_asr"].as_array().unwrap().len(), 1);
    assert_eq!(
        preview["data"]["cancel_strategies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        change(
            &mut db,
            Action::Tags {
                event_id: id,
                expected_revision: 3,
                tag_ids: vec![OTHER_TAG],
                plan: Some(preview["data"]["plan"].as_str().unwrap().to_owned())
            },
            NOW + 2
        )["ok"],
        true
    );
    assert_eq!(counts(&db), (0, 0));
    assert_eq!(
        event_state(&db, &id.to_string(), &asr_version(1, START, END)).unwrap(),
        "asr_skipped"
    );
}
