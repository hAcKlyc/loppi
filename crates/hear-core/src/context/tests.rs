use super::*;

fn db() -> Connection {
    crate::Store::new(Connection::open_in_memory().unwrap())
        .unwrap()
        .db
}
fn run(db: &Connection, operation: &str, arguments: Value) -> Value {
    let q = Query::parse(operation, arguments).unwrap();
    let value = query(db, &q, Uuid::new_v4());
    assert_eq!(value["ok"], true, "{value}");
    assert!(serde_json::to_vec(&value).unwrap().len() <= BUDGET);
    value
}
fn time() -> Value {
    json!({"from":"1970-01-01T00:00:00Z","to":"1970-01-01T00:01:00Z"})
}
fn speech(db: &Connection, start: i64, end: i64, text: &str) -> Uuid {
    let id = Uuid::new_v4();
    db.execute(
        "INSERT INTO coarse_segments VALUES (?1,'block','session','microphone',?2,?3,?4,'test')",
        params![id.to_string(), start, end, text],
    )
    .unwrap();
    id
}
fn fixture(db: &Connection) -> (Uuid, Uuid) {
    let sid = speech(db, 1000, 10000, "先验证导出，再决定周五是否发布。");
    let eid = Uuid::new_v4();
    let u = json!({"schema_version":1,"topics":["发布计划"],"key_points":[{"text":"导出验证决定发布时间。","evidence_refs":[{"segment_id":sid,"source":"microphone","start_ms":1000,"end_ms":10000}]}],"mentioned_objects":[]});
    db.execute("INSERT INTO named_events(id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,created_ms,summary,understanding_json) VALUES(?1,0,20000,'发布讨论',1,1,'manual',1,1,0,0,'讨论导出验证与发布时间。',?2)",params![eid.to_string(),u.to_string()]).unwrap();
    (eid, sid)
}
fn precise(db: &Connection, eid: Uuid, kind: &str, start: i64, end: i64, text: &str) -> Uuid {
    let target = if kind == "range" {
        let id = Uuid::new_v4();
        db.execute(
            "INSERT INTO manual_ranges VALUES(?1,0,20000,1,1)",
            [id.to_string()],
        )
        .unwrap();
        id
    } else {
        eid
    };
    let task = Uuid::new_v4();
    db.execute("INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,source,state,created_ms,updated_ms) VALUES(?1,'asr',?2,?3,?4,'test','test',0,20000,'microphone','complete',0,0)",params![task.to_string(),kind,target.to_string(),if kind=="event" {"1:0:20000"} else {"1"}]).unwrap();
    let sid = Uuid::new_v4();
    db.execute(
        "INSERT INTO precise_segments VALUES(?1,?2,'microphone',?3,?4,?5,?2,'0')",
        params![sid.to_string(), task.to_string(), start, end, text],
    )
    .unwrap();
    db.execute(
        "INSERT INTO speaker_groups VALUES(?1,'0',?1)",
        [task.to_string()],
    )
    .unwrap();
    sid
}

