//! Explicit opt-in replay of a user-selected frozen fixture; never starts capture or ASR.
use super::*;
use serde_json::{Value, json};

#[test]
#[ignore = "explicit private replay, paid configured LLM calls; never live DB writes"]
fn real_understanding_replay() {
    use std::{
        cell::RefCell,
        fs,
        io::Write,
        os::unix::fs::OpenOptionsExt,
        path::PathBuf,
        time::{Duration, Instant},
    };
    assert_eq!(
        std::env::var("HEAR_REAL_UNDERSTANDING_ALLOW").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("HEAR_REAL_UNDERSTANDING_OUTPUT").unwrap());
    let save = |name: &str, value: &Value| {
        if out.join(name).exists() && (name == "window-plan.json" || name.ends_with("-input.json"))
        {
            let previous: Value =
                serde_json::from_slice(&fs::read(out.join(name)).unwrap()).unwrap();
            assert_eq!(&previous, value, "resume must use identical input");
            return;
        }
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(out.join(name))
            .unwrap();
        f.write_all(serde_json::to_string_pretty(value).unwrap().as_bytes())
            .unwrap();
    };
    let manifest: Value =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    let from = manifest["from_ms"].as_i64().unwrap();
    let to = manifest["to_ms"].as_i64().unwrap();
    let source = Connection::open_with_flags(
        out.join("source.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let model = manifest["model"].as_str().unwrap();
    let target = out.join("replay.sqlite");
    let resume = std::env::var("HEAR_REAL_UNDERSTANDING_RESUME").as_deref() == Ok("1");
    assert_eq!(
        target.exists(),
        resume,
        "existing replay requires explicit resume opt-in"
    );
    assert!(
        !out.join("final-saved.json").exists(),
        "completed replays are immutable"
    );
    let mut db = crate::Store::new(Connection::open(&target).unwrap())
        .unwrap()
        .db;
    if !resume {
        for table in ["coarse_segments", "audio_blocks"] {
            let cols: Vec<String> = source
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |r| r.get(1))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let mut select=source.prepare(&format!("SELECT {} FROM {table} WHERE started_ms<?2 AND ended_ms>?1 ORDER BY started_ms,ended_ms,id",cols.join(","))).unwrap();
            let rows = select
                .query_map(params![from, to], |r| {
                    (0..cols.len())
                        .map(|i| r.get::<_, rusqlite::types::Value>(i))
                        .collect::<Result<Vec<_>, _>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let sql = format!(
                "INSERT INTO {table} ({}) VALUES ({})",
                cols.join(","),
                (0..cols.len()).map(|_| "?").collect::<Vec<_>>().join(",")
            );
            for row in rows {
                db.execute(&sql, rusqlite::params_from_iter(row)).unwrap();
            }
        }
        set_configured(&db, CloudProvider::Llm, model).unwrap();
    }
    let now = to + 600_000;
    if resume {
        recover_tasks(&mut db, now).unwrap();
    } else {
        refresh_candidates(&mut db, now, true).unwrap();
    }
    let plan:Vec<Value>=db.prepare("SELECT id,started_ms,ended_ms FROM candidate_windows ORDER BY started_ms").unwrap().query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"start_ms":r.get::<_,i64>(1)?,"end_ms":r.get::<_,i64>(2)?}))).unwrap().collect::<Result<_,_>>().unwrap();
    let windows = plan.len();
    save("window-plan.json", &json!(plan));
    eprintln!("replay_planned windows={windows}");
    assert!((2..=12).contains(&windows));
    let reference = credential_reference(&source, CloudProvider::Llm)
        .unwrap()
        .unwrap();
    let key = crate::cloud::credential(CloudProvider::Llm, &reference)
        .unwrap()
        .expect("configured credential unavailable");
    struct ReplayMeter(RefCell<Vec<Value>>, PathBuf);
    impl crate::usage::Meter for ReplayMeter {
        fn begin(&self, _: &str, _: Option<Uuid>, _: Option<i64>) -> Result<String, String> {
            Ok(Uuid::new_v4().to_string())
        }
        fn llm_response(&self, id: &str, r: &Value) -> Result<(), String> {
            // Save even malformed model content before cloud::chat parses it.
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(self.1.join(format!("provider-response-{id}.json")))
                .map_err(|e| e.to_string())?;
            f.write_all(serde_json::to_string_pretty(r).unwrap().as_bytes())
                .map_err(|e| e.to_string())?;
            self.0
                .borrow_mut()
                .push(json!({"id":id,"provider_id":r["id"],"usage":r["usage"]}));
            Ok(())
        }
        fn asr_response(&self, _: Uuid, _: &str, _: &Value) -> Result<(), String> {
            panic!("No ASR calls allowed")
        }
        fn rejected(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn observation_failed(&self) {
            panic!("usage save failed")
        }
    }
    let meter = ReplayMeter(RefCell::new(Vec::new()), out.clone());
    let snapshots = |db: &Connection| -> Value {
        let values:Vec<Value>=db.prepare("SELECT id,started_ms,ended_ms,title,source_mask,summary,understanding_json,revision FROM named_events WHERE deleted_ms IS NULL ORDER BY started_ms,id").unwrap().query_map([],|r|{
            let data:Option<String>=r.get(6)?;
            Ok(json!({"id":r.get::<_,String>(0)?,"start_ms":r.get::<_,i64>(1)?,"end_ms":r.get::<_,i64>(2)?,"title":r.get::<_,String>(3)?,"source_mask":r.get::<_,i64>(4)?,"summary":r.get::<_,Option<String>>(5)?,"understanding":data.and_then(|v|serde_json::from_str::<Value>(&v).ok()),"revision":r.get::<_,i64>(7)?}))
        }).unwrap().collect::<Result<_,_>>().unwrap();
        json!({"events":values})
    };
    let completed = (0..windows)
        .take_while(|n| out.join(format!("window-{:02}-saved.json", n + 1)).exists())
        .count();
    if resume {
        eprintln!("replay_resumed completed={completed}; usage.json covers resumed calls only");
    }
    for n in completed..windows {
        let task = take_task_fair(&mut db, CloudProvider::Llm, now, n as u64)
            .unwrap()
            .expect("chronological task available");
        let input = llm_input(&db, &task).unwrap();
        save(
            &format!("window-{:02}-input.json", n + 1),
            &serde_json::from_str(&input).unwrap(),
        );
        eprintln!(
            "replay_request window={} start={} end={}",
            n + 1,
            task.start,
            task.end
        );
        let response = crate::cloud::chat(
            &key,
            model,
            FIT_SYSTEM,
            &input,
            Instant::now() + Duration::from_secs(90),
            &meter,
        )
        .expect("LLM call failed; do not silently rerun paid calls");
        save(&format!("window-{:02}-model.json", n + 1), &response);
        let applied = publish_fit_with_input(&mut db, &task, &input, response, now).unwrap();
        save(&format!("window-{:02}-saved.json", n + 1), &snapshots(&db));
        eprintln!("replay_saved window={} applied={applied}", n + 1);
    }
    save("final-saved.json", &snapshots(&db));
    save("usage.json", &json!(*meter.0.borrow()));
    eprintln!("replay_complete windows={windows}");
}
