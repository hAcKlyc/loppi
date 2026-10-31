//! Read-only projections over the existing facts. No background jobs or new store.
use loppi_protocol::DiagnosticFilter;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

const ROW_LIMIT: usize = 100_000;
const TASK_LIMIT: usize = 1_000;

pub fn validate(filter: &DiagnosticFilter) -> Result<(), String> {
    if filter.from_ms < 0
        || filter.to_ms <= filter.from_ms
        || filter.to_ms - filter.from_ms > 7 * 86_400_000
    {
        return Err("diagnostics require an ordered range of at most seven days".into());
    }
    Ok(())
}
fn code(value: Option<String>) -> Option<String> {
    value.map(|s| {
        let mut e =
            loppi_protocol::LogEvent::simple("diagnostics", loppi_protocol::LogLevel::Info, "code");
        e.error_code = Some(s.clone());
        if crate::unilogs::valid_event(&e) {
            s
        } else {
            "unclassified".into()
        }
    })
}
fn id(value: String) -> Option<uuid::Uuid> {
    value.parse().ok()
}
fn union_ms(ranges: &mut [(i64, i64)]) -> i64 {
    ranges.sort_unstable();
    let mut end = 0;
    let mut total = 0;
    for &(a, b) in ranges.iter() {
        total += (b - a.max(end)).max(0);
        end = end.max(b);
    }
    total
}