#[test]
fn event_search_evidence_and_deletion() {
    let db = db();
    let (eid, sid) = fixture(&db);
    let cards = run(&db, "list", json!({"time":time()}));
    assert_eq!(cards["data"]["items"][0]["event_id"], eid.to_string());
    let hits = run(&db, "search", json!({"query":"导出验证"}));
    assert!(
        hits["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["matched_field"] == "key_point:0" && x["evidence_refs"][0].is_string())
    );
    let points = run(&db, "event", json!({"event_id":eid,"section":"key_points"}));
    let refs = &points["data"]["items"][0]["evidence_refs"];
    let raw = run(&db, "read", json!({"selector":{"refs":refs}}));
    assert_eq!(raw["data"]["items"][0]["item_id"], sid.to_string());
    assert_eq!(raw["data"]["items"][0]["kind"], "coarse");
    db.execute("DELETE FROM coarse_segments WHERE id=?1", [sid.to_string()])
        .unwrap();
    assert_eq!(
        run(&db, "read", json!({"selector":{"refs":refs}}))["data"]["items"][0]["reason"],
        "unavailable"
    );
    assert_eq!(
        run(&db, "event", json!({"event_id":eid}))["data"]["understanding_available"],
        false
    );
    db.execute(
        "UPDATE named_events SET deleted_ms=1 WHERE id=?1",
        [eid.to_string()],
    )
    .unwrap();
    assert!(
        run(&db, "search", json!({"query":"发布"}))["data"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn long_text_exact_continuation_and_stale_cursor() {
    let db = db();
    let text = "中文α🦊".repeat(1800);
    let sid = speech(&db, 1000, 10000, &text);
    speech(&db, 11000, 12000, "第二条");
    let mut args = json!({"selector":{"time":time()},"limit":1});
    let first = run(&db, "read", args.clone());
    let old_cursor = first["page"]["next_cursor"].clone();
    let mut output = String::new();
    let mut value = first;
    loop {
        for item in value["data"]["items"].as_array().unwrap() {
            output.push_str(item["text"].as_str().unwrap());
        }
        if value["page"]["has_more"] == false {
            break;
        }
        args["cursor"] = value["page"]["next_cursor"].clone();
        value = run(&db, "read", args.clone());
    }
    assert_eq!(output, format!("{text}第二条"));
    db.execute(
        "UPDATE search_records SET text=?2 WHERE item_id=?1",
        params![sid.to_string(), "改".repeat(8000)],
    )
    .unwrap();
    args["cursor"] = old_cursor;
    let q = Query::parse("read", args).unwrap();
    assert_eq!(
        query(&db, &q, Uuid::new_v4())["error"]["code"],
        "cursor_stale"
    );
}

#[test]
fn precise_fallback_duplicates_corrections_and_identity() {
    let mut db = db();
    let (eid, _) = fixture(&db);
    let sid = precise(&db, eid, "event", 1000, 5000, "精确原话");
    precise(&db, eid, "range", 1000, 5000, "精确原话");
    let result = run(&db, "read", json!({"selector":{"event_id":eid}}));
    let items = result["data"]["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        2,
        "partial coarse coverage must remain; equal precise duplicates must collapse"
    );
    assert!(items.iter().any(|v| v["overlaps_precise"] == true));
    let old = items.iter().find(|v| v["kind"] == "precise").unwrap()["ref"].clone();
    let tx = db.transaction().unwrap();
    crate::knowledge::correct_text(&tx, sid, "更正原话", 100).unwrap();
    let person = crate::knowledge::create_person(&tx, "林", 101).unwrap();
    crate::knowledge::bind_speaker(&tx, sid, person, false, 102).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        run(&db, "read", json!({"selector":{"refs":[old]}}))["data"]["items"][0]["reason"],
        "stale"
    );
    let result = run(&db, "search", json!({"query":"更正","person_id":person}));
    assert_eq!(result["data"]["items"][0]["corrected"], true);
    assert_eq!(result["data"]["items"][0]["person_name"], "林");
    assert_eq!(
        run(&db, "people", json!({"name":"林"}))["data"]["items"][0]["person_id"],
        person.to_string()
    );
    assert_eq!(
        run(&db, "event", json!({"event_id":eid}))["data"]["understanding_available"],
        false
    );
    db.execute("UPDATE cloud_tasks SET state='superseded'", [])
        .unwrap();
    let result = run(
        &db,
        "read",
        json!({"selector":{"event_id":eid},"transcript":"precise"}),
    );
    assert!(result["data"]["items"].as_array().unwrap().is_empty());
}

#[test]
fn malformed_metadata_does_not_break_writes_and_cursor_binding() {
    let db = db();
    let (eid, _) = fixture(&db);
    for raw in [
        "bad json",
        r#"{"key_points":["oops",4,null,{}],"mentioned_objects":["oops"],"topics":[]}"#,
    ] {
        db.execute(
            "UPDATE named_events SET understanding_json=?2 WHERE id=?1",
            params![eid.to_string(), raw],
        )
        .unwrap();
        assert_eq!(
            run(&db, "event", json!({"event_id":eid}))["data"]["understanding_available"],
            false
        );
    }
    let value = run(&db, "search", json!({"query":"发布","limit":1}));
    let q = Query::parse(
        "search",
        json!({"query":"导出","cursor":value["page"]["next_cursor"]}),
    )
    .unwrap();
    assert_eq!(
        query(&db, &q, Uuid::new_v4())["error"]["code"],
        "cursor_stale"
    );
    init(&db).unwrap();
    assert_eq!(
        run(&db, "search", json!({"query":"发布"}))["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn many_hits_are_paged_once_with_bounded_output() {
    let db = db();
    let tx = db.unchecked_transaction().unwrap();
    for i in 1..=4200 {
        tx.execute("INSERT INTO search_records(kind,item_id,source,started_ms,ended_ms,text) VALUES('note',?1,NULL,?2,?2+1,'commonword 内容')",params![Uuid::from_u128(i).to_string(),i as i64]).unwrap();
    }
    tx.commit().unwrap();
    let mut args = json!({"query":"commonword","limit":100});
    let mut seen = std::collections::HashSet::new();
    let mut times = Vec::new();
    loop {
        let t = std::time::Instant::now();
        let r = run(&db, "search", args.clone());
        times.push(t.elapsed().as_millis());
        for item in r["data"]["items"].as_array().unwrap() {
            assert!(seen.insert(item["item_id"].as_str().unwrap().to_owned()));
        }
        if r["page"]["has_more"] == false {
            break;
        }
        args["cursor"] = r["page"]["next_cursor"].clone();
    }
    assert_eq!(seen.len(), 4200);
    times.sort();
    eprintln!(
        "context 4200 records, {} pages, p95={}ms",
        times.len(),
        times[times.len() * 95 / 100]
    );
}

#[test]
fn moments_continue_across_short_items_and_long_fragments() {
    let db = db();
    let (eid, _) = fixture(&db);
    let mut expected = String::new();
    for i in 0..4 {
        let text = if i == 1 {
            "长评论".repeat(1800)
        } else {
            format!("评论{i}")
        };
        expected.push_str(&text);
        db.execute(
            "INSERT INTO moments(id,kind,anchor_ms,created_ms,text) VALUES(?1,'comment',?2,0,?3)",
            params![Uuid::new_v4().to_string(), i * 1000 + 100, &text],
        )
        .unwrap();
    }
    let mut args = json!({"event_id":eid,"section":"moments","limit":1});
    let mut actual = String::new();
    loop {
        let value = run(&db, "event", args.clone());
        for item in value["data"]["items"].as_array().unwrap() {
            actual.push_str(item["text"].as_str().unwrap());
        }
        if value["page"]["has_more"] == false {
            break;
        }
        args["cursor"] = value["page"]["next_cursor"].clone();
    }
    assert_eq!(actual, expected);
}

#[test]
fn screenshots_without_text_keep_locators_and_availability() {
    let db = db();
    let (eid, _) = fixture(&db);
    for (i, state, text) in [
        (0, Some("failed"), None),
        (1, None, None),
        (2, Some("complete"), Some("")),
        (3, Some("complete"), Some("截图文字")),
    ] {
        db.execute("INSERT INTO moments(id,kind,anchor_ms,created_ms,ocr_state,ocr_text) VALUES(?1,'screenshot',?2,0,?3,?4)",params![Uuid::new_v4().to_string(),i*1000+100,state,text]).unwrap();
    }
    let value = run(&db, "event", json!({"event_id":eid,"section":"moments"}));
    let items = value["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    for item in &items[..3] {
        assert_eq!(item["text_available"], false);
        assert!(item["ref"].is_string());
    }
    assert_eq!(items[0]["screenshot"]["ocr_state"], "failed");
    assert_eq!(items[3]["text_available"], true);
    let evidence = run(&db, "read", json!({"selector":{"refs":[items[0]["ref"]]}}));
    assert_eq!(evidence["data"]["items"][0]["text_available"], false);
    db.execute("UPDATE named_events SET deleted_ms=1", [])
        .unwrap();
    let listed = run(&db, "list", json!({"time":time()}));
    assert_eq!(
        listed["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["kind"] == "ocr")
            .count(),
        4
    );
}

#[test]
fn coverage_respects_sources_for_failures_and_pending_jobs() {
    let db = db();
    speech(&db, 1000, 2000, "已完成的麦克风内容");
    db.execute("INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,source,state,created_ms,updated_ms) VALUES('task','asr','range','range','1','test','test',0,20000,'system','failed',0,0)",[]).unwrap();
    db.execute("INSERT INTO coarse_jobs(id,block_id,session_id,source,start_sample,end_sample,started_ms,ended_ms,state) VALUES('job','block','session','system',0,1000,0,20000,'pending')",[]).unwrap();
    let microphone = run(
        &db,
        "read",
        json!({"selector":{"time":time(),"sources":"microphone"}}),
    );
    assert_eq!(microphone["quality"]["issues"], json!([]));
    let system = run(
        &db,
        "read",
        json!({"selector":{"time":time(),"sources":"system"}}),
    );
    assert!(
        system["quality"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["code"] == "processing_failed")
    );
    assert!(
        system["quality"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["code"] == "coarse_pending")
    );
    assert_eq!(
        run(
            &db,
            "search",
            json!({"query":"missing","sources":"microphone"})
        )["data"]["empty_reason"],
        "no_match"
    );
}

#[test]
fn empty_filter_dedupe_and_skipped_coverage_are_explicit() {
    let db = db();
    let (eid, _) = fixture(&db);
    speech(&db, 11000, 12000, "  ");
    precise(&db, eid, "event", 1000, 5000, "精转");
    let v = run(
        &db,
        "read",
        json!({"selector":{"event_id":eid},"dedupe":true,"include_coverage":false}),
    );
    assert_eq!(v["quality"]["coverage_state"], "not_requested");
    assert_eq!(v["data"]["items"].as_array().unwrap().len(), 1);
    let v = run(
        &db,
        "read",
        json!({"selector":{"event_id":eid},"min_chars":0}),
    );
    assert_eq!(v["data"]["items"].as_array().unwrap().len(), 3);
}

#[test]
fn digest_and_gap_totals_survive_paging() {
    let db = db();
    let (eid, _) = fixture(&db);
    for i in 0..20 {
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES(?1,'system',?2,?3,'source_failure')",
            params![i.to_string(), i * 1000, i * 1000 + 500],
        )
        .unwrap();
    }
    let v = run(&db, "digest", json!({"time":time(),"limit":1}));
    assert_eq!(v["data"]["events"][0]["event_id"], eid.to_string());
    assert_eq!(v["data"]["events"][0]["key_points_count"], 1);
    assert_eq!(v["quality"]["gap_summary"][0]["duration_ms"], 10000);
    assert_eq!(v["quality"]["gaps_truncated"], true);
}

#[test]
fn compact_digest_keeps_seven_large_events_within_two_pages() {
    let db = db();
    for _ in 0..7 {
        let (eid, _) = fixture(&db);
        let mut u: Value = serde_json::from_str(
            &load_event(&db, &eid.to_string())
                .unwrap()
                .understanding
                .unwrap(),
        )
        .unwrap();
        u["topics"] = json!(vec!["很长的话题".repeat(30); 8]);
        u["key_points"] = json!(vec![u["key_points"][0].clone(); 200]);
        // Unsupported legacy extras are ignored without rewriting stored JSON.
        u["decisions"] = json!([{"invalid_old_field":true}]);
        u["todos"] = json!("old unsupported value");
        db.execute(
            "UPDATE named_events SET title=?2,summary=?3,understanding_json=?4 WHERE id=?1",
            params![
                eid.to_string(),
                "长标题".repeat(100),
                "正文".repeat(10000),
                u.to_string()
            ],
        )
        .unwrap();
    }
    let mut args = json!({"time":time()});
    let mut ids = std::collections::HashSet::new();
    let mut pages = 0;
    loop {
        let v = run(&db, "digest", args.clone());
        pages += 1;
        assert_eq!(v["data"]["mode"], "compact");
        for row in v["data"]["events"].as_array().unwrap() {
            assert!(ids.insert(row["event_id"].as_str().unwrap().to_owned()));
            assert_eq!(row["key_points_count"], 200);
            assert_eq!(row["topics_count"], 8);
            assert_eq!(row["topics_truncated"], true);
            assert_eq!(row["title_truncated"], true);
            for absent in [
                "summary",
                "key_points",
                "mentioned_objects",
                "event_fragment",
                "decisions",
                "todos",
                "structured_actions_available",
            ] {
                assert!(row.get(absent).is_none());
            }
        }
        if v["page"]["has_more"] == false {
            break;
        }
        assert!(pages < 2);
        let mut wrong_mode = args.clone();
        wrong_mode["mode"] = json!("full");
        wrong_mode["cursor"] = v["page"]["next_cursor"].clone();
        assert_eq!(
            query(
                &db,
                &Query::parse("digest", wrong_mode).unwrap(),
                Uuid::new_v4()
            )["error"]["code"],
            "cursor_stale"
        );
        args["cursor"] = v["page"]["next_cursor"].clone();
    }
    assert_eq!(ids.len(), 7);
    let v = run(
        &db,
        "digest",
        json!({"time":time(),"mode":"full","limit":1}),
    );
    let row = &v["data"]["events"][0];
    assert!(row["summary"].is_array() && row["event_fragment"].is_object());
    assert!(row.get("decisions").is_none() && row.get("todos").is_none());
    let eid = ids.iter().next().unwrap();
    let v = run(&db, "event", json!({"event_id":eid,"section":"overview"}));
    assert_eq!(v["data"]["understanding_available"], true);
    assert_eq!(
        v["data"]["sections"],
        json!([
            "summary",
            "topics",
            "key_points",
            "mentioned_objects",
            "moments"
        ])
    );
    assert!(
        load_event(&db, eid)
            .unwrap()
            .understanding
            .unwrap()
            .contains("invalid_old_field")
    );
}

#[test]
#[ignore = "local read-only backup acceptance; set HEAR_REVIEW_SNAPSHOT to a closed SQLite backup"]
fn review_dataset_acceptance() {
    let source = std::env::var("HEAR_REVIEW_SNAPSHOT").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hear.sqlite");
    std::fs::copy(source, &path).unwrap();
    let store = crate::Store::new(Connection::open(&path).unwrap()).unwrap();
    let mut stmt=store.db.prepare("SELECT id FROM named_events WHERE deleted_ms IS NULL AND started_ms>=1793116800000 AND started_ms<1793203200000 ORDER BY started_ms").unwrap();
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(ids.len(), 7);
    let pool = crate::reads::Pool::open(&path).unwrap();
    let mut timings = Vec::new();
    let mut event_timings = Vec::new();
    for i in 0..100 {
        let (op, args) = if i % 2 == 0 {
            (
                "event",
                json!({"event_id":ids[(i/2)%7],"section":"key_points"}),
            )
        } else {
            (
                "read",
                json!({"selector":{"time":{"from":"2026-10-28T12:00:00+08:00","to":"2026-10-28T15:00:00+08:00"}}}),
            )
        };
        let start = std::time::Instant::now();
        let result = run(&pool.snapshot().unwrap(), op, args);
        timings.push(start.elapsed().as_millis());
        if op == "event" {
            event_timings.push(start.elapsed().as_millis());
        }
        if result["page"]["has_more"] == true {
            assert!(result["page"]["next_cursor"].is_string());
            assert_eq!(result["page"]["truncated"], true);
        }
    }
    let digest = run(
        &pool.snapshot().unwrap(),
        "digest",
        json!({"time":{"date":"2026-10-28","timezone":"Asia/Shanghai"}}),
    );
    let status = run(&pool.snapshot().unwrap(), "status", json!({}));
    assert_eq!(
        status["data"]["health_7d"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    timings.sort();
    println!(
        "acceptance events={} calls=100 p95={}ms max={}ms digest_events={}",
        ids.len(),
        timings[94],
        timings[99],
        digest["data"]["events"].as_array().unwrap().len()
    );
    assert!(timings[94] < 2000);
    event_timings.sort();
    assert!(event_timings[47] < 2000);
    println!(
        "event p95={}ms max={}ms",
        event_timings[47], event_timings[49]
    );
    if std::env::var("HEAR_REVIEW_WIRE").as_deref() == Ok("1") {
        review_wire(&path, &ids);
    }
}

fn review_wire(path: &std::path::Path, ids: &[String]) {
    use std::io::{BufRead, Read, Write};
    use std::process::{Command, Stdio};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let dir = path.parent().unwrap();
    let listener = std::os::unix::net::UnixListener::bind(dir.join("core.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let pool = crate::reads::Pool::open(path).unwrap();
    let server = std::thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            let (mut stream, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).unwrap();
            if raw.is_empty() {
                continue;
            }
            let envelope: hear_protocol::Envelope = serde_json::from_slice(&raw).unwrap();
            let hear_protocol::Request::Context { query: q } = envelope.request else {
                panic!("read-only acceptance harness")
            };
            assert_eq!(envelope.version, hear_protocol::PROTOCOL_VERSION);
            let value = query(&pool.snapshot().unwrap(), &q, envelope.id);
            let response = hear_protocol::Response::Context {
                id: envelope.id,
                result: value,
            };
            stream
                .write_all(&serde_json::to_vec(&response).unwrap())
                .unwrap();
        }
    });
    let cli = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/hear");
    let mut cli_times = Vec::new();
    for id in ids {
        let start = std::time::Instant::now();
        let output = Command::new(&cli)
            .env("HEAR_DATA_DIR", dir)
            .args(["context", "event", id, "--section", "key_points"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["ok"], true);
        cli_times.push(start.elapsed().as_millis());
    }
    let mut child = Command::new(&cli)
        .env("HEAR_DATA_DIR", dir)
        .args(["mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut rpc = |request: Value| {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    rpc(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acceptance","version":"1"}}}),
    );
    // A notification has no reply; deliver it before the first ordinary request.
    // Drop the closure's borrows briefly to write the notification.
    drop(rpc);
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    stdin.flush().unwrap();
    let mut rpc = |request: Value| {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    let discovery = rpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let names: Vec<_> = discovery["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].clone())
        .collect();
    assert_eq!(names.len(), 7);
    let status = rpc(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"hear_status","arguments":{"client_tools":names}}}),
    );
    assert_eq!(
        status["result"]["structuredContent"]["data"]["tool_inventory"]["state"],
        "matched"
    );
    let mut timings = Vec::new();
    for i in 0..100 {
        let (name, args) = if i % 2 == 0 {
            (
                "hear_get_event",
                json!({"event_id":ids[(i/2)%7],"section":"key_points"}),
            )
        } else {
            (
                "hear_read_context",
                json!({"selector":{"time":{"from":"2026-10-28T12:00:00+08:00","to":"2026-10-28T15:00:00+08:00"}}}),
            )
        };
        let start = std::time::Instant::now();
        let v = rpc(
            json!({"jsonrpc":"2.0","id":i+10,"method":"tools/call","params":{"name":name,"arguments":args}}),
        );
        assert_eq!(v["result"]["isError"], false, "{v}");
        assert_eq!(v["result"]["structuredContent"]["ok"], true);
        timings.push(start.elapsed().as_millis());
    }
    drop(rpc);
    drop(stdin);
    assert!(child.wait().unwrap().success());
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    timings.sort();
    cli_times.sort();
    assert!(timings[94] < 2000);
    assert!(cli_times[6] < 2000);
    println!(
        "wire CLI 7/7 max={}ms; MCP 100/100 p95={}ms max={}ms; tools={}/{}",
        cli_times[6],
        timings[94],
        timings[99],
        names.len(),
        names.len()
    );
}

#[test]
fn vocabulary_entities_resolve_across_days_and_disabled_injection() {
    let db = db();
    let vocabulary = crate::vocabulary::apply(
        &db,
        &hear_protocol::vocabulary::Action::Upsert {
            id: None,
            name: "看山".into(),
            kind: "product".into(),
            aliases: vec!["开山".into()],
            enabled: true,
        },
    )
    .unwrap();
    let expected = vocabulary["entries"][0]["entity_id"].clone();
    for (day, name) in [(0, "开山"), (1, "看山")] {
        let start = day * 86_400_000 + 1000;
        let end = start + 1000;
        let sid = speech(&db, start, end, "讨论看山额度");
        let eid = Uuid::new_v4();
        let u = json!({"schema_version":1,"topics":[],"key_points":[],"mentioned_objects":[{"name":name,"type":"产品","context":"讨论额度","entity_id":null,"evidence_refs":[{"segment_id":sid,"source":"microphone","start_ms":start,"end_ms":end}]}]});
        db.execute("INSERT INTO named_events(id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,created_ms,understanding_json) VALUES(?1,?2,?3,'词库测试',1,1,'manual',1,1,0,0,?4)",params![eid.to_string(),start,end,u.to_string()]).unwrap();
        let result = run(
            &db,
            "event",
            json!({"event_id":eid,"section":"mentioned_objects"}),
        );
        assert_eq!(result["data"]["items"][0]["entity_id"], expected);
        if day == 0 {
            crate::vocabulary::apply(
                &db,
                &hear_protocol::vocabulary::Action::Upsert {
                    id: Some(Uuid::parse_str(expected.as_str().unwrap()).unwrap()),
                    name: "看山".into(),
                    kind: "product".into(),
                    aliases: vec!["开山".into()],
                    enabled: false,
                },
            )
            .unwrap();
        }
    }
}
