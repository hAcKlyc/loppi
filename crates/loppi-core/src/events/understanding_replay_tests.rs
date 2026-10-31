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
        std::env::var("LOPPI_REAL_UNDERSTANDING_ALLOW").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("LOPPI_REAL_UNDERSTANDING_OUTPUT").unwrap());
    let prompt = std::env::var_os("LOPPI_REAL_UNDERSTANDING_PROMPT")
        .map(|path| fs::read_to_string(path).unwrap())
        .unwrap_or_else(|| FIT_SYSTEM.to_owned());
    assert!(!prompt.trim().is_empty());
    // A longer opt-in evaluation budget can inspect model quality separately
    // from production's 60-second deadline; it never changes production calls.
    let deadline_seconds: u64 = std::env::var("LOPPI_REAL_UNDERSTANDING_DEADLINE_SECS")
        .map(|value| value.parse().unwrap())
        .unwrap_or(60);
    assert!(matches!(deadline_seconds, 60 | 120));
    let effort =
        std::env::var("LOPPI_REAL_UNDERSTANDING_EFFORT").unwrap_or_else(|_| "low".to_owned());
    assert!(matches!(effort.as_str(), "low" | "high"));
    let save = |name: &str, value: &Value| {
        if out.join(name).exists()
            && (name == "window-plan.json"
                || name == "request-settings.json"
                || name.ends_with("-input.json"))
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
    save(
        "request-settings.json",
        &json!({"model":model,"system_prompt":prompt,"deadline_seconds":deadline_seconds,
            "thinking":"enabled","reasoning_effort":effort}),
    );
    let target = out.join("replay.sqlite");
    let resume = std::env::var("LOPPI_REAL_UNDERSTANDING_RESUME").as_deref() == Ok("1");
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
    } else if let Some(windows) = manifest["frozen_windows"].as_array() {
        // Paired transcript experiments keep the coarse planner's boundaries.
        // Only this ignored fixture entry bypasses replanning different ASR rows.
        assert!((1..=12).contains(&windows.len()));
        let tx = db.transaction().unwrap();
        let mut previous_start = -1;
        for window in windows {
            let start = window["start_ms"].as_i64().unwrap();
            let end = window["end_ms"].as_i64().unwrap();
            assert!(start >= previous_start && start < end);
            previous_start = start;
            let id = window["id"].as_str().unwrap();
            let hash = manifest["source_sha256"].as_str().unwrap();
            tx.execute(
                "INSERT INTO candidate_windows VALUES(?1,?2,?3,?4,'pending',NULL)",
                params![id, start, end, hash],
            )
            .unwrap();
            insert_task(
                &tx,
                "llm",
                "candidate",
                id,
                hash,
                start,
                end,
                None,
                true,
                now,
                FIT_POLICY_VERSION,
            )
            .unwrap();
        }
        tx.commit().unwrap();
    } else {
        refresh_candidates(&mut db, now, true).unwrap();
    }
    let plan:Vec<Value>=db.prepare("SELECT id,started_ms,ended_ms FROM candidate_windows ORDER BY started_ms").unwrap().query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"start_ms":r.get::<_,i64>(1)?,"end_ms":r.get::<_,i64>(2)?}))).unwrap().collect::<Result<_,_>>().unwrap();
    let windows = plan.len();
    save("window-plan.json", &json!(plan));
    eprintln!("replay_planned windows={windows}");
    assert!((1..=12).contains(&windows));
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
        let requested = Instant::now();
        let response = crate::cloud::chat(
            &key,
            model,
            &prompt,
            &input,
            Instant::now() + Duration::from_secs(deadline_seconds),
            &meter,
        );
        save(
            &format!("attempt-{}.json", Uuid::new_v4()),
            &json!({"window":n+1,"elapsed_seconds":requested.elapsed().as_secs_f64(),
                "success":response.is_ok(),"error":response.as_ref().err().map(|e|format!("{e:?}"))}),
        );
        let response = response.unwrap_or_else(|error| {
            save("retained-saved.json", &snapshots(&db));
            save("usage.json", &json!(*meter.0.borrow()));
            panic!("LLM call failed; no paid rerun: {error:?}");
        });
        save(&format!("window-{:02}-model.json", n + 1), &response);
        let publication = publish_fit_with_input(&mut db, &task, &input, response, now);
        save(
            &format!("window-{:02}-publication.json", n + 1),
            &json!({"success":publication.is_ok(),"error":publication.as_ref().err()}),
        );
        let applied = publication.unwrap_or_else(|error| {
            save("retained-saved.json", &snapshots(&db));
            save("usage.json", &json!(*meter.0.borrow()));
            panic!("Publication rejected; no paid rerun: {error}");
        });
        save(&format!("window-{:02}-saved.json", n + 1), &snapshots(&db));
        eprintln!("replay_saved window={} applied={applied}", n + 1);
    }
    save("final-saved.json", &snapshots(&db));
    save("usage.json", &json!(*meter.0.borrow()));
    eprintln!("replay_complete windows={windows}");
}

