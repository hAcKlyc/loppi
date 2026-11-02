//! Explicit user completion belongs to the event; processing stays in cloud_tasks.
use super::*;

pub fn init(db: &Connection) -> Result<(), String> {
    crate::storage::migrate(db, "event_completion_v1", "
        ALTER TABLE named_events ADD COLUMN completed_ms INTEGER;
        ALTER TABLE event_reorganization_requests ADD COLUMN completion_event_id TEXT;
        ALTER TABLE event_reorganization_requests ADD COLUMN expected_revision INTEGER;
        CREATE INDEX reorganization_completion_event ON event_reorganization_requests(completion_event_id);
        CREATE TRIGGER completed_event_audio_insert AFTER INSERT ON audio_blocks WHEN NEW.media_path IS NOT NULL BEGIN
          UPDATE event_asr_state SET planned_version=NULL WHERE event_id IN
            (SELECT id FROM named_events WHERE completed_ms IS NOT NULL AND deleted_ms IS NULL
             AND started_ms<NEW.ended_ms AND ended_ms>NEW.started_ms
             AND (source_mask & CASE NEW.source WHEN 'microphone' THEN 1 ELSE 2 END)!=0);
        END;
        CREATE TRIGGER completed_event_audio_available AFTER UPDATE OF media_path ON audio_blocks
          WHEN OLD.media_path IS NULL AND NEW.media_path IS NOT NULL BEGIN
          UPDATE event_asr_state SET planned_version=NULL WHERE event_id IN
            (SELECT id FROM named_events WHERE completed_ms IS NOT NULL AND deleted_ms IS NULL
             AND started_ms<NEW.ended_ms AND ended_ms>NEW.started_ms
             AND (source_mask & CASE NEW.source WHEN 'microphone' THEN 1 ELSE 2 END)!=0);
        END;
    ")
}

pub fn completed_at(db: &Connection, id: &str) -> Result<Option<i64>, String> {
    db.query_row(
        "SELECT completed_ms FROM named_events WHERE id=?1 AND deleted_ms IS NULL",
        [id],
        |r| r.get(0),
    )
    .optional()
    .map(|v| v.flatten())
    .map_err(|e| e.to_string())
}

pub fn task_state(db: &Connection, id: &str) -> Result<Option<String>, String> {
    db.query_row(
        "SELECT t.state FROM event_reorganization_requests r JOIN cloud_tasks t
        ON t.target_kind='reorganization' AND t.target_id=r.id WHERE r.completion_event_id=?1
        ORDER BY t.created_ms DESC,t.id DESC LIMIT 1",
        [id],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn ranges(db: &Connection, start: i64, end: i64) -> Result<Vec<(i64, i64, i64)>, String> {
    let mut stmt = db
        .prepare(
            "SELECT started_ms,ended_ms,source_mask FROM named_events
        WHERE completed_ms IS NOT NULL AND deleted_ms IS NULL AND started_ms<?2 AND ended_ms>?1
        ORDER BY started_ms,id",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_map(params![start, end], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

pub fn finish(tx: &Transaction<'_>, id: &str, now: i64) -> Result<(), String> {
    let (start,end,mask): (i64,i64,i64) = tx.query_row(
        "SELECT started_ms,ended_ms,source_mask FROM named_events WHERE id=?1 AND deleted_ms IS NULL",
        [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    tx.execute(
        "UPDATE named_events SET completed_ms=?2,boundary_locked=1,revision=revision+1 WHERE id=?1",
        params![id, now],
    )
    .map_err(|e| e.to_string())?;
    asr::inherit_coverage(tx, id, start, end, mask)?;
    queue_asr(
        tx,
        "event",
        id,
        &asr_version(mask, start, end),
        start,
        end,
        mask,
        true,
        now,
    )?;
    Ok(())
}

// Only an explicit queue retry accepts a newer target revision. Old model
// responses still have to match the complete input snapshot at publication.
pub fn prepare_retry(tx: &Transaction<'_>, request_id: &str) -> Result<(), String> {
    let target: Option<String> = tx
        .query_row(
            "SELECT completion_event_id FROM event_reorganization_requests WHERE id=?1",
            [request_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if let Some(id) = target {
        let revision: i64 = tx
            .query_row(
                "SELECT revision FROM named_events WHERE id=?1 AND deleted_ms IS NULL",
                [&id],
                |r| r.get(0),
            )
            .map_err(|_| "task is no longer current".to_owned())?;
        tx.execute(
            "UPDATE event_reorganization_requests SET expected_revision=?2 WHERE id=?1",
            params![request_id, revision],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn confirm(
    tx: &Transaction<'_>,
    request_id: Uuid,
    id: Uuid,
    expected: i64,
    start: i64,
    end: i64,
    now: i64,
) -> Result<Uuid, String> {
    let (old_start, old_end, mask, title) = active_event(tx, id, expected)?;
    validate_range(start, end)?;
    if end > now {
        return Err("event_completion_future_range".into());
    }
    if matches!(
        task_state(tx, &id.to_string())?.as_deref(),
        Some("pending" | "processing")
    ) {
        return Err("event_completion_pending".into());
    }
    if start == old_start && end == old_end {
        finish(tx, &id.to_string(), now)?;
        return Ok(id);
    }
    let task_id =
        reorganization::enqueue(tx, request_id, start, end, &title, sources_for(mask), now)?;
    tx.execute("UPDATE event_reorganization_requests SET completion_event_id=?2,expected_revision=?3 WHERE id=?1",
        params![request_id.to_string(),id.to_string(),expected]).map_err(|e|e.to_string())?;
    Ok(task_id)
}

#[cfg(test)]
mod tests;
