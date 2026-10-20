//! Event membership is mutable; a submitted recognition input never is.
use super::*;
use serde::{Deserialize, Serialize};

pub const VERSION: &str = "volc-event-silence-v3";
pub const WINDOW_MS: i64 = 20 * 60 * 1000;
const MAX_CONFIRMED_SILENCE_MS: i64 = 3 * 60 * 1000;
const SETTLE_MS: i64 = 15 * 60 * 1000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part {
    pub block_id: String,
    pub path: PathBuf,
    pub frames: usize,
    pub block_start: i64,
    pub start: i64,
    pub end: i64,
    pub offset: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub parts: Vec<Part>,
    pub duration_ms: i64,
}

pub fn init(db: &Connection) -> Result<(), String> {
    crate::storage::migrate(db, "event_asr_inputs_v1", "
        CREATE TABLE asr_inputs(task_id TEXT PRIMARY KEY REFERENCES cloud_tasks(id), specification TEXT NOT NULL);
        CREATE TABLE event_asr_state(event_id TEXT PRIMARY KEY REFERENCES named_events(id), planned_version TEXT);
        CREATE TABLE event_asr_refs(event_id TEXT NOT NULL REFERENCES named_events(id),task_id TEXT NOT NULL REFERENCES cloud_tasks(id),
          started_ms INTEGER NOT NULL,ended_ms INTEGER NOT NULL,PRIMARY KEY(event_id,task_id),CHECK(ended_ms>started_ms));
        CREATE INDEX event_asr_refs_task ON event_asr_refs(task_id);
        INSERT INTO event_asr_state SELECT id,printf('%d:%d:%d',source_mask,started_ms,ended_ms) FROM named_events WHERE deleted_ms IS NULL;
        INSERT INTO event_asr_refs SELECT e.id,t.id,max(e.started_ms,t.started_ms),min(e.ended_ms,t.ended_ms)
          FROM named_events e JOIN cloud_tasks t ON t.target_id=e.id AND t.target_kind='event' AND t.provider='asr'
          WHERE e.deleted_ms IS NULL AND t.state!='superseded' AND t.started_ms<e.ended_ms AND t.ended_ms>e.started_ms
          AND t.target_version=printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms);
        UPDATE event_asr_state SET planned_version=NULL WHERE event_id IN
          (SELECT r.event_id FROM event_asr_refs r JOIN cloud_tasks t ON t.id=r.task_id WHERE t.submit_attempts=0 AND t.state='pending');
        UPDATE cloud_tasks SET state='superseded' WHERE id IN
          (SELECT task_id FROM event_asr_refs) AND submit_attempts=0 AND state='pending';
        DELETE FROM event_asr_refs WHERE task_id IN(SELECT id FROM cloud_tasks WHERE state='superseded');
    ")?;
    // All content/identity readers see the current event membership while the
    // network worker and billing continue to use the original physical task.
    db.execute_batch("DROP VIEW IF EXISTS transcript_tasks;
      CREATE VIEW transcript_tasks AS
      SELECT t.id,t.provider,'event' AS target_kind,e.id AS target_id,
        printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms) AS target_version,
        t.model_id,t.processor_version,max(r.started_ms,e.started_ms) AS started_ms,min(r.ended_ms,e.ended_ms) AS ended_ms,t.source,t.manual,t.state,
        t.attempts,t.submit_attempts,t.next_attempt_ms,t.remote_id,t.error_code,t.created_ms,t.updated_ms
      FROM event_asr_refs r JOIN named_events e ON e.id=r.event_id JOIN cloud_tasks t ON t.id=r.task_id
      WHERE e.deleted_ms IS NULL AND t.state!='superseded' AND r.started_ms<e.ended_ms AND r.ended_ms>e.started_ms
        AND ((t.source='microphone' AND e.source_mask&1!=0) OR(t.source='system' AND e.source_mask&2!=0))
      UNION ALL SELECT t.* FROM cloud_tasks t WHERE t.target_kind!='event' OR NOT EXISTS
        (SELECT 1 FROM event_asr_state s WHERE s.event_id=t.target_id);")
        .map_err(|e|e.to_string())
}

