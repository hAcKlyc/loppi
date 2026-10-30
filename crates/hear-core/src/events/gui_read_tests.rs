//! Opt-in measurements use a closed backup, never the live database.
use super::*;
use serde_json::json;
use std::{cell::RefCell, ffi::CStr, time::Instant};

thread_local! {
    static QUERIES: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" fn profile(
    _: u32,
    _: *mut std::ffi::c_void,
    statement: *mut std::ffi::c_void,
    elapsed: *mut std::ffi::c_void,
) -> i32 {
    // sqlite3_sql returns placeholders, not private bound values.
    unsafe {
        let stmt = statement.cast::<rusqlite::ffi::sqlite3_stmt>();
        let sql = CStr::from_ptr(rusqlite::ffi::sqlite3_sql(stmt)).to_string_lossy();
        let ns = *elapsed.cast::<u64>();
        let scans = rusqlite::ffi::sqlite3_stmt_status(
            stmt,
            rusqlite::ffi::SQLITE_STMTSTATUS_FULLSCAN_STEP,
            0,
        );
        let steps =
            rusqlite::ffi::sqlite3_stmt_status(stmt, rusqlite::ffi::SQLITE_STMTSTATUS_VM_STEP, 0);
        QUERIES.with(|q| {
            q.borrow_mut()
                .push(json!({"sql":sql,"ms":ns as f64/1e6,"scan_steps":scans,"vm_steps":steps}))
        });
    }
    0
}

#[test]
#[ignore = "set HEAR_REVIEW_SNAPSHOT to a closed SQLite backup and HEAR_GUI_PROFILE to output JSON"]
fn gui_read_profile() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hear.sqlite");
    std::fs::copy(std::env::var("HEAR_REVIEW_SNAPSHOT").unwrap(), &path).unwrap();
    let store = crate::Store::new(Connection::open(&path).unwrap()).unwrap();
    let db = &store.db;
    unsafe {
        rusqlite::ffi::sqlite3_trace_v2(
            db.handle(),
            rusqlite::ffi::SQLITE_TRACE_PROFILE as u32,
            Some(profile),
            std::ptr::null_mut(),
        );
    }
    let start = std::env::var("HEAR_PROFILE_START_MS")
        .map(|v| v.parse().unwrap())
        .unwrap_or(1793289600000_i64);
    let end = start + 86_400_000;
    let id: Option<String> = db.query_row(
        "SELECT id FROM named_events WHERE deleted_ms IS NULL AND started_ms<?2 AND ended_ms>?1 ORDER BY started_ms LIMIT 1",
        params![start, end], |r| r.get(0),
    ).optional().unwrap();
    let mut results = Vec::new();
    let mut detail = None;
    for op in [
        "calendar_days",
        "timeline",
        "event_detail",
        "session_speakers",
    ] {
        if op == "event_detail" && id.is_none() {
            continue;
        }
        if op == "session_speakers"
            && detail
                .as_ref()
                .is_none_or(|d: &hear_protocol::TimelineItem| d.precise_segments.is_empty())
        {
            continue;
        }
        QUERIES.with(|q| q.borrow_mut().clear());
        let begun = Instant::now();
        let value = match op {
            "calendar_days" => serde_json::to_value(
                calendar_days(
                    db,
                    &(0..=31)
                        .map(|day| end - (31 - day) * 86_400_000)
                        .collect::<Vec<_>>(),
                    end,
                )
                .unwrap(),
            )
            .unwrap(),
            "timeline" => serde_json::to_value(timeline(db, start, end).unwrap()).unwrap(),
            "event_detail" => {
                let item = event_detail(db, Uuid::parse_str(id.as_ref().unwrap()).unwrap(), 0, 100)
                    .unwrap();
                detail = Some(item.clone());
                serde_json::to_value(item).unwrap()
            }
            _ => serde_json::to_value(
                crate::knowledge::session_speakers(
                    db,
                    detail.as_ref().unwrap().precise_segments[0].id,
                )
                .unwrap(),
            )
            .unwrap(),
        };
        let query_ms = begun.elapsed().as_secs_f64() * 1000.;
        let begun = Instant::now();
        let bytes = serde_json::to_vec(&value).unwrap().len();
        let serialize_ms = begun.elapsed().as_secs_f64() * 1000.;
        let queries = QUERIES.with(|q| q.borrow().clone());
        let plans = queries
            .iter()
            .filter(|q| q["ms"].as_f64().unwrap() >= 10.)
            .map(|q| {
                let sql = q["sql"].as_str().unwrap();
                let mut stmt = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
                let params = vec![rusqlite::types::Value::Null; stmt.parameter_count()];
                let plan = stmt
                    .query_map(rusqlite::params_from_iter(params), |r| {
                        r.get::<_, String>(3)
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                json!({"sql":sql,"plan":plan})
            })
            .collect::<Vec<_>>();
        println!(
            "{op}: {query_ms:.2} ms, {} statements, {bytes} bytes, serialize {serialize_ms:.2} ms",
            queries.len()
        );
        results.push(json!({"operation":op,"query_ms":query_ms,"serialize_ms":serialize_ms,"bytes":bytes,"queries":queries,"plans":plans}));
    }
    unsafe {
        rusqlite::ffi::sqlite3_trace_v2(db.handle(), 0, None, std::ptr::null_mut());
    }
    std::fs::write(
        std::env::var("HEAR_GUI_PROFILE").unwrap(),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "offline numeric quality audit; HEAR_REVIEW_SNAPSHOT, HEAR_REVIEW_MEDIA and HEAR_GUI_PROFILE required"]
fn voice_quality_profile() {
    let db = Connection::open_with_flags(
        std::env::var("HEAR_REVIEW_SNAPSHOT").unwrap(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let directory = std::path::PathBuf::from(std::env::var("HEAR_REVIEW_MEDIA").unwrap());
    let mut statement = db.prepare("SELECT segment_id,source,started_ms,ended_ms,error_code FROM (SELECT segment_id,source,started_ms,ended_ms,error_code,row_number() OVER(PARTITION BY error_code ORDER BY started_ms) AS ordinal FROM voice_observations WHERE state='unavailable' AND error_code IN ('audio_quality','audio_unavailable') AND started_ms>=1793289600000 AND started_ms<1793376000000) WHERE ordinal<=64 ORDER BY error_code,started_ms").unwrap();
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .unwrap();
    let mut output = Vec::new();
    for row in rows {
        let (id, source, start, end, original) = row.unwrap();
        let mut stats = hear_protocol::LogContext::default();
        let result = voice_manifest(&db, &source, start, end)
            .and_then(|(task, parts)| voice_pcm_16k(&directory, &task, &parts, &mut stats));
        output.push(json!({"id":id,"source":source,"duration_ms":end-start,"original":original,"current_error":result.err().map(|e|crate::voice_error_code(&e)),"rms_millionths":stats.rms_millionths,"clipped_ppm":stats.clipped_ppm,"sample_frames":stats.sample_frames}));
    }
    std::fs::write(
        std::env::var("HEAR_GUI_PROFILE").unwrap(),
        serde_json::to_vec_pretty(&output).unwrap(),
    )
    .unwrap();
    println!(
        "numeric voice audit: {} bounded inputs, no inference/cloud/private text",
        output.len()
    );
}
