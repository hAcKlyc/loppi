//! One-time user instructions use the existing LLM queue and event publisher.
use super::*;
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS event_reorganization_requests (
        id TEXT PRIMARY KEY,title TEXT NOT NULL,started_ms INTEGER NOT NULL,ended_ms INTEGER NOT NULL,
        source_mask INTEGER NOT NULL,created_ms INTEGER NOT NULL);")
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn enqueue(
    tx: &Transaction<'_>,
    id: Uuid,
    start: i64,
    end: i64,
    title: &str,
    sources: EventSources,
    now: i64,
) -> Result<Uuid, String> {
    validate_range(start, end)?;
    validate_title(title)?;
    if end > now {
        return Err("reorganization_future_range".into());
    }
    let mask = source_mask(sources);
    let has_text: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM coarse_segments WHERE started_ms<?2 AND ended_ms>?1 AND length(trim(text))>0 AND ((source='microphone' AND ?3&1!=0) OR(source='system' AND ?3&2!=0)))",params![start,end,mask],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !has_text {
        return Err("no_coarse_text".into());
    }
    tx.execute(
        "INSERT INTO event_reorganization_requests VALUES(?1,?2,?3,?4,?5,?6)",
        params![id.to_string(), title.trim(), start, end, mask, now],
    )
    .map_err(|e| e.to_string())?;
    insert_task(
        tx,
        "llm",
        "reorganization",
        &id.to_string(),
        "1",
        start,
        end,
        None,
        true,
        now,
        "event-reorganization-v1",
    )?;
    let task_id: String = tx
        .query_row(
            "SELECT id FROM cloud_tasks WHERE target_kind='reorganization' AND target_id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Uuid::parse_str(&task_id).map_err(|e| e.to_string())
}