#[test]
#[ignore = "explicit local response manifest; offline, no credentials or live database"]
fn recorded_fit_response_validation() {
    let path = std::env::var("LOPPI_RECORDED_FIT_MANIFEST").expect("private response manifest");
    let cases: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut rejected = 0;
    for case in cases.as_array().unwrap() {
        let raw: Value = serde_json::from_slice(
            &std::fs::read(case["response_path"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let result = crate::cloud::parse_chat_content(&raw)
            .map_err(|e| e.code)
            .and_then(|v| {
                let events = v["events"]
                    .as_array()
                    .ok_or("invalid_fit_schema".to_owned())?;
                validate_fit_ranges(
                    events,
                    case["start_ms"].as_i64().unwrap(),
                    case["end_ms"].as_i64().unwrap(),
                )
            });
        assert_eq!(
            result.as_ref().err().map(String::as_str),
            case["expected_error"].as_str(),
            "{}",
            case["id"]
        );
        rejected += usize::from(result.is_err());
    }
    eprintln!(
        "recorded_validation total={} rejected={rejected}",
        cases.as_array().unwrap().len()
    );
}

#[test]
#[ignore = "explicit private fixture repair; one paid ASR submission, never live DB writes"]
fn real_precise_fixture_repair() {
    use std::{
        cell::Cell,
        fs,
        path::PathBuf,
        time::{Duration, Instant},
    };
    assert_eq!(
        std::env::var("LOPPI_REAL_UNDERSTANDING_ALLOW").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("LOPPI_REAL_ASR_FIXTURE_OUTPUT").unwrap());
    assert!(
        !out.join("request-id.txt").exists(),
        "No implicit paid rerun"
    );
    let source = Connection::open_with_flags(
        out.join("source.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let input: asr::Input =
        serde_json::from_slice(&fs::read(out.join("asr-input.json")).unwrap()).unwrap();
    let audio = asr::assemble(&out, &input).unwrap();
    fs::write(out.join("submitted.ogg"), &audio).unwrap();
    let reference = credential_reference(&source, CloudProvider::Asr)
        .unwrap()
        .unwrap();
    let key = crate::cloud::credential(CloudProvider::Asr, &reference)
        .unwrap()
        .unwrap();
    struct Meter(PathBuf, Cell<usize>);
    impl crate::usage::Meter for Meter {
        fn begin(&self, _: &str, _: Option<Uuid>, _: Option<i64>) -> Result<String, String> {
            Ok(Uuid::new_v4().to_string())
        }
        fn llm_response(&self, _: &str, _: &Value) -> Result<(), String> {
            panic!("ASR fixture only")
        }
        fn asr_response(&self, _: Uuid, status: &str, body: &Value) -> Result<(), String> {
            let n = self.1.get();
            self.1.set(n + 1);
            fs::write(
                self.0.join(format!("response-{n:03}.json")),
                serde_json::to_vec_pretty(&json!({"status":status,"body":body})).unwrap(),
            )
            .map_err(|e| e.to_string())
        }
        fn rejected(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn observation_failed(&self) {
            panic!("Could not record ASR response")
        }
    }
    let meter = Meter(out.clone(), Cell::new(0));
    let id = Uuid::new_v4();
    fs::write(out.join("request-id.txt"), id.to_string()).unwrap();
    crate::cloud::submit_audio(
        &key,
        id,
        &audio,
        crate::cloud::AsrAudioFormat::Ogg,
        &serde_json::json!({}),
        input.duration_ms,
        Instant::now() + Duration::from_secs(60),
        &meter,
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(240);
    loop {
        assert!(
            Instant::now() < until,
            "ASR not complete; preserve request ID"
        );
        match crate::cloud::query_audio(&key, id, Instant::now() + Duration::from_secs(30), &meter)
            .unwrap()
        {
            crate::cloud::AsrQuery::Processing => std::thread::sleep(Duration::from_secs(2)),
            crate::cloud::AsrQuery::Missing => panic!("Submitted ASR request missing"),
            crate::cloud::AsrQuery::Complete(items) => {
                fs::write(
                    out.join("provider-utterances.json"),
                    serde_json::to_vec_pretty(&items).unwrap(),
                )
                .unwrap();
                let mapped = asr::map(&input, &items).unwrap();
                fs::write(
                    out.join("mapped-utterances.json"),
                    serde_json::to_vec_pretty(&mapped).unwrap(),
                )
                .unwrap();
                break;
            }
        }
    }
}

#[test]
#[ignore = "explicit paid vocabulary acceptance with synthetic text/audio and read-only credential references"]
fn live_vocabulary_acceptance() {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };
    assert_eq!(std::env::var("LOPPI_VOCABULARY_LIVE").as_deref(), Ok("1"));
    let out = PathBuf::from(std::env::var("LOPPI_VOCABULARY_OUTPUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let source = Connection::open_with_flags(
        std::env::var("LOPPI_REVIEW_SNAPSHOT").unwrap(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    struct Meter;
    impl crate::usage::Meter for Meter {
        fn begin(&self, _: &str, _: Option<Uuid>, _: Option<i64>) -> Result<String, String> {
            Ok(Uuid::new_v4().to_string())
        }
        fn llm_response(&self, _: &str, _: &Value) -> Result<(), String> {
            Ok(())
        }
        fn asr_response(&self, _: Uuid, _: &str, _: &Value) -> Result<(), String> {
            Ok(())
        }
        fn rejected(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn observation_failed(&self) {
            panic!("usage observation failed")
        }
    }
    let meter = Meter;
    let key = |provider| {
        let reference = credential_reference(&source, provider)
            .unwrap()
            .expect("configured provider required");
        crate::cloud::credential(provider, &reference)
            .unwrap()
            .expect("credential required")
    };
    let input=json!({"candidate":{"start_ms":0,"end_ms":10000},"vocabulary":[{"entity_id":"test-kanshan","name":"看山","kind":"product","aliases":["开山"],"enabled":true},{"entity_id":"test-inception","name":"盗梦空间","kind":"project","aliases":["梦空间"],"enabled":true}],"observations":[{"evidence_index":0,"source":"microphone","start_ms":0,"end_ms":10000,"text":"开山工作台决定先提高额度。小王明天负责验证梦空间项目的导出功能。"}],"evidence":[{"segment_id":"synthetic","source":"microphone","start_ms":0,"end_ms":10000}]}).to_string();
    let model: String = source
        .query_row(
            "SELECT model FROM cloud_queues WHERE provider='llm'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let output = crate::cloud::chat(
        &key(CloudProvider::Llm),
        &model,
        rebuild::SYSTEM,
        &input,
        Instant::now() + Duration::from_secs(60),
        &meter,
    )
    .unwrap();
    std::fs::write(
        out.join("llm-output.json"),
        serde_json::to_vec_pretty(&output).unwrap(),
    )
    .unwrap();
    assert!(output.to_string().contains("看山") && output.to_string().contains("盗梦空间"));
    assert!(!output.to_string().contains("开山"));
    println!("live LLM vocabulary passed");
    let audio = std::fs::read(std::env::var("LOPPI_VOCABULARY_WAV").unwrap()).unwrap();
    let id_path = out.join("asr-request-id.txt");
    let asr_key = key(CloudProvider::Asr);
    let id = if id_path.exists() {
        Uuid::parse_str(std::fs::read_to_string(&id_path).unwrap().trim()).unwrap()
    } else {
        let id = Uuid::new_v4();
        std::fs::write(&id_path, id.to_string()).unwrap();
        crate::cloud::submit_audio(
            &asr_key,
            id,
            &audio,
            crate::cloud::AsrAudioFormat::Wav,
            &json!({"hotwords":[{"word":"看山"},{"word":"盗梦空间"}]}),
            ((audio.len() - 44) / 32) as i64,
            Instant::now() + Duration::from_secs(60),
            &meter,
        )
        .unwrap();
        id
    };
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        match crate::cloud::query_audio(
            &asr_key,
            id,
            Instant::now() + Duration::from_secs(20),
            &meter,
        )
        .unwrap()
        {
            crate::cloud::AsrQuery::Complete(items) => {
                let text = items.iter().map(|v| v.text.as_str()).collect::<String>();
                std::fs::write(
                    out.join("asr-output.json"),
                    serde_json::to_vec_pretty(&json!({"request_id":id,"text":text})).unwrap(),
                )
                .unwrap();
                assert!(text.contains("看山") && text.contains("盗梦空间"), "{text}");
                println!("live cloud ASR hotwords accepted and both canonical terms recognized");
                break;
            }
            crate::cloud::AsrQuery::Processing if Instant::now() < until => {
                std::thread::sleep(Duration::from_secs(3))
            }
            other => panic!("ASR incomplete; preserve request ID for query only: {other:?}"),
        }
    }
}