pub fn input(db: &Connection, id: Uuid) -> Result<Option<Input>, String> {
    let value: Option<String> = db
        .query_row(
            "SELECT specification FROM asr_inputs WHERE task_id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    value
        .map(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
        .transpose()
}

pub fn schedule(
    tx: &Transaction<'_>,
    kind: &str,
    target: &str,
    version: &str,
    start: i64,
    end: i64,
    mask: i64,
    manual: bool,
    now: i64,
    processor: &str,
) -> Result<usize, String> {
    if kind == "event" {
        tx.execute("INSERT INTO event_asr_state(event_id,planned_version) VALUES(?1,NULL) ON CONFLICT(event_id) DO UPDATE SET planned_version=NULL",[target]).map_err(|e|e.to_string())?;
        // Keep historical membership even when a boundary temporarily excludes
        // it. The view clips current applicability; reverting must not repay.
        cancel_orphans(tx, now)?;
        if !manual {
            return Ok(0);
        }
    }
    let count = plan(
        tx, kind, target, version, start, end, mask, manual, now, processor,
    )?;
    if kind == "event" {
        tx.execute(
            "UPDATE event_asr_state SET planned_version=?2 WHERE event_id=?1",
            params![target, version],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(count)
}

/// Projection and scheduling must agree on the same next-event horizon.
/// Failed work outside this range cannot explain why this event is waiting.
pub fn settling_state(
    db: &Connection,
    id: &str,
    start: i64,
    end: i64,
    mask: i64,
) -> Result<Option<&'static str>, String> {
    let horizon:i64=db.query_row("SELECT min(?1,coalesce(min(started_ms),?1)) FROM named_events WHERE deleted_ms IS NULL AND id!=?2 AND started_ms>=?3 AND source_mask&?4=?4",params![end+SETTLE_MS,id,end,mask],|r|r.get(0)).map_err(|e|e.to_string())?;
    let busy: bool=db.query_row("SELECT EXISTS(SELECT 1 FROM audio_blocks b WHERE b.started_ms<?3 AND b.ended_ms>?2
          AND ((b.source='microphone' AND ?4&1!=0) OR (b.source='system' AND ?4&2!=0))
          AND (b.analysis_state IN('pending','processing','sealing','failed') OR b.retention_decision='pending_vad_v2'))
          OR EXISTS(SELECT 1 FROM coarse_jobs WHERE started_ms<?3 AND ended_ms>?2 AND state IN('pending','processing','failed') AND ((source='microphone' AND ?4&1!=0) OR(source='system' AND ?4&2!=0)))
          OR EXISTS(SELECT 1 FROM candidate_windows WHERE started_ms<?3 AND ended_ms>?2 AND state IN('pending','processing','failed'))
          OR EXISTS(SELECT 1 FROM coarse_segments c WHERE c.started_ms<?3 AND c.ended_ms>?5
             AND ((c.source='microphone' AND ?4&1!=0) OR (c.source='system' AND ?4&2!=0))
             AND NOT EXISTS(SELECT 1 FROM candidate_windows w WHERE w.state='complete' AND w.started_ms<=c.started_ms AND w.ended_ms>=c.ended_ms)
             AND NOT EXISTS(SELECT 1 FROM named_events n WHERE n.deleted_ms IS NULL AND n.id!=?1
                AND n.started_ms<=c.started_ms AND n.ended_ms>=c.ended_ms
                AND ((c.source='microphone' AND n.source_mask&1!=0) OR (c.source='system' AND n.source_mask&2!=0))))",
          params![id,start,horizon,mask,end],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !busy {
        return Ok(None);
    }
    let fit_failed: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM candidate_windows WHERE started_ms<?2 AND ended_ms>?1 AND state='failed')",
        params![start,horizon], |r| r.get(0)).map_err(|e|e.to_string())?;
    if fit_failed {
        return Ok(Some("fit_failed"));
    }
    let analysis_failed: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE started_ms<?2 AND ended_ms>?1
          AND ((source='microphone' AND ?3&1!=0) OR(source='system' AND ?3&2!=0))
          AND (analysis_state='failed' OR(analysis_state='pending' AND analysis_attempts>=3)))
          OR EXISTS(SELECT 1 FROM coarse_jobs WHERE started_ms<?2 AND ended_ms>?1 AND state='failed'
          AND ((source='microphone' AND ?3&1!=0) OR(source='system' AND ?3&2!=0)))",
            params![start, horizon, mask],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(if analysis_failed {
        "analysis_failed"
    } else {
        "settling_asr"
    }))
}