pub fn facts(db: &Connection, filter: &DiagnosticFilter, now: i64) -> Result<Value, String> {
    validate(filter)?;
    // The time window selects physical task ranges; event membership is resolved
    // through the current references, including reused ASR and candidate fits.
    let (a, b) = (filter.from_ms, filter.to_ms);
    let event_id = filter.event_id.map(|v| v.to_string());
    let job_id = filter.job_id.map(|v| v.to_string());
    let predicate = "t.started_ms < ?2 AND t.ended_ms > ?1
      AND (?3 IS NULL OR t.id=?3)
      AND (?4 IS NULL OR (t.target_kind='event' AND t.target_id=?4)
        OR EXISTS(SELECT 1 FROM event_asr_refs r WHERE r.task_id=t.id AND r.event_id=?4)
        OR (t.target_kind='candidate' AND EXISTS(SELECT 1 FROM event_candidate_links l WHERE l.candidate_id=t.target_id AND l.event_id=?4)))";
    let sql = format!(
        "SELECT t.id,t.provider,t.target_kind,t.target_id,t.state,t.error_code,t.started_ms,t.ended_ms,t.attempts,t.submit_attempts,t.created_ms,t.updated_ms,t.processor_version,t.model_id FROM cloud_tasks t WHERE {predicate} ORDER BY CASE t.state WHEN 'failed' THEN 0 WHEN 'pending' THEN 1 WHEN 'processing' THEN 2 WHEN 'submitted' THEN 3 ELSE 4 END,t.started_ms,t.id LIMIT {}",
        TASK_LIMIT + 1
    );
    let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
    let mut tasks = stmt.query_map(params![a,b,job_id,event_id], |r| Ok(json!({
        "job_id":id(r.get(0)?),"provider":code(r.get(1)?),"target_kind":code(r.get(2)?),"target_id":id(r.get(3)?),
        "state":code(r.get(4)?),"error_code":code(r.get(5)?),"start_ms":r.get::<_,i64>(6)?,"end_ms":r.get::<_,i64>(7)?,
        "failed_attempts":r.get::<_,i64>(8)?,"submit_attempts":r.get::<_,i64>(9)?,"created_ms":r.get::<_,i64>(10)?,"updated_ms":r.get::<_,i64>(11)?,
        "processor_version":r.get::<_,String>(12)?.chars().filter(|c|c.is_ascii_alphanumeric()||"._-".contains(*c)).take(80).collect::<String>(),
        "model_id":r.get::<_,String>(13)?.chars().filter(|c|c.is_ascii_alphanumeric()||"._-".contains(*c)).take(80).collect::<String>()
    }))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let tasks_truncated = tasks.len() > TASK_LIMIT;
    tasks.truncate(TASK_LIMIT);
    let mut summary = db.prepare(&format!("SELECT provider,state,count(*),min(CASE WHEN state IN ('pending','processing','submitted') THEN created_ms END),sum(CASE WHEN state='complete' AND attempts>0 THEN 1 ELSE 0 END),avg(CASE WHEN state='complete' THEN max(0,updated_ms-created_ms) END) FROM cloud_tasks t WHERE {predicate} GROUP BY provider,state")).map_err(|e|e.to_string())?;
    let queues = summary
        .query_map(params![a, b, job_id, event_id], |r| {
            Ok(json!({
                "provider":code(r.get(0)?),"state":code(r.get(1)?),"count":r.get::<_,i64>(2)?,
                "oldest_pending_age_ms":r.get::<_,Option<i64>>(3)?.map(|v|(now-v).max(0)),
                "completed_after_failure":r.get::<_,i64>(4)?,
                "mean_completion_elapsed_ms":r.get::<_,Option<f64>>(5)?
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    // Coverage is always for the requested WINDOW, independent of task filters.
    // Sealed/silent/cleaned blocks prove capture; media presence only proves retention.
    let mut coverage = Vec::new();
    for source in ["microphone", "system"] {
        let mut stmt=db.prepare("SELECT max(started_ms,?1),min(ended_ms,?2),media_path IS NOT NULL FROM audio_blocks WHERE source=?3 AND started_ms<?2 AND ended_ms>?1 ORDER BY started_ms,id LIMIT ?4").map_err(|e|e.to_string())?;
        let mut rows = stmt
            .query_map(params![a, b, source, ROW_LIMIT as i64 + 1], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let truncated = rows.len() > ROW_LIMIT;
        rows.truncate(ROW_LIMIT);
        let mut all = rows.iter().map(|(a, b, _)| (*a, *b)).collect::<Vec<_>>();
        let mut retained = rows
            .iter()
            .filter(|(_, _, present)| *present)
            .map(|(a, b, _)| (*a, *b))
            .collect::<Vec<_>>();
        let mut stmt=db.prepare("SELECT max(started_ms,?1),min(coalesce(ended_ms,?4),?2) FROM media_gaps WHERE source=?3 AND started_ms<min(?2,?4) AND coalesce(ended_ms,?4)>?1 ORDER BY started_ms,id LIMIT ?5").map_err(|e|e.to_string())?;
        let mut gaps = stmt
            .query_map(params![a, b, source, now, ROW_LIMIT as i64 + 1], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let gaps_truncated = gaps.len() > ROW_LIMIT;
        gaps.truncate(ROW_LIMIT);
        let mut sessions_stmt=db.prepare("SELECT max(started_ms,?1),min(coalesce(ended_ms,?4),?2) FROM recording_sessions WHERE source=?3 AND started_ms<min(?2,?4) AND coalesce(ended_ms,?4)>?1 ORDER BY started_ms,id LIMIT ?5").map_err(|e|e.to_string())?;
        let mut sessions = sessions_stmt
            .query_map(params![a, b, source, now, ROW_LIMIT as i64 + 1], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let sessions_truncated = sessions.len() > ROW_LIMIT;
        sessions.truncate(ROW_LIMIT);
        let sealed = union_ms(&mut all);
        coverage.push(json!({"source":source,"recording_session_span_ms":union_ms(&mut sessions),"sessions_truncated":sessions_truncated,"sealed_blocks":rows.len(),"sealed_coverage_ms":sealed,"retained_media_coverage_ms":union_ms(&mut retained),"recorded_gap_ms":union_ms(&mut gaps),"uncovered_window_ms":b-a-sealed,"blocks_truncated":truncated,"gaps_truncated":gaps_truncated}));
    }
    let mut local = Vec::new();
    let mut local_jobs = Vec::new();
    let mut local_jobs_truncated = false;
    for (stage, table, state, attempts, error) in [
        (
            "vad",
            "audio_blocks",
            "analysis_state",
            "analysis_attempts",
            "NULL",
        ),
        ("coarse", "coarse_jobs", "state", "attempts", "error_code"),
    ] {
        let sql = format!(
            "SELECT {state},count(*),min(started_ms),sum(CASE WHEN {attempts}>0 THEN 1 ELSE 0 END) FROM {table} WHERE started_ms<?2 AND ended_ms>?1 GROUP BY {state} LIMIT 128"
        );
        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        for row in stmt.query_map(params![a,b],|r| {
            let state:String=r.get(0)?;
            Ok(json!({"stage":stage,"state":code(Some(state.clone())),"count":r.get::<_,i64>(1)?,"oldest_audio_age_ms":(now-r.get::<_,i64>(2)?).max(0),"nonzero_attempt_counter_count":r.get::<_,i64>(3)?}))
        }).map_err(|e|e.to_string())? {local.push(row.map_err(|e|e.to_string())?);}
        let sql = format!(
            "SELECT id,session_id,source,started_ms,ended_ms,{state},{attempts},{error} FROM {table} WHERE started_ms<?2 AND ended_ms>?1 AND (?3 IS NULL OR id=?3) AND ({state} IN ('pending','processing','failed') OR ?3 IS NOT NULL) ORDER BY CASE {state} WHEN 'failed' THEN 0 ELSE 1 END,started_ms,id LIMIT {}",
            TASK_LIMIT + 1
        );
        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        let rows=stmt.query_map(params![a,b,job_id],|r|Ok(json!({"stage":stage,"job_id":id(r.get(0)?),"capture_span_id":id(r.get(1)?),"source":code(r.get(2)?),"start_ms":r.get::<_,i64>(3)?,"end_ms":r.get::<_,i64>(4)?,"state":code(r.get(5)?),"attempt_counter":r.get::<_,i64>(6)?,"error_code":code(r.get(7)?)}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        local_jobs_truncated |= rows.len() > TASK_LIMIT;
        local_jobs.extend(rows.into_iter().take(TASK_LIMIT));
    }
    Ok(
        json!({"cloud_tasks":tasks,"cloud_tasks_truncated":tasks_truncated,"cloud_summary":queues,
        "window_capture_coverage":coverage,"window_local_processing":local,
        "window_local_jobs":local_jobs,"local_jobs_truncated":local_jobs_truncated,
        "window_usage":crate::usage::diagnostic_totals(db,a,b)?,
        "scope_semantics":"usage by call time; cloud tasks intersect time/job/event; local jobs intersect time/job; capture and local summary cover the window independently of IDs",
        "coverage_semantics":"sealed metadata, not decoded audio or speech recall; uncovered includes idle, cleaned and unknown time",
        "completion_semantics":"physical tasks overlapping window; current state; elapsed includes queueing; cloud failed_attempts count failures, not polls; completed_after_failure is within the current counter lifetime; VAD attempt_counter counts claims, coarse counts failures"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use uuid::Uuid;

    #[test]
    fn snapshot_is_content_free_clipped_and_does_not_change_facts() {
        let store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let db = &store.db;
        db.execute_batch("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state) VALUES
          ('a','private-session','microphone',500,2000,0,72000,10,'/private/audio-secret','retain','complete'),
          ('b','private-session','microphone',1500,2500,72000,48000,0,NULL,'discard','complete');
          INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES('gap','microphone',2500,NULL,'private reason');
          INSERT INTO coarse_segments VALUES('transcript','a','private-session','microphone',1000,1500,'private words','model');").unwrap();
        let job = Uuid::new_v4();
        let target = Uuid::new_v4();
        db.execute("INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,state,attempts,error_code,created_ms,updated_ms) VALUES(?1,'llm','candidate',?2,'v','deepseek-flash','fit-v3',1000,2000,'failed',2,'private /path secret',1000,3000)",params![job.to_string(),target.to_string()]).unwrap();
        let before = db.total_changes();
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        let filter = DiagnosticFilter {
            from_ms: 1000,
            to_ms: 3000,
            event_id: None,
            job_id: None,
        };
        let report = facts(db, &filter, 2800).unwrap();
        assert_eq!(db.total_changes(), before);
        assert_eq!(
            report["window_capture_coverage"][0]["sealed_coverage_ms"],
            1500
        );
        assert_eq!(
            report["window_capture_coverage"][0]["retained_media_coverage_ms"],
            1000
        );
        assert_eq!(report["window_capture_coverage"][0]["recorded_gap_ms"], 300);
        assert_eq!(report["cloud_tasks"][0]["error_code"], "unclassified");
        let encoded = report.to_string();
        for secret in ["private", "audio-secret", "words", "transcript"] {
            assert!(!encoded.contains(secret), "{secret}");
        }
        let missing = DiagnosticFilter {
            job_id: Some(Uuid::new_v4()),
            ..filter.clone()
        };
        assert_eq!(
            facts(db, &missing, 2800).unwrap()["cloud_tasks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(
            validate(&DiagnosticFilter {
                to_ms: 1000,
                ..filter.clone()
            })
            .is_err()
        );
        assert!(
            validate(&DiagnosticFilter {
                to_ms: 8 * 86_400_000,
                ..filter
            })
            .is_err()
        );
    }

    #[test]
    fn event_filter_includes_candidate_and_shared_asr_dependencies() {
        let store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let event = Uuid::new_v4();
        let candidate = Uuid::new_v4();
        let llm = Uuid::new_v4();
        let asr = Uuid::new_v4();
        store.db.execute("INSERT INTO named_events(id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,created_ms) VALUES(?1,1000,2000,'private title',1,0,'automatic',0,0,1,0)",[event.to_string()]).unwrap();
        store
            .db
            .execute(
                "INSERT INTO event_candidate_links VALUES(?1,0,?2)",
                params![candidate.to_string(), event.to_string()],
            )
            .unwrap();
        for (job, provider, kind, target) in [
            (llm, "llm", "candidate", candidate),
            (asr, "asr", "event", Uuid::new_v4()),
        ] {
            store.db.execute("INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,state,created_ms,updated_ms) VALUES(?1,?2,?3,?4,'v','model','v',1000,2000,'complete',500,2500)",params![job.to_string(),provider,kind,target.to_string()]).unwrap();
        }
        store
            .db
            .execute(
                "INSERT INTO event_asr_refs VALUES(?1,?2,1000,2000)",
                params![event.to_string(), asr.to_string()],
            )
            .unwrap();
        let filter = DiagnosticFilter {
            from_ms: 1000,
            to_ms: 2000,
            event_id: Some(event),
            job_id: None,
        };
        assert_eq!(
            facts(&store.db, &filter, 3000).unwrap()["cloud_tasks"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let filter = DiagnosticFilter {
            job_id: Some(llm),
            ..filter
        };
        assert_eq!(
            facts(&store.db, &filter, 3000).unwrap()["cloud_tasks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[cfg(test)]
#[test]
#[ignore = "read-only frozen database audit; requires LOPPI_TEST_DB and LOPPI_TEST_REPORT"]
fn audit_frozen_snapshot_without_starting_services() {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let path = std::env::var("LOPPI_TEST_DB").expect("frozen snapshot required");
    let output = std::env::var("LOPPI_TEST_REPORT").expect("private output required");
    let from_ms = std::env::var("LOPPI_TEST_FROM_MS")
        .unwrap()
        .parse()
        .unwrap();
    let to_ms = std::env::var("LOPPI_TEST_TO_MS").unwrap().parse().unwrap();
    let pool = crate::reads::Pool::open(std::path::Path::new(&path)).unwrap();
    let db = pool.snapshot().unwrap();
    let filter = DiagnosticFilter {
        from_ms,
        to_ms,
        event_id: None,
        job_id: None,
    };
    let report = facts(&db, &filter, crate::now_ms().unwrap()).unwrap();
    let mut file = loppi_protocol::platform::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output)
        .unwrap();
    file.write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes())
        .unwrap();
}