pub fn input(db: &Connection, task: &CloudTask) -> Result<String, String> {
    let (title, mask): (String, i64) = db
        .query_row(
            "SELECT title,source_mask FROM event_reorganization_requests WHERE id=?1",
            [&task.target_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let from = (task.start - FIT_CONTINUATION_GAP_MS).max(0);
    let to = task.end.saturating_add(FIT_CONTINUATION_GAP_MS);
    let mut stmt=db.prepare("SELECT id,started_ms,ended_ms,title,summary,revision,source_mask,boundary_locked,title_locked,understanding_json FROM named_events WHERE deleted_ms IS NULL AND started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0 ORDER BY started_ms,id").map_err(|e|e.to_string())?;
    let linked=stmt.query_map(params![from,to,mask],|r|Ok(json!({"id":r.get::<_,String>(0)?,"start_ms":r.get::<_,i64>(1)?,"end_ms":r.get::<_,i64>(2)?,"title":r.get::<_,String>(3)?,"summary":r.get::<_,Option<String>>(4)?,"revision":r.get::<_,i64>(5)?,"source_mask":r.get::<_,i64>(6)?,"boundary_locked":r.get::<_,bool>(7)?,"title_locked":r.get::<_,bool>(8)?,"understanding":r.get::<_,Option<String>>(9)?.and_then(|v|serde_json::from_str::<Value>(&v).ok())}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let start = linked
        .iter()
        .filter_map(|v| v["start_ms"].as_i64())
        .fold(from, i64::min);
    let end = linked
        .iter()
        .filter_map(|v| v["end_ms"].as_i64())
        .fold(to, i64::max);
    let mut catalog = Vec::new();
    let observations:Vec<_>=evidence(db,start,end)?.into_iter().filter(|e|source_bit(&e.source)&mask!=0).map(|e| {
        let index=e.text.as_ref().filter(|s|!s.trim().is_empty()).map(|_|understanding::index(&mut catalog,(&e).into()));
        json!({"evidence_index":index,"start_ms":e.start.max(start),"end_ms":e.end.min(end),"source":e.source,"text":e.text})
    }).collect();
    if !observations
        .iter()
        .any(|v| v["text"].as_str().is_some_and(|s| !s.trim().is_empty()))
    {
        return Err("no_coarse_text".into());
    }
    let start = linked
        .iter()
        .filter_map(|v| v["start_ms"].as_i64())
        .chain(observations.iter().filter_map(|v| v["start_ms"].as_i64()))
        .fold(task.start, i64::min);
    let end = linked
        .iter()
        .filter_map(|v| v["end_ms"].as_i64())
        .chain(observations.iter().filter_map(|v| v["end_ms"].as_i64()))
        .fold(task.end, i64::max);
    let mut context = json!({"previous_events":[],"linked_events":linked});
    understanding::prepare_context(db, &mut context, &mut catalog)?;
    for event in context["linked_events"].as_array_mut().unwrap() {
        event["summary_sentences"] = summary::indexed(event["summary"].as_str());
    }
    let value=json!({"candidate":{"start_ms":start,"end_ms":end},"user_instruction":{"title":title,"start_ms":task.start,"end_ms":task.end,"sources":sources_for(mask)},"source_mask":mask,"summary_budget":{"target_chars":900,"tolerance_chars":1100},"vocabulary":crate::vocabulary::llm_context(db)?,"context":context,"observations":observations,"evidence":catalog}).to_string();
    if value.len() > 512 * 1024 {
        return Err("reorganization_input_too_large".into());
    }
    Ok(value)
}

/// The outer publisher owns the transaction, input freshness, and task completion.
pub(super) fn apply(
    tx: &Transaction<'_>,
    _task: &CloudTask,
    input: &Value,
    result: &Value,
    now: i64,
) -> Result<usize, String> {
    let events = result["events"].as_array().ok_or("invalid_fit_schema")?;
    validate_fit_ranges(
        events,
        input["candidate"]["start_ms"]
            .as_i64()
            .ok_or("invalid_fit_input")?,
        input["candidate"]["end_ms"]
            .as_i64()
            .ok_or("invalid_fit_input")?,
    )?;
    let old = input["context"]["linked_events"]
        .as_array()
        .ok_or("invalid_fit_input")?;
    let find = |id: &str| {
        old.iter()
            .find(|e| e["id"].as_str() == Some(id))
            .ok_or("invalid_fit_event_id")
    };
    let selected = input["source_mask"].as_i64().ok_or("invalid_fit_input")?;
    let retire: Vec<String> = result
        .get("retire_event_ids")
        .map(|v| serde_json::from_value(v.clone()).map_err(|_| "invalid_fit_schema"))
        .transpose()?
        .unwrap_or_default();
    let mut seen = HashSet::new();
    for id in &retire {
        let prior = find(id)?;
        if !seen.insert(id.clone()) || prior["source_mask"].as_i64().unwrap() & !selected != 0 {
            return Err("invalid_fit_event_source".into());
        }
        tx.execute(
            "UPDATE named_events SET deleted_ms=?2,revision=revision+1 WHERE id=?1",
            params![id, now],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM event_candidate_links WHERE event_id=?1", [id])
            .map_err(|e| e.to_string())?;
    }
    let mut published = Vec::new();
    for event in events {
        let start = event["start_ms"].as_i64().unwrap();
        let end = event["end_ms"].as_i64().unwrap();
        let sources: EventSources = serde_json::from_value(event["sources"].clone())
            .map_err(|_| "invalid_fit_event_source")?;
        let mask = source_mask(sources);
        let title = event["title"].as_str().ok_or("invalid_fit_schema")?;
        validate_title(title)?;
        let existing = match event.get("event_id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) => Some(id.as_str()),
            _ => return Err("invalid_fit_event_id".into()),
        };
        let prior = existing.map(&find).transpose()?;
        if let Some(id) = existing {
            if !seen.insert(id.to_string()) {
                return Err("invalid_fit_event_id".into());
            }
        }
        // A partial-source request leaves the original ID and metadata on the
        // unselected source. The model creates a separate selected-source event.
        let protected = prior.is_some_and(|p| p["source_mask"].as_i64().unwrap() & !selected != 0);
        if protected {
            let p = prior.unwrap();
            if mask & !selected != p["source_mask"].as_i64().unwrap() & !selected
                || start != p["start_ms"].as_i64().unwrap()
                || end != p["end_ms"].as_i64().unwrap()
                || title != p["title"].as_str().unwrap()
            {
                return Err("invalid_fit_event_source".into());
            }
        } else if mask & !selected != 0 {
            return Err("invalid_fit_event_source".into());
        }
        if !protected
            && [1, 2].iter().any(|bit| {
                mask & bit != 0
                    && !input["observations"].as_array().unwrap().iter().any(|o| {
                        source_bit(o["source"].as_str().unwrap_or("")) & bit != 0
                            && o["start_ms"].as_i64().unwrap() < end
                            && o["end_ms"].as_i64().unwrap() > start
                    })
            })
        {
            return Err("invalid_fit_event_source".into());
        }
        let summary = if protected {
            prior.unwrap()["summary"].as_str().map(str::to_owned)
        } else {
            interpretation_for_scope(
                tx,
                start,
                end,
                mask,
                summary::apply(&event["summary"], None, input, start, end, mask),
            )?
        };
        let understanding = if protected {
            prior.unwrap()["understanding"]
                .is_object()
                .then(|| prior.unwrap()["understanding"].to_string())
        } else {
            interpretation_for_scope(
                tx,
                start,
                end,
                mask,
                understanding::normalize(&event["understanding"], input, None, start, end, mask),
            )?
        };
        let id = existing
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if existing.is_some() {
            // Preserve storage-format understanding on the unselected side;
            // the input context uses request-local evidence indices.
            if protected {
                tx.execute(
                    "UPDATE named_events SET source_mask=?2,revision=revision+1 WHERE id=?1",
                    params![id, mask],
                )
                .map_err(|e| e.to_string())?;
            } else {
                tx.execute("UPDATE named_events SET started_ms=?2,ended_ms=?3,source_mask=?4,title=?5,summary=?6,understanding_json=?7,origin='automatic',boundary_locked=0,title_locked=0,asr_required=1,revision=revision+1 WHERE id=?1",params![id,start,end,mask,title,summary,understanding]).map_err(|e|e.to_string())?;
            }
        } else {
            insert_generated_event(
                tx,
                &id,
                start,
                end,
                mask,
                title,
                summary,
                understanding,
                now,
            )?;
        }
        published.push((id, start, end, mask, !protected));
    }
    // Check the final event set after all explicit edits, not intermediate overlaps.
    for (id, start, end, mask, _) in &published {
        ensure_nonoverlap(tx, *start, *end, *mask, Some(id))?;
    }
    for (id, start, end, mask, _) in &published {
        for p in old.iter().filter(|p| {
            p["id"].as_str() != Some(id)
                && p["start_ms"].as_i64().unwrap() < *end
                && p["end_ms"].as_i64().unwrap() > *start
                && p["source_mask"].as_i64().unwrap() & mask != 0
        }) {
            let prior = p["id"].as_str().unwrap();
            asr::inherit(tx, prior, id)?;
            tx.execute(
                "INSERT OR IGNORE INTO event_lineage VALUES(?1,?2,?3)",
                params![prior, id, now],
            )
            .map_err(|e| e.to_string())?;
        }
        asr::inherit_coverage(tx, id, *start, *end, *mask)?;
    }
    for (id, start, end, mask, editable) in &published {
        if *editable {
            release_suppressions(tx, *start, *end, *mask)?;
        }
        queue_asr(
            tx,
            "event",
            id,
            &asr_version(*mask, *start, *end),
            *start,
            *end,
            *mask,
            true,
            now,
        )?;
    }
    Ok(events.len() + retire.len())
}

// Remove only the revised source/time projection of a previous manual suppression.
fn release_suppressions(
    tx: &Transaction<'_>,
    start: i64,
    end: i64,
    mask: i64,
) -> Result<(), String> {
    let rows = {
        let mut stmt=tx.prepare("SELECT id,started_ms,ended_ms,source_mask,created_ms FROM event_suppressions WHERE started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0").map_err(|e|e.to_string())?;
        stmt.query_map(params![start, end, mask], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    for (id, a, b, m, created) in rows {
        tx.execute("DELETE FROM event_suppressions WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        for (x, y, bits) in [
            (a, start.min(b), m),
            (end.max(a), b, m),
            (a.max(start), b.min(end), m & !mask),
        ] {
            if y > x && bits != 0 {
                tx.execute(
                    "INSERT INTO event_suppressions VALUES(?1,?2,?3,?4,?5)",
                    params![Uuid::new_v4().to_string(), x, y, bits, created],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

pub fn clear(
    tx: &Transaction<'_>,
    start: i64,
    end: i64,
    mask: i64,
    now: i64,
) -> Result<(), String> {
    // The bounded context can include neighboring material, so clear every
    // intersecting request context, including already-complete task payloads.
    let ids = {
        let mut stmt=tx.prepare("SELECT r.id FROM event_reorganization_requests r WHERE r.source_mask&?3!=0 AND ((r.started_ms<?2 AND r.ended_ms>?1) OR EXISTS(SELECT 1 FROM named_events e WHERE e.deleted_ms IS NULL AND e.source_mask&r.source_mask&?3!=0 AND e.started_ms<r.ended_ms+?6 AND e.ended_ms>max(0,r.started_ms-?6) AND e.started_ms<?5 AND e.ended_ms>?4))").map_err(|e|e.to_string())?;
        stmt.query_map(
            params![
                (start - FIT_CONTINUATION_GAP_MS).max(0),
                end.saturating_add(FIT_CONTINUATION_GAP_MS),
                mask,
                start,
                end,
                FIT_CONTINUATION_GAP_MS
            ],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    for id in ids {
        tx.execute("UPDATE cloud_tasks SET state='superseded',error_code='data_cleared',updated_ms=?2 WHERE target_kind='reorganization' AND target_id=?1",params![id,now]).map_err(|e|e.to_string())?;
        tx.execute(
            "DELETE FROM event_reorganization_requests WHERE id=?1",
            [id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