pub fn refresh(db: &mut Connection, now: i64) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let events = {
        let mut stmt=tx.prepare("SELECT e.id,e.started_ms,e.ended_ms,e.source_mask FROM named_events e JOIN event_asr_state s ON s.event_id=e.id
        WHERE e.deleted_ms IS NULL AND coalesce(s.planned_version,'')!=printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms)
        AND (e.ended_ms<=?1 OR EXISTS(SELECT 1 FROM named_events n WHERE n.deleted_ms IS NULL AND n.started_ms>=e.ended_ms AND n.started_ms<=?2 AND n.source_mask&e.source_mask=e.source_mask)) ORDER BY e.ended_ms").map_err(|e|e.to_string())?;
        stmt.query_map(params![now - SETTLE_MS, now], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    let mut planned = 0;
    for (id, start, end, mask) in events {
        if settling_state(&tx, &id, start, end, mask)?.is_some() {
            continue;
        }
        planned += 1;
        let version = asr_version(mask, start, end);
        plan(
            &tx, "event", &id, &version, start, end, mask, false, now, VERSION,
        )?;
        tx.execute(
            "UPDATE event_asr_state SET planned_version=?2 WHERE event_id=?1",
            params![id, version],
        )
        .map_err(|e| e.to_string())?;
        if planned >= 32 {
            break;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

fn plan(
    tx: &Transaction<'_>,
    kind: &str,
    target: &str,
    version: &str,
    start: i64,
    end: i64,
    mask: i64,
    manual: bool,
    now: i64,
    processor: &str,
) -> Result<usize, String> {
    let mut count = 0;
    for source in ["microphone", "system"] {
        if source_bit(source) & mask == 0 {
            continue;
        }
        let mut covered = Vec::new();
        let mut stmt=tx.prepare("SELECT t.id,t.started_ms,t.ended_ms FROM transcript_tasks t WHERE provider='asr' AND target_kind=?1 AND target_id=?2 AND target_version=?3
           AND source=?4 AND state!='superseded' ORDER BY started_ms").map_err(|e|e.to_string())?;
        for row in stmt
            .query_map(params![kind, target, version, source], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?
        {
            let (id, a, b) = row.map_err(|e| e.to_string())?;
            if let Some(input) = input(tx, Uuid::parse_str(&id).map_err(|e| e.to_string())?)? {
                covered.extend(
                    input
                        .parts
                        .iter()
                        .map(|p| (p.start.max(a), p.end.min(b)))
                        .filter(|(a, b)| b > a),
                );
            } else {
                covered.push((a, b));
            }
        }
        covered.sort_unstable();
        let mut stmt=tx.prepare("SELECT id,started_ms,ended_ms,frames,media_path FROM audio_blocks WHERE source=?1 AND media_path IS NOT NULL AND started_ms<?3 AND ended_ms>?2 ORDER BY started_ms,id").map_err(|e|e.to_string())?;
        let blocks = stmt
            .query_map(params![source, start, end], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)? as usize,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut parts: Vec<Part> = Vec::new();
        let mut duration = 0;
        let mut source_cursor = start;
        for (id, a, b, frames, path) in blocks {
            let mut spans = vec![(a.max(start).max(source_cursor), b.min(end))];
            source_cursor = source_cursor.max(b.min(end));
            for &(x, y) in &covered {
                spans = spans
                    .into_iter()
                    .flat_map(|(a, b)| {
                        if y <= a || x >= b {
                            vec![(a, b)]
                        } else {
                            vec![(a, x.min(b)), (y.max(a), b)]
                        }
                    })
                    .filter(|(a, b)| b > a)
                    .collect();
            }
            for (from, to) in spans.into_iter().filter(|(a, b)| b > a) {
                let gap = parts.last().map(|p| from - p.end).unwrap_or(0).max(0);
                // A synthetic separator does not make provider word timing
                // invertible: a single word can span multiple source runs.
                // Preserve the entire real pause. Only positively established
                // no-speech coverage may bridge a missing-media interval.
                let can_join = gap == 0 || confirmed_silence(tx, source, from - gap, from)?;
                let encoded_gap = gap;
                if !parts.is_empty()
                    && (!can_join || duration + encoded_gap + to - from > WINDOW_MS)
                {
                    save(
                        tx, kind, target, version, source, manual, now, processor, &parts, duration,
                    )?;
                    count += 1;
                    parts.clear();
                    duration = 0;
                }
                if !parts.is_empty() {
                    duration += encoded_gap;
                }
                parts.push(Part {
                    block_id: id.clone(),
                    path: PathBuf::from(&path),
                    frames,
                    block_start: a,
                    start: from,
                    end: to,
                    offset: duration,
                });
                duration += to - from;
            }
        }
        if !parts.is_empty() {
            save(
                tx, kind, target, version, source, manual, now, processor, &parts, duration,
            )?;
            count += 1;
        }
    }
    Ok(count)
}

fn confirmed_silence(db: &Connection, source: &str, start: i64, end: i64) -> Result<bool, String> {
    if end - start > MAX_CONFIRMED_SILENCE_MS {
        return Ok(false);
    }
    let forbidden: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM media_gaps WHERE source=?1
           AND started_ms<?3 AND coalesce(ended_ms,?3)>?2)
         OR EXISTS(SELECT 1 FROM clear_ranges WHERE from_ms<?3 AND to_ms>?2
           AND mode IN ('all_data','raw_audio')
           AND source_mask & ?4 != 0)",
            params![source, start, end, source_bit(source)],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if forbidden {
        return Ok(false);
    }
    // Preserve the existing sub-frame clock tolerance, not unknown pauses.
    if end - start <= 10 {
        return Ok(true);
    }
    let mut stmt = db
        .prepare(
            "SELECT started_ms,ended_ms FROM audio_blocks WHERE source=?1
           AND started_ms<?3 AND ended_ms>?2
           AND media_path IS NULL
           AND retention_decision IN ('discard_no_speech_v2','discard_digital_silence_v1')
           ORDER BY started_ms,ended_ms",
        )
        .map_err(|e| e.to_string())?;
    let spans = stmt
        .query_map(params![source, start, end], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut covered = start;
    for span in spans {
        let (a, b) = span.map_err(|e| e.to_string())?;
        if a > covered {
            return Ok(false);
        }
        covered = covered.max(b);
        if covered >= end {
            return Ok(true);
        }
    }
    Ok(false)
}

fn save(
    tx: &Transaction<'_>,
    kind: &str,
    target: &str,
    version: &str,
    source: &str,
    manual: bool,
    now: i64,
    processor: &str,
    parts: &[Part],
    duration: i64,
) -> Result<(), String> {
    let start = parts.first().ok_or("empty asr plan")?.start;
    let end = parts.last().unwrap().end;
    insert_task(
        tx,
        "asr",
        kind,
        target,
        version,
        start,
        end,
        Some(source),
        manual,
        now,
        processor,
    )?;
    let id:String=tx.query_row("SELECT id FROM cloud_tasks WHERE provider='asr' AND target_kind=?1 AND target_id=?2 AND target_version=?3 AND started_ms=?4 AND ended_ms=?5 AND source=?6 AND processor_version=?7",params![kind,target,version,start,end,source,processor],|r|r.get(0)).map_err(|e|e.to_string())?;
    let json = serde_json::to_string(&Input {
        parts: parts.to_vec(),
        duration_ms: duration,
    })
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT OR IGNORE INTO asr_inputs VALUES(?1,?2)",
        params![id, json],
    )
    .map_err(|e| e.to_string())?;
    if kind == "event" {
        tx.execute(
            "INSERT OR IGNORE INTO event_asr_refs VALUES(?1,?2,?3,?4)",
            params![target, id, start, end],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn inherit(tx: &Transaction<'_>, old: &str, new: &str) -> Result<(), String> {
    // Inherit the original recognition history, not the child's initial crop.
    // Current applicability is clipped by transcript_tasks. A later extension
    // can then reuse even a completed task wholly outside the initial child.
    tx.execute("INSERT INTO event_asr_refs SELECT ?2,r.task_id,r.started_ms,r.ended_ms
      FROM event_asr_refs r JOIN cloud_tasks t ON t.id=r.task_id WHERE r.event_id=?1 AND t.state!='superseded'
      ON CONFLICT(event_id,task_id) DO UPDATE SET started_ms=min(event_asr_refs.started_ms,excluded.started_ms),ended_ms=max(event_asr_refs.ended_ms,excluded.ended_ms)",params![old,new]).map_err(|e|e.to_string())?;
    Ok(())
}

pub fn current(db: &Connection, task: &CloudTask) -> Result<Option<bool>, String> {
    if task.target_kind != "event" {
        return Ok(None);
    }
    let managed: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM event_asr_state WHERE event_id=?1)",
            [&task.target_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !managed {
        return Ok(None);
    }
    let active: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM event_asr_refs r JOIN named_events e ON e.id=r.event_id
      WHERE r.task_id=?1 AND e.deleted_ms IS NULL AND r.started_ms<e.ended_ms AND r.ended_ms>e.started_ms
      AND ((?2='microphone' AND e.source_mask&1!=0) OR(?2='system' AND e.source_mask&2!=0)))",
            params![task.id.to_string(),task.source],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(active))
}

/// Cleanup can replace a physical block without changing the selected samples.
/// Resolve those immutable source-clock ranges against the current media owner;
/// the persisted upload clock and the result mapping never change.
pub fn resolve_media(input: &Input, media: &[AudioPart]) -> Result<Input, String> {
    let mut parts = Vec::new();
    for original in &input.parts {
        let mut cursor = original.start;
        for block in media {
            let a = cursor.max(block.start);
            let b = original.end.min(block.end);
            if b <= a {
                continue;
            }
            if a > cursor + 10 {
                return Err("ASR source audio unavailable".into());
            }
            parts.push(Part {
                block_id: original.block_id.clone(),
                path: block.path.clone(),
                frames: block.frames,
                block_start: block.start,
                start: a,
                end: b,
                offset: original.offset + a - original.start,
            });
            cursor = b;
            if cursor >= original.end {
                break;
            }
        }
        if cursor + 10 < original.end {
            return Err("ASR source audio unavailable".into());
        }
    }
    Ok(Input {
        parts,
        duration_ms: input.duration_ms,
    })
}

pub fn assemble(directory: &Path, input: &Input) -> Result<Vec<u8>, String> {
    if input.duration_ms <= 0 || input.duration_ms > WINDOW_MS {
        return Err("ASR task duration invalid".into());
    }
    let scratch = tempfile::Builder::new()
        .prefix(".asr-ogg-")
        .rand_bytes(12)
        .tempdir_in(directory)
        .map_err(|e| e.to_string())?;
    let mut encoded = crate::media::OpusBlock::create(scratch.path(), Uuid::new_v4())?;
    // At most one ten-second slab and one original block are decoded at once.
    let mut chunk = 0;
    while chunk < input.duration_ms {
        let end = (chunk + 10_000).min(input.duration_ms);
        let mut pcm = vec![0.0; ((end - chunk) * 48) as usize];
        for part in &input.parts {
            let a = part.offset.max(chunk);
            let b = (part.offset + part.end - part.start).min(end);
            if b <= a {
                continue;
            }
            if part.path.components().count() != 3
                || part
                    .path
                    .components()
                    .next()
                    .is_none_or(|c| c.as_os_str() != "media")
                || part.path.extension().is_none_or(|e| e != "opus")
                || part.frames == 0
                || part.frames > 48_000 * 11
            {
                return Err("invalid media manifest".into());
            }
            let audio = crate::media::decode_block(&directory.join(&part.path), part.frames)?;
            let offset = ((part.start - part.block_start + a - part.offset) * 48) as usize;
            let n = ((b - a) * 48) as usize;
            let dst = ((a - chunk) * 48) as usize;
            if offset + n > audio.len() {
                return Err("ASR source audio unavailable".into());
            }
            pcm[dst..dst + n].copy_from_slice(&audio[offset..offset + n]);
        }
        for frame in pcm.chunks(960) {
            encoded.push(frame)?;
        }
        chunk = end;
    }
    let file = encoded.seal()?;
    if file.bytes > 10_000_000 {
        return Err("asr_audio_size".into());
    }
    std::fs::read(file.path).map_err(|e| e.to_string())
}

// A run is a linear part of the request clock, including ordinary short pauses.
fn runs(input: &Input) -> Vec<(i64, i64, i64)> {
    let mut output: Vec<(i64, i64, i64)> = Vec::new();
    for p in &input.parts {
        let delta = p.start - p.offset;
        if let Some(last) = output.last_mut() {
            if (last.2 - delta).abs() <= 10 {
                last.1 = p.offset + p.end - p.start;
                continue;
            }
        }
        output.push((p.offset, p.offset + p.end - p.start, delta));
    }
    output
}

pub fn map(
    input: &Input,
    items: &[crate::cloud::AsrUtterance],
) -> Result<Vec<crate::cloud::AsrUtterance>, String> {
    let runs = runs(input);
    let mut output = Vec::new();
    for item in items {
        if item.start_ms < 0 || item.end_ms > input.duration_ms || item.end_ms <= item.start_ms {
            return Err("provider_invalid_timing".into());
        }
        if let Some(&(_, _, delta)) = runs
            .iter()
            .find(|&&(a, b, _)| item.start_ms >= a && item.end_ms <= b)
        {
            let mut mapped = item.clone();
            mapped.start_ms += delta;
            mapped.end_ms += delta;
            output.push(mapped);
            continue;
        }
        let words = item
            .words
            .as_ref()
            .and_then(serde_json::Value::as_array)
            .ok_or("provider_unmapped_timing_words_missing")?;
        let mut cursor = 0;
        let mut fragments: Vec<(usize, i64, i64, i64)> = Vec::new();
        for word in words {
            let text = word["text"]
                .as_str()
                .ok_or("provider_unmapped_timing_word_text_missing")?;
            if text.is_empty() {
                return Err("provider_unmapped_timing_word_text_empty".into());
            }
            let offset = item.text[cursor..]
                .find(text)
                .map(|p| p + cursor)
                .ok_or("provider_unmapped_timing_text_alignment")?;
            if item.text[cursor..offset].chars().any(char::is_alphanumeric) {
                return Err("provider_unmapped_timing_text_gap".into());
            }
            cursor = offset + text.len();
            let a = word["start_time"]
                .as_i64()
                .ok_or("provider_unmapped_timing_word_start_missing")?;
            let b = word["end_time"]
                .as_i64()
                .ok_or("provider_unmapped_timing_word_end_missing")?;
            if a == -1 && b == -1 && !text.chars().any(char::is_alphanumeric) {
                continue;
            }
            if a < item.start_ms || b > item.end_ms || b <= a {
                return Err("provider_unmapped_timing_word_bounds".into());
            }
            let &(rstart, rend, delta) = runs
                .iter()
                .find(|&&(x, y, _)| a >= x && b <= y)
                .ok_or("provider_unmapped_timing_word_in_separator")?;
            let _ = (rstart, rend);
            if let Some(last) = fragments.last_mut() {
                if a + delta < last.2 {
                    return Err("provider_unmapped_timing_word_order".into());
                }
                if delta == last.3 {
                    last.2 = b + delta;
                    continue;
                }
            }
            fragments.push((offset, a + delta, b + delta, delta));
        }
        if fragments.is_empty() || item.text[cursor..].chars().any(char::is_alphanumeric) {
            return Err("provider_unmapped_timing_text_incomplete".into());
        }
        // Validate the original envelope too: bad leading/trailing timing is
        // not repaired by clipping to otherwise plausible word timestamps.
        let first = runs
            .iter()
            .find(|&&(a, b, _)| item.start_ms >= a && item.start_ms < b)
            .ok_or("provider_unmapped_timing_start_in_separator")?;
        let last = runs
            .iter()
            .find(|&&(a, b, _)| item.end_ms > a && item.end_ms <= b)
            .ok_or("provider_unmapped_timing_end_in_separator")?;
        if first.2 != fragments[0].3 || last.2 != fragments.last().unwrap().3 {
            return Err("provider_unmapped_timing_envelope_mismatch".into());
        }
        fragments[0].0 = 0;
        for (i, &(offset, a, b, _)) in fragments.iter().enumerate() {
            let to = fragments.get(i + 1).map(|v| v.0).unwrap_or(item.text.len());
            output.push(crate::cloud::AsrUtterance {
                start_ms: a,
                end_ms: b,
                text: item.text[offset..to].to_owned(),
                speaker: item.speaker.clone(),
                words: None,
            });
        }
    }
    Ok(output)
}

/// Only obsolete ownership is cancelled. A cropped completed input is retained
/// so restoring its boundary can reuse it without another paid request.
pub fn cancel_orphans(tx: &Transaction<'_>, now: i64) -> Result<(), String> {
    tx.execute("UPDATE cloud_tasks SET state='superseded',updated_ms=?1 WHERE provider='asr' AND target_kind='event'
      AND state IN('pending','processing','submitted','failed') AND EXISTS(SELECT 1 FROM event_asr_state WHERE event_id=cloud_tasks.target_id)
      AND NOT EXISTS(SELECT 1 FROM event_asr_refs r JOIN named_events e ON e.id=r.event_id WHERE r.task_id=cloud_tasks.id
        AND e.deleted_ms IS NULL AND r.started_ms<e.ended_ms AND r.ended_ms>e.started_ms
        AND ((cloud_tasks.source='microphone' AND e.source_mask&1!=0) OR(cloud_tasks.source='system' AND e.source_mask&2!=0)))",[now]).map_err(|e|e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests;
