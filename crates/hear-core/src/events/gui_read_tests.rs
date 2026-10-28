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
    let start = 1793116800000;
    let end = 1793203200000;
    let id = Uuid::parse_str("4b0b5314-0fad-4e0b-bdab-d7afbba4e3e7").unwrap();
    let mut results = Vec::new();
    let mut detail = None;
    for op in ["timeline", "event_detail", "session_speakers"] {
        QUERIES.with(|q| q.borrow_mut().clear());
        let begun = Instant::now();
        let value = match op {
            "timeline" => serde_json::to_value(timeline(db, start, end).unwrap()).unwrap(),
            "event_detail" => {
                let item = event_detail(db, id, 0, 100).unwrap();
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
