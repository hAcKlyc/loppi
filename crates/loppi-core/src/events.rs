//! The event axis is a versioned projection over immutable source media and
//! coarse text. Candidate windows are scheduling inputs, never user events.
use loppi_protocol::{
    AudioSource, CloudProvider, CloudQueueState, CloudTaskView, CoarseSegment, EventSources,
    PreciseSegment, TimeGap, TimelineItem, TimelinePreview, TranscriptionProgress,
    UnrecognizedAudio,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use sherpa_onnx::LinearResampler;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[cfg(test)]
mod activity_tests;
pub(crate) mod asr;
pub mod completion;
#[cfg(test)]
mod gui_read_tests;
pub mod rebuild;
pub mod reorganization;
mod summary;
mod understanding;
#[cfg(test)]
mod understanding_replay_tests;
#[cfg(test)]
mod understanding_tests;

struct CandidatePolicy {
    quiet_cut_ms: i64,
    max_window_ms: i64,
    version: &'static str,
}
const CANDIDATE_POLICY: CandidatePolicy = CandidatePolicy {
    quiet_cut_ms: 5 * 60 * 1000,
    max_window_ms: 15 * 60 * 1000,
    // Keep the evidence hash format stable: a prompt/window-size change alone
    // must not re-enqueue completed historical inputs.
    version: "candidate-v1",
};
const FIT_POLICY_VERSION: &str = "fit-v3";
const FIT_CONTINUATION_GAP_MS: i64 = 30 * 60 * 1000;
const FIT_CONTEXT_TAIL_MS: i64 = 3 * 60 * 1000;
const FIT_CONTEXT_MAX_LINES: usize = 24;
const FIT_CONTEXT_TEXT_CHARS: usize = 400;
// Only work that can still produce local evidence delays automatic fitting.
// Terminal failures remain visible but cannot hold the LLM queue forever.
const FIT_LOCAL_PENDING_RANGES: &str = "
    SELECT started_ms,ended_ms FROM coarse_jobs WHERE state IN ('pending','processing')
    UNION ALL
    SELECT started_ms,ended_ms FROM audio_blocks
    WHERE analysis_state IN ('processing','sealing')
       OR (analysis_state='pending' AND analysis_attempts<3)
       OR (analysis_state='complete' AND retention_decision='pending_vad_v2')";
const ASR_ADAPTER_VERSION: &str = asr::VERSION;
const ASR_WINDOW_MS: i64 = 15 * 60 * 1000;
pub(crate) const MAX_ASR_INTERIOR_SILENCE_MS: i64 = 3_000;

// Rebuild the CHECK constraint once while preserving task IDs and all references.
// legacy_alter_table keeps existing dependent views bound to the final table name.
fn init_cloud_tasks(db: &Connection) -> Result<(), String> {
    const SCHEMA: &str = r#"CREATE TABLE IF NOT EXISTS cloud_tasks (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL CHECK(provider IN ('llm','asr')),
            target_kind TEXT NOT NULL CHECK(target_kind IN ('candidate','event','range','screenshot','reorganization')),
            target_id TEXT NOT NULL,
            target_version TEXT NOT NULL,
            model_id TEXT NOT NULL,
            processor_version TEXT NOT NULL,
            started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL,
            source TEXT,
            manual INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL CHECK(state IN ('pending','processing','submitted','complete','failed','superseded')),
            attempts INTEGER NOT NULL DEFAULT 0,
            submit_attempts INTEGER NOT NULL DEFAULT 0,
            next_attempt_ms INTEGER NOT NULL DEFAULT 0,
            remote_id TEXT,
            error_code TEXT,
            created_ms INTEGER NOT NULL,
            updated_ms INTEGER NOT NULL,
            CHECK(ended_ms > started_ms)
         );"#;
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    let old: bool = db
        .query_row(
            "SELECT instr(sql, ?1)=0 FROM sqlite_master WHERE type='table' AND name='cloud_tasks'",
            ["'reorganization'"],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if old {
        let legacy: bool = db
            .pragma_query_value(None, "legacy_alter_table", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let foreign_keys: bool = db
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        // SQLite's documented table rebuild procedure disables FK enforcement
        // outside the migration transaction; IDs and referencing rows are copied unchanged.
        db.pragma_update(None, "foreign_keys", false)
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "legacy_alter_table", true)
            .map_err(|e| e.to_string())?;
        let has_retry_budget: bool = db
            .query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('cloud_tasks') WHERE name='retry_failures')", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let retry_column = if has_retry_budget {
            "ALTER TABLE cloud_tasks_new ADD COLUMN retry_failures INTEGER NOT NULL DEFAULT 0;"
        } else {
            ""
        };
        let result = crate::storage::migrate(
            db,
            "reorganization_cloud_tasks_v1",
            &format!(
                "{} {retry_column} INSERT INTO cloud_tasks_new SELECT * FROM cloud_tasks; DROP TABLE cloud_tasks; ALTER TABLE cloud_tasks_new RENAME TO cloud_tasks;",
                SCHEMA.replace("IF NOT EXISTS cloud_tasks", "cloud_tasks_new")
            ),
        );
        db.pragma_update(None, "legacy_alter_table", legacy)
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "foreign_keys", foreign_keys)
            .map_err(|e| e.to_string())?;
        result?;
    }
    Ok(())
}

pub fn init(db: &Connection) -> Result<(), String> {
    init_cloud_tasks(db)?;
    reorganization::init(db)?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud_queues (
            provider TEXT PRIMARY KEY CHECK(provider IN ('llm','asr')),
            configured INTEGER NOT NULL DEFAULT 0,
            model TEXT NOT NULL,
            paused INTEGER NOT NULL DEFAULT 0
         );
         INSERT OR IGNORE INTO cloud_queues(provider,model) VALUES ('llm','deepseek-flash');
         INSERT OR IGNORE INTO cloud_queues(provider,model) VALUES ('asr','bigmodel');
         CREATE TABLE IF NOT EXISTS cloud_credential_refs (
            provider TEXT PRIMARY KEY REFERENCES cloud_queues(provider),
            account TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS candidate_scan_state (
            id INTEGER PRIMARY KEY CHECK(id=1),
            from_ms INTEGER NOT NULL
         );
         INSERT OR IGNORE INTO candidate_scan_state VALUES (1,0);
         CREATE INDEX IF NOT EXISTS coarse_segments_ended ON coarse_segments(ended_ms);
         CREATE INDEX IF NOT EXISTS audio_blocks_started ON audio_blocks(started_ms);
         CREATE INDEX IF NOT EXISTS audio_blocks_ended ON audio_blocks(ended_ms);
         CREATE TABLE IF NOT EXISTS candidate_windows (
            id TEXT PRIMARY KEY,
            started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL,
            input_hash TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('pending','processing','complete','failed')),
            error_code TEXT,
            CHECK(ended_ms > started_ms)
         );
         CREATE INDEX IF NOT EXISTS candidate_windows_time ON candidate_windows(started_ms);
         CREATE TABLE IF NOT EXISTS named_events (
            id TEXT PRIMARY KEY,
            started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL,
            title TEXT NOT NULL,
            source_mask INTEGER NOT NULL CHECK(source_mask IN (1,2,3)),
            revision INTEGER NOT NULL,
            origin TEXT NOT NULL CHECK(origin IN ('automatic','manual')),
            boundary_locked INTEGER NOT NULL,
            title_locked INTEGER NOT NULL,
            asr_required INTEGER NOT NULL,
            deleted_ms INTEGER,
            created_ms INTEGER NOT NULL,
            CHECK(ended_ms > started_ms)
         );
         CREATE INDEX IF NOT EXISTS named_events_time ON named_events(started_ms,ended_ms);
         CREATE TABLE IF NOT EXISTS event_candidate_links (
            candidate_id TEXT NOT NULL,
            slot INTEGER NOT NULL,
            event_id TEXT NOT NULL,
            PRIMARY KEY(candidate_id,slot)
         );
         CREATE INDEX IF NOT EXISTS event_candidate_links_event ON event_candidate_links(event_id);
         CREATE TABLE IF NOT EXISTS event_suppressions (
            id TEXT PRIMARY KEY,
            started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL,
            source_mask INTEGER NOT NULL,
            created_ms INTEGER NOT NULL,
            CHECK(ended_ms > started_ms)
         );
         CREATE INDEX IF NOT EXISTS event_suppressions_time ON event_suppressions(started_ms,ended_ms);
         CREATE INDEX IF NOT EXISTS cloud_tasks_next ON cloud_tasks(provider,state,next_attempt_ms,manual,created_ms);
         DROP INDEX IF EXISTS cloud_tasks_logical;
         CREATE UNIQUE INDEX IF NOT EXISTS cloud_tasks_run ON cloud_tasks
            (provider,target_kind,target_id,target_version,processor_version,
             started_ms,ended_ms,coalesce(source,''));
         CREATE TABLE IF NOT EXISTS precise_segments (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            source TEXT NOT NULL,
            started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL,
            text TEXT NOT NULL,
            speaker_scope TEXT NOT NULL,
            speaker_label TEXT,
            CHECK(ended_ms > started_ms)
         );
         CREATE INDEX IF NOT EXISTS precise_segments_time ON precise_segments(started_ms);
         CREATE INDEX IF NOT EXISTS precise_segments_task_time
           ON precise_segments(task_id,started_ms,ended_ms);
         CREATE INDEX IF NOT EXISTS precise_segments_speaker
           ON precise_segments(speaker_scope,speaker_label);
         CREATE TABLE IF NOT EXISTS manual_ranges (
            id TEXT PRIMARY KEY,
            started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL,
            source_mask INTEGER NOT NULL CHECK(source_mask IN (1,2,3)),
            revision INTEGER NOT NULL,
            CHECK(ended_ms > started_ms)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS manual_ranges_scope
           ON manual_ranges(started_ms,ended_ms,source_mask);
         CREATE TABLE IF NOT EXISTS event_lineage (
           old_event_id TEXT NOT NULL,
           new_event_id TEXT NOT NULL,
           created_ms INTEGER NOT NULL,
           PRIMARY KEY(old_event_id,new_event_id)
         );",
    )
    .map_err(|error| error.to_string())?;
    crate::storage::migrate(
        db,
        "cloud_cooldown_v1",
        "ALTER TABLE cloud_queues ADD COLUMN cooldown_ms INTEGER NOT NULL DEFAULT 0;",
    )?;
    crate::storage::migrate(
        db,
        "cloud_account_block_v1",
        "ALTER TABLE cloud_queues ADD COLUMN blocked_reason TEXT;",
    )?;
    crate::storage::migrate(
        db,
        "cloud_retry_budget_v1",
        "ALTER TABLE cloud_tasks ADD COLUMN retry_failures INTEGER NOT NULL DEFAULT 0;",
    )?;
    crate::storage::migrate(
        db,
        "event_summary_v1",
        "ALTER TABLE named_events ADD COLUMN summary TEXT;",
    )?;
    crate::storage::migrate(
        db,
        "event_understanding_v1",
        "ALTER TABLE named_events ADD COLUMN understanding_json TEXT;",
    )?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS cloud_tasks_ready_priority
        ON cloud_tasks(provider,manual DESC,started_ms,created_ms)
        WHERE state IN ('pending','submitted');
        CREATE INDEX IF NOT EXISTS cloud_tasks_ready_age
        ON cloud_tasks(provider,started_ms,created_ms)
        WHERE state IN ('pending','submitted');",
    )
    .map_err(|e| e.to_string())?;
    asr::init(db)?;
    completion::init(db)?;
    crate::knowledge::init(db)
}

fn provider_name(provider: CloudProvider) -> &'static str {
    match provider {
        CloudProvider::Llm => "llm",
        CloudProvider::Asr => "asr",
    }
}

pub fn configured(db: &Connection) -> Result<bool, String> {
    db.query_row(
        "SELECT count(*) = 2 FROM cloud_queues q JOIN cloud_credential_refs c USING(provider)
         WHERE q.configured = 1",
        [],
        |row| row.get(0),
    )
    .map_err(|error| error.to_string())
}

fn wake_credential_tasks(db: &Connection, provider: Option<CloudProvider>) -> Result<(), String> {
    db.execute(
        "UPDATE cloud_tasks SET next_attempt_ms=0
         WHERE state IN ('pending','submitted') AND (?1 IS NULL OR provider=?1)
           AND (error_code GLOB 'keychain_read_*' OR error_code='credential_missing')",
        params![provider.map(provider_name)],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_configured_with_credential(
    db: &Connection,
    provider: CloudProvider,
    model: &str,
    reference: &str,
) -> Result<(), String> {
    db.execute(
        "UPDATE cloud_queues SET configured = 1, model = ?2,blocked_reason=NULL WHERE provider = ?1",
        params![provider_name(provider), model],
    )
    .map_err(|error| error.to_string())?;
    db.execute(
        "INSERT INTO cloud_credential_refs(provider,account) VALUES (?1,?2)
        ON CONFLICT(provider) DO UPDATE SET account=excluded.account",
        params![provider_name(provider), reference],
    )
    .map_err(|error| error.to_string())?;
    wake_credential_tasks(db, Some(provider))?;
    Ok(())
}

#[cfg(test)]
pub fn set_configured(db: &Connection, provider: CloudProvider, model: &str) -> Result<(), String> {
    set_configured_with_credential(db, provider, model, "test:credential")
}

pub fn credential_reference(
    db: &Connection,
    provider: CloudProvider,
) -> Result<Option<String>, String> {
    db.query_row(
        "SELECT c.account FROM cloud_queues q JOIN cloud_credential_refs c USING(provider)
        WHERE q.provider=?1 AND q.configured=1",
        params![provider_name(provider)],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn set_paused(db: &Connection, provider: CloudProvider, paused: bool) -> Result<(), String> {
    db.execute(
        "UPDATE cloud_queues SET paused = ?2,blocked_reason=CASE WHEN ?2=0 THEN NULL ELSE blocked_reason END WHERE provider = ?1",
        params![provider_name(provider), paused],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn model(db: &Connection, provider: CloudProvider) -> Result<Option<String>, String> {
    db.query_row(
        "SELECT model FROM cloud_queues WHERE provider = ?1 AND configured = 1",
        params![provider_name(provider)],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn queue_states(db: &Connection) -> Result<Vec<CloudQueueState>, String> {
    let mut result = Vec::new();
    for provider in [CloudProvider::Llm, CloudProvider::Asr] {
        let (configured, paused, blocked_reason): (bool, bool, Option<String>) = db
            .query_row(
                "SELECT q.configured=1 AND c.account IS NOT NULL,q.paused,q.blocked_reason FROM cloud_queues q
                 LEFT JOIN cloud_credential_refs c USING(provider) WHERE q.provider = ?1",
                params![provider_name(provider)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        let count = |state: &str| -> Result<u64, String> {
            db.query_row(
                "SELECT count(*) FROM cloud_tasks WHERE provider = ?1 AND state = ?2",
                params![provider_name(provider), state],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as u64)
            .map_err(|error| error.to_string())
        };
        result.push(CloudQueueState {
            provider,
            configured,
            paused,
            blocked_reason,
            pending: count("pending")?,
            processing: count("processing")? + count("submitted")?,
            failed: count("failed")?,
        });
    }
    Ok(result)
}

pub fn task_views(
    db: &Connection,
    provider: CloudProvider,
    limit: u16,
) -> Result<Vec<CloudTaskView>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id,target_kind,target_id,source,started_ms,ended_ms,
        CASE WHEN state='complete' AND provider='asr'
          AND NOT EXISTS(SELECT 1 FROM precise_segments p WHERE p.task_id=cloud_tasks.id)
          THEN 'no_transcript' ELSE state END,error_code,manual,
        (SELECT source_mask FROM event_reorganization_requests r WHERE target_kind='reorganization' AND r.id=cloud_tasks.target_id)
        FROM cloud_tasks WHERE provider=?1 AND coalesce(error_code,'')!='data_cleared'
        ORDER BY CASE
          WHEN state='failed' THEN 0
          WHEN state IN ('pending','processing','submitted') THEN 1
          ELSE 2 END, created_ms DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_map(
        params![provider_name(provider), limit.clamp(1, 200)],
        |row| {
            task_view_from_row(
                row,
                provider,
                row.get::<_, Option<i64>>(9)?.map(sources_for),
            )
        },
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

fn task_view_from_row(
    row: &rusqlite::Row<'_>,
    provider: CloudProvider,
    sources: Option<EventSources>,
) -> rusqlite::Result<CloudTaskView> {
    let id: String = row.get(0)?;
    let source: Option<String> = row.get(3)?;
    let source = match source.as_deref() {
        Some("microphone") => Some(AudioSource::Microphone),
        Some("system") => Some(AudioSource::System),
        None => None,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(CloudTaskView {
        id: Uuid::parse_str(&id).map_err(|_| rusqlite::Error::InvalidQuery)?,
        provider,
        target_kind: row.get(1)?,
        target_id: row.get(2)?,
        source,
        sources,
        started_ms: row.get(4)?,
        ended_ms: row.get(5)?,
        state: row.get(6)?,
        error_code: row.get(7)?,
        manual: row.get(8)?,
    })
}

#[derive(Clone)]
struct Evidence {
    id: String,
    start: i64,
    end: i64,
    text: Option<String>,
    source: String,
}

fn evidence(db: &Connection, from: i64, to: i64) -> Result<Vec<Evidence>, String> {
    let mut statement = db
        .prepare(
            "SELECT id,started_ms,ended_ms,text,source FROM coarse_segments
         WHERE ended_ms > ?1 AND started_ms < ?2
         UNION ALL
         SELECT b.id,b.started_ms,b.ended_ms,NULL,b.source FROM audio_blocks b
         WHERE b.speech_active = 1 AND b.retention_decision!='all_data'
           AND b.ended_ms > ?1 AND b.started_ms < ?2
           AND NOT EXISTS (SELECT 1 FROM coarse_segments c WHERE c.block_id=b.id)
         ORDER BY 2,3,1",
        )
        .map_err(|error| error.to_string())?;
    statement
        .query_map(params![from, to], |row| {
            Ok(Evidence {
                id: row.get(0)?,
                start: row.get(1)?,
                end: row.get(2)?,
                text: row.get(3)?,
                source: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub start: i64,
    pub end: i64,
    pub hash: String,
}

fn build_candidates_with_cuts(
    evidence: &[Evidence],
    cuts: &[(i64, i64, i64, i64)],
    existing_cut_positions: &[(usize, i64)],
) -> Vec<Candidate> {
    let mut groups: Vec<Vec<&Evidence>> = Vec::new();
    let mut persisted_end = i64::MIN;
    for (position, item) in evidence.iter().enumerate() {
        if item.end <= item.start {
            continue;
        }
        let new_group = groups.last().is_none_or(|group| {
            let prior_end = group.iter().map(|e| e.end).max().unwrap();
            let mask = group.iter().fold(source_bit(&item.source), |mask, e| {
                mask | source_bit(&e.source)
            });
            // Existing paid windows retain their membership when policy gets
            // shorter as well as longer. Only new evidence uses the new size.
            ((item.start - prior_end > CANDIDATE_POLICY.quiet_cut_ms
                || item.end - g_start(group) > CANDIDATE_POLICY.max_window_ms)
                && item.start >= persisted_end)
                // Published compute boundaries remain stable when the window
                // policy grows. In particular, do not absorb an old 30-minute
                // candidate into its predecessor and orphan its event links.
                // Late same-start evidence can precede the old first row;
                // it belongs in that window, not a second identical-start one.
                || (item.start > g_start(group)
                    && existing_cut_positions.binary_search_by_key(&position, |v| v.0).is_ok())
                || cuts.iter().any(|(a, b, cut_mask, committed)| {
                    *a <= item.start
                        && *b >= prior_end
                        && *a < *b
                        && *cut_mask & mask == mask
                        && item.start <= *committed
                })
        });
        if new_group {
            persisted_end = existing_cut_positions
                .iter()
                .filter(|(index, _)| evidence.get(*index).is_some_and(|e| e.start == item.start))
                .map(|(_, end)| *end)
                .max()
                .unwrap_or(i64::MIN);
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().push(item);
    }
    groups
        .into_iter()
        .filter_map(|group| {
            let first = group.first()?;
            let start = first.start;
            let id = first.id.clone();
            let end = group.iter().map(|e| e.end).max()?;
            let mut hasher = Sha256::new();
            hasher.update(CANDIDATE_POLICY.version.as_bytes());
            hasher.update(FIT_POLICY_VERSION.as_bytes());
            for item in group {
                hasher.update(item.id.as_bytes());
                hasher.update(item.start.to_le_bytes());
                hasher.update(item.end.to_le_bytes());
                hasher.update(item.source.as_bytes());
                if let Some(text) = &item.text {
                    hasher.update(text.as_bytes());
                }
            }
            let hash = format!("{:x}", hasher.finalize());
            Some(Candidate {
                id,
                start,
                end,
                hash,
            })
        })
        .collect()
}

fn candidate_cut_positions(
    evidence: &[Evidence],
    persisted: &[(String, i64, i64)],
) -> Vec<(usize, i64)> {
    let positions: std::collections::HashMap<&str, usize> = evidence
        .iter()
        .enumerate()
        .map(|(index, item)| (item.id.as_str(), index))
        .collect();
    let mut cuts: Vec<(usize, i64)> = persisted
        .iter()
        .map(|(id, start, end)| {
            // A candidate originally uses its first evidence ID. Two sources
            // can have equal starts and different ends, and the old size
            // limit can split between those rows. Time alone loses that cut.
            // Late coarse text can replace the original VAD/block evidence,
            // or move a reused candidate's start; then use the persisted time.
            let position = positions
                .get(id.as_str())
                .copied()
                .filter(|index| evidence[*index].start == *start)
                .unwrap_or_else(|| evidence.partition_point(|item| item.start < *start));
            (position, *end)
        })
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

fn source_bit(source: &str) -> i64 {
    if source == "microphone" { 1 } else { 2 }
}

fn g_start(group: &[&Evidence]) -> i64 {
    group[0].start
}

pub fn refresh_candidates(db: &mut Connection, now: i64, force: bool) -> Result<usize, String> {
    let tx = db.transaction().map_err(|error| error.to_string())?;
    let queued = refresh_candidates_in(&tx, now, force)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(queued)
}

fn fit_local_pending(db: &Connection, start: i64, end: i64, now: i64) -> Result<bool, String> {
    // Unanalysed blocks may not yet appear in evidence at all. Include the
    // remainder of this compute window, not only its last recognized word.
    let end = end.max(now.min(start.saturating_add(CANDIDATE_POLICY.max_window_ms)));
    db.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM ({FIT_LOCAL_PENDING_RANGES})
            WHERE started_ms<?2 AND ended_ms>?1)"
        ),
        params![start, end],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

pub fn refresh_candidates_in(tx: &Transaction<'_>, now: i64, force: bool) -> Result<usize, String> {
    // A persisted cursor covers shutdowns of arbitrary length. The live
    // lookback bounds repeated work; late local analysis moves it backward.
    let checkpoint: i64 = tx
        .query_row(
            "SELECT from_ms FROM candidate_scan_state WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut from = checkpoint.min((now - 2 * 60 * 60 * 1000).max(0));
    // Deferred candidates have no cloud task to wake up. Keep them reachable
    // even when local work finishes without new text or a cursor rewind.
    let deferred: Option<i64> = tx
        .query_row(
            "SELECT min(started_ms) FROM candidate_windows c
            WHERE c.state='pending' AND NOT EXISTS(
                SELECT 1 FROM cloud_tasks t WHERE t.target_kind='candidate'
                AND t.target_id=c.id AND t.target_version=c.input_hash)",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if let Some(start) = deferred {
        from = from.min(start);
    }
    let crossing: Option<i64> = tx
        .query_row(
            "SELECT min(started_ms) FROM candidate_windows
        WHERE started_ms < ?1 AND ended_ms > ?1",
            params![from],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if let Some(start) = crossing {
        from = start;
    }
    let all = evidence(tx, from, now + 1)?;
    let cuts = {
        let mut stmt = tx
            .prepare(
                "SELECT from_ms,to_ms,source_mask,committed_ms FROM clear_ranges
          WHERE mode='all_data' AND from_ms<?2 AND to_ms>?1 ORDER BY from_ms",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_map(params![from, now + 1], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    let persisted = {
        let mut stmt = tx
            .prepare("SELECT id,started_ms,ended_ms FROM candidate_windows WHERE started_ms>=?1 AND started_ms<?2 ORDER BY started_ms")
            .map_err(|e| e.to_string())?;
        stmt.query_map(params![from, now + 1], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    let existing_cuts = candidate_cut_positions(&all, &persisted);
    let candidates = build_candidates_with_cuts(&all, &cuts, &existing_cuts);
    let mut queued = 0;
    for mut candidate in candidates {
        if !force
            && now - candidate.end < CANDIDATE_POLICY.quiet_cut_ms
            && now - candidate.start < CANDIDATE_POLICY.max_window_ms
        {
            continue;
        }
        // Local rough analysis can arrive late. Reuse the substantially
        // overlapping candidate instead of inventing a second cloud identity.
        let existing_id: Option<String> = tx
            .query_row(
                "SELECT id FROM candidate_windows WHERE started_ms < ?2 AND ended_ms > ?1
             AND min(ended_ms,?2)-max(started_ms,?1) >= (?2-?1)/2
             AND NOT EXISTS(SELECT 1 FROM clear_ranges cut WHERE cut.mode='all_data'
               AND cut.from_ms<candidate_windows.ended_ms
               AND cut.to_ms>candidate_windows.started_ms)
             ORDER BY min(ended_ms,?2)-max(started_ms,?1) DESC LIMIT 1",
                params![candidate.start, candidate.end],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(id) = existing_id {
            candidate.id = id;
        }
        let current: Option<(String, String)> = tx
            .query_row(
                "SELECT input_hash,state FROM candidate_windows WHERE id = ?1",
                params![candidate.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let ready = force || !fit_local_pending(tx, candidate.start, candidate.end, now)?;
        if current
            .as_ref()
            .is_some_and(|(hash, _)| hash == &candidate.hash)
        {
            let queued: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM cloud_tasks WHERE target_kind='candidate'
                 AND target_id=?1 AND target_version=?2)",
                    params![candidate.id, candidate.hash],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if force && queued {
                tx.execute(
                    "UPDATE cloud_tasks SET manual=1 WHERE target_kind='candidate'
                    AND target_id=?1 AND target_version=?2 AND state='pending'",
                    params![candidate.id, candidate.hash],
                )
                .map_err(|e| e.to_string())?;
            }
            if queued || !ready || current.as_ref().unwrap().1 != "pending" {
                continue;
            }
        }
        tx.execute(
            "INSERT INTO candidate_windows(id,started_ms,ended_ms,input_hash,state,error_code)
            VALUES (?1,?2,?3,?4,'pending',NULL)
            ON CONFLICT(id) DO UPDATE SET started_ms=excluded.started_ms,ended_ms=excluded.ended_ms,
               input_hash=excluded.input_hash,state='pending',error_code=NULL",
            params![candidate.id, candidate.start, candidate.end, candidate.hash],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "UPDATE cloud_tasks SET state='superseded',updated_ms=?2
            WHERE target_kind='candidate' AND target_id=?1 AND target_version!=?3
              AND state IN ('pending','processing','submitted','failed')",
            params![candidate.id, now, candidate.hash],
        )
        .map_err(|error| error.to_string())?;
        // Keep the pending candidate as the chronological predecessor, but
        // coalesce local progress before creating a paid inference task.
        if !ready {
            continue;
        }
        insert_task(
            tx,
            "llm",
            "candidate",
            &candidate.id,
            &candidate.hash,
            candidate.start,
            candidate.end,
            None,
            force,
            now,
            FIT_POLICY_VERSION,
        )?;
        queued += 1;
    }
    tx.execute(
        "UPDATE candidate_scan_state SET from_ms=?1 WHERE id=1",
        params![(now - 2 * 60 * 60 * 1000).max(0)],
    )
    .map_err(|e| e.to_string())?;
    Ok(queued)
}

pub fn note_analysis(tx: &Transaction<'_>, block_id: &str) -> Result<(), String> {
    tx.execute(
        "UPDATE candidate_scan_state SET from_ms=min(from_ms,
        (SELECT max(0,started_ms-30*60*1000) FROM audio_blocks WHERE id=?1)) WHERE id=1",
        params![block_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn insert_task(
    tx: &Transaction<'_>,
    provider: &str,
    kind: &str,
    target: &str,
    version: &str,
    start: i64,
    end: i64,
    source: Option<&str>,
    manual: bool,
    now: i64,
    processor_version: &str,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO cloud_tasks
       (id,provider,target_kind,target_id,target_version,model_id,processor_version,
        started_ms,ended_ms,source,manual,state,created_ms,updated_ms)
       SELECT ?1,?2,?3,?4,?5,model,?11,?6,?7,?8,?9,'pending',?10,?10
       FROM cloud_queues WHERE provider=?2
       ON CONFLICT DO UPDATE SET
         state=CASE WHEN cloud_tasks.provider='asr' AND cloud_tasks.submit_attempts>0
           THEN 'submitted' ELSE 'pending' END,
         next_attempt_ms=excluded.created_ms,error_code=NULL,
         model_id=excluded.model_id,processor_version=excluded.processor_version,
         updated_ms=CASE WHEN cloud_tasks.provider='asr' AND cloud_tasks.submit_attempts>0
           THEN cloud_tasks.updated_ms ELSE excluded.created_ms END
       WHERE cloud_tasks.state='superseded'
          OR (cloud_tasks.state='failed' AND excluded.target_kind!='range')
          OR (cloud_tasks.provider='llm' AND cloud_tasks.state='complete')",
        params![
            Uuid::new_v4().to_string(),
            provider,
            kind,
            target,
            version,
            start,
            end,
            source,
            manual,
            now,
            processor_version
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_range(start: i64, end: i64) -> Result<(), String> {
    if start < 0 || end <= start || end - start > 24 * 60 * 60 * 1000 {
        Err("invalid time range".to_owned())
    } else {
        Ok(())
    }
}

fn validate_title(title: &str) -> Result<(), String> {
    if title.trim().is_empty() || title.len() > 240 || title.chars().any(char::is_control) {
        Err("invalid event title".to_owned())
    } else {
        Ok(())
    }
}

fn source_mask(sources: EventSources) -> i64 {
    match sources {
        EventSources::Microphone => 1,
        EventSources::System => 2,
        EventSources::Both => 3,
    }
}

fn sources_for(mask: i64) -> EventSources {
    match mask {
        1 => EventSources::Microphone,
        2 => EventSources::System,
        _ => EventSources::Both,
    }
}

pub(crate) fn asr_version(mask: i64, start: i64, end: i64) -> String {
    format!("{mask}:{start}:{end}")
}

fn ensure_nonoverlap(
    tx: &Transaction<'_>,
    start: i64,
    end: i64,
    mask: i64,
    excluding: Option<&str>,
) -> Result<(), String> {
    let conflict: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM named_events WHERE deleted_ms IS NULL
          AND started_ms < ?2 AND ended_ms > ?1 AND source_mask & ?3 != 0
          AND id != coalesce(?4,''))",
            params![start, end, mask, excluding],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if conflict {
        Err("event overlaps another named event".to_owned())
    } else {
        Ok(())
    }
}

// A conflicting proposal cannot replace existing content, but it must not
// roll back independent proposals from the same response.
fn fit_nonoverlap(
    tx: &Transaction<'_>,
    start: i64,
    end: i64,
    mask: i64,
    excluding: Option<&str>,
) -> Result<bool, String> {
    let conflict: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1
         FROM named_events WHERE deleted_ms IS NULL
          AND started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0
          AND id!=coalesce(?4,''))",
            params![start, end, mask, excluding],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(!conflict)
}

#[cfg(test)]
pub fn create_event(
    db: &mut Connection,
    start: i64,
    end: i64,
    title: &str,
    sources: EventSources,
    now: i64,
) -> Result<Uuid, String> {
    let tx = db.transaction().map_err(|error| error.to_string())?;
    let id = create_event_in(&tx, start, end, title, sources, now)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(id)
}

pub fn create_event_in(
    tx: &Transaction<'_>,
    start: i64,
    end: i64,
    title: &str,
    sources: EventSources,
    now: i64,
) -> Result<Uuid, String> {
    validate_range(start, end)?;
    validate_title(title)?;
    let id = Uuid::new_v4();
    let mask = source_mask(sources);
    ensure_nonoverlap(tx, start, end, mask, None)?;
    tx.execute(
        "INSERT INTO named_events (id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,deleted_ms,created_ms) VALUES (?1,?2,?3,?4,?5,1,'manual',1,1,1,NULL,?6)",
        params![id.to_string(), start, end, title.trim(), mask, now],
    )
    .map_err(|error| error.to_string())?;
    queue_asr(
        tx,
        "event",
        &id.to_string(),
        &asr_version(mask, start, end),
        start,
        end,
        mask,
        true,
        now,
    )?;
    Ok(id)
}

#[cfg(test)]
pub fn edit_event(
    db: &mut Connection,
    id: Uuid,
    expected: i64,
    start: i64,
    end: i64,
    title: &str,
    sources: Option<EventSources>,
    now: i64,
) -> Result<(), String> {
    let tx = db.transaction().map_err(|error| error.to_string())?;
    edit_event_in(&tx, id, expected, start, end, title, sources, now)?;
    tx.commit().map_err(|error| error.to_string())
}

pub fn edit_event_in(
    tx: &Transaction<'_>,
    id: Uuid,
    expected: i64,
    start: i64,
    end: i64,
    title: &str,
    sources: Option<EventSources>,
    now: i64,
) -> Result<(), String> {
    validate_range(start, end)?;
    validate_title(title)?;
    let prior: Option<(i64, i64, i64, i64, String)> = tx
        .query_row(
            "SELECT revision,started_ms,ended_ms,source_mask,title
        FROM named_events WHERE id=?1 AND deleted_ms IS NULL",
            params![id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((revision, old_start, old_end, old_mask, old_title)) = prior else {
        return Err("event not found".to_owned());
    };
    if revision != expected {
        return Err("event revision conflict".to_owned());
    }
    let mask = sources.map(source_mask).unwrap_or(old_mask);
    ensure_nonoverlap(tx, start, end, mask, Some(&id.to_string()))?;
    let audio_changed = old_start != start || old_end != end || old_mask != mask;
    let title_changed = old_title != title.trim();
    tx.execute("UPDATE named_events SET started_ms=?2,ended_ms=?3,title=?4,source_mask=?5,revision=revision+1,
        boundary_locked=CASE WHEN ?6 THEN 1 ELSE boundary_locked END,
        title_locked=CASE WHEN ?7 THEN 1 ELSE title_locked END,
        summary=CASE WHEN ?6 THEN NULL ELSE summary END,
        understanding_json=CASE WHEN ?6 THEN NULL ELSE understanding_json END,asr_required=1 WHERE id=?1",
        params![id.to_string(),start,end,title.trim(),mask,audio_changed,title_changed])
        .map_err(|error| error.to_string())?;
    if audio_changed {
        queue_asr(
            tx,
            "event",
            &id.to_string(),
            &asr_version(mask, start, end),
            start,
            end,
            mask,
            true,
            now,
        )?;
    }
    Ok(())
}

/// Explicitly start a new ASR run for the same virtual event. Old transcript
/// rows remain as correction/identity anchors, while only the new run is
/// projected into the current detail and queue.
pub fn retranscribe_event_in(
    tx: &Transaction<'_>,
    id: Uuid,
    expected_revision: i64,
    now: i64,
) -> Result<usize, String> {
    let (start, end, mask, _) = active_event(tx, id, expected_revision)?;
    let version = asr_version(mask, start, end);
    // An explicit rerun is a new paid request. Keep the previous task and
    // segments for review, but prevent an in-flight old result from publishing.
    crate::knowledge::pending_event_corrections(tx, id, now)?;
    tx.execute(
        "DELETE FROM event_asr_refs WHERE event_id=?1",
        [id.to_string()],
    )
    .map_err(|e| e.to_string())?;
    let run = format!("{ASR_ADAPTER_VERSION}:{}", Uuid::new_v4());
    let count = queue_asr_with_processor(
        tx,
        "event",
        &id.to_string(),
        &version,
        start,
        end,
        mask,
        true,
        now,
        &run,
    )?;
    if count == 0 {
        return Err("no retained audio in event".to_owned());
    }
    Ok(count)
}

fn active_event(
    tx: &Connection,
    id: Uuid,
    expected: i64,
) -> Result<(i64, i64, i64, String), String> {
    let row: Option<(i64, i64, i64, i64, String)> = tx
        .query_row(
            "SELECT revision,started_ms,ended_ms,source_mask,title FROM named_events
         WHERE id=?1 AND deleted_ms IS NULL",
            params![id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some((rev, start, end, mask, title)) if rev == expected => Ok((start, end, mask, title)),
        _ => Err("event revision conflict or event not found".to_owned()),
    }
}

fn suppress_old(
    tx: &Transaction<'_>,
    id: Uuid,
    start: i64,
    end: i64,
    mask: i64,
    now: i64,
) -> Result<(), String> {
    tx.execute(
        "UPDATE named_events SET deleted_ms=?2 WHERE id=?1 AND deleted_ms IS NULL",
        params![id.to_string(), now],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM event_candidate_links WHERE event_id=?1",
        params![id.to_string()],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO event_suppressions VALUES (?1,?2,?3,?4,?5)",
        params![Uuid::new_v4().to_string(), start, end, mask, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn insert_manual_tx(
    tx: &Transaction<'_>,
    start: i64,
    end: i64,
    mask: i64,
    title: &str,
    now: i64,
) -> Result<Uuid, String> {
    ensure_nonoverlap(tx, start, end, mask, None)?;
    let id = Uuid::new_v4();
    tx.execute(
        "INSERT INTO named_events (id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,deleted_ms,created_ms) VALUES (?1,?2,?3,?4,?5,1,'manual',1,1,1,NULL,?6)",
        params![id.to_string(), start, end, title, mask, now],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO event_asr_state VALUES(?1,NULL)",
        [id.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

#[cfg(test)]
pub fn delete_event(db: &mut Connection, id: Uuid, expected: i64, now: i64) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    delete_event_in(&tx, id, expected, now)?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn delete_event_in(
    tx: &Transaction<'_>,
    id: Uuid,
    expected: i64,
    now: i64,
) -> Result<(), String> {
    let (start, end, mask, _) = active_event(tx, id, expected)?;
    crate::knowledge::pending_event_corrections(tx, id, now)?;
    suppress_old(tx, id, start, end, mask, now)?;
    asr::cancel_orphans(tx, now)
}

#[cfg(test)]
pub fn split_event(
    db: &mut Connection,
    id: Uuid,
    expected: i64,
    at: i64,
    left_title: &str,
    right_title: &str,
    now: i64,
) -> Result<(Uuid, Uuid), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let ids = split_event_in(&tx, id, expected, at, left_title, right_title, now)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ids)
}

pub fn split_event_in(
    tx: &Transaction<'_>,
    id: Uuid,
    expected: i64,
    at: i64,
    left_title: &str,
    right_title: &str,
    now: i64,
) -> Result<(Uuid, Uuid), String> {
    validate_title(left_title)?;
    validate_title(right_title)?;
    let (start, end, mask, _) = active_event(tx, id, expected)?;
    if at <= start || at >= end {
        return Err("split point outside event".to_owned());
    }
    suppress_old(tx, id, start, end, mask, now)?;
    let left = insert_manual_tx(tx, start, at, mask, left_title.trim(), now)?;
    let right = insert_manual_tx(tx, at, end, mask, right_title.trim(), now)?;
    for next in [left, right] {
        tx.execute(
            "INSERT INTO event_lineage VALUES (?1,?2,?3)",
            params![id.to_string(), next.to_string(), now],
        )
        .map_err(|error| error.to_string())?;
    }
    for next in [left, right] {
        asr::inherit(tx, &id.to_string(), &next.to_string())?;
    }
    for (next, a, b) in [(left, start, at), (right, at, end)] {
        queue_asr(
            tx,
            "event",
            &next.to_string(),
            &asr_version(mask, a, b),
            a,
            b,
            mask,
            true,
            now,
        )?;
    }
    Ok((left, right))
}

#[cfg(test)]
pub fn merge_events(
    db: &mut Connection,
    left: Uuid,
    left_rev: i64,
    right: Uuid,
    right_rev: i64,
    title: &str,
    now: i64,
) -> Result<Uuid, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let id = merge_events_in(&tx, left, left_rev, right, right_rev, title, now)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn merge_events_in(
    tx: &Transaction<'_>,
    left: Uuid,
    left_rev: i64,
    right: Uuid,
    right_rev: i64,
    title: &str,
    now: i64,
) -> Result<Uuid, String> {
    if left == right {
        return Err("choose two different events".to_owned());
    }
    validate_title(title)?;
    let (a_start, a_end, a_mask, _) = active_event(tx, left, left_rev)?;
    let (b_start, b_end, b_mask, _) = active_event(tx, right, right_rev)?;
    let start = a_start.min(b_start);
    let end = a_end.max(b_end);
    let mask = a_mask | b_mask;
    validate_range(start, end)?;
    suppress_old(tx, left, a_start, a_end, a_mask, now)?;
    suppress_old(tx, right, b_start, b_end, b_mask, now)?;
    let merged = insert_manual_tx(tx, start, end, mask, title.trim(), now)?;
    for prior in [left, right] {
        tx.execute(
            "INSERT INTO event_lineage VALUES (?1,?2,?3)",
            params![prior.to_string(), merged.to_string(), now],
        )
        .map_err(|error| error.to_string())?;
    }
    for prior in [left, right] {
        asr::inherit(tx, &prior.to_string(), &merged.to_string())?;
    }
    queue_asr(
        tx,
        "event",
        &merged.to_string(),
        &asr_version(mask, start, end),
        start,
        end,
        mask,
        true,
        now,
    )?;
    Ok(merged)
}

fn suppressed(tx: &Transaction<'_>, start: i64, end: i64, mask: i64) -> Result<bool, String> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM event_suppressions
        WHERE started_ms < ?2 AND ended_ms > ?1 AND source_mask & ?3 != 0)",
        params![start, end, mask],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
pub fn transcribe_range(
    db: &mut Connection,
    start: i64,
    end: i64,
    sources: EventSources,
    now: i64,
) -> Result<Uuid, String> {
    let tx = db.transaction().map_err(|error| error.to_string())?;
    let (id, _) = transcribe_range_in(&tx, start, end, sources, now)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(id)
}

pub fn transcribe_range_in(
    tx: &Transaction<'_>,
    start: i64,
    end: i64,
    sources: EventSources,
    now: i64,
) -> Result<(Uuid, Vec<CloudTaskView>), String> {
    validate_range(start, end)?;
    let mask = source_mask(sources);
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM manual_ranges WHERE started_ms=?1 AND ended_ms=?2 AND source_mask=?3",
            params![start, end, mask],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let id = existing
        .map(|value| Uuid::parse_str(&value).map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or_else(Uuid::new_v4);
    tx.execute(
        "INSERT OR IGNORE INTO manual_ranges VALUES (?1,?2,?3,?4,1)",
        params![id.to_string(), start, end, mask],
    )
    .map_err(|e| e.to_string())?;
    // A repeat request is a read of the original manual run, including runs
    // created by an earlier adapter version. Changing the encoder must not
    // silently purchase another transcription or project two transcripts.
    let existing_tasks: i64 = tx
        .query_row(
            "SELECT count(*) FROM cloud_tasks WHERE provider='asr' AND target_kind='range'
             AND target_id=?1 AND target_version='1' AND state!='superseded'",
            params![id.to_string()],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let count = if existing_tasks > 0 {
        existing_tasks as usize
    } else {
        queue_asr(
            tx,
            "range",
            &id.to_string(),
            "1",
            start,
            end,
            mask,
            true,
            now,
        )?
    };
    if count == 0 {
        return Err("no retained audio in selected range".to_owned());
    }
    let mut statement = tx
        .prepare(
            "SELECT id,target_kind,target_id,source,started_ms,ended_ms,
             CASE WHEN state='complete' AND NOT EXISTS(
               SELECT 1 FROM precise_segments p WHERE p.task_id=cloud_tasks.id)
               THEN 'no_transcript' ELSE state END,error_code,manual
             FROM cloud_tasks WHERE provider='asr' AND target_kind='range'
               AND target_id=?1 AND target_version='1' AND state!='superseded'
               AND coalesce(error_code,'')!='data_cleared'
             ORDER BY started_ms,source,id",
        )
        .map_err(|error| error.to_string())?;
    let tasks = statement
        .query_map(params![id.to_string()], |row| {
            task_view_from_row(row, CloudProvider::Asr, None)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok((id, tasks))
}

pub(crate) fn queue_asr(
    tx: &Transaction<'_>,
    kind: &str,
    target: &str,
    version: &str,
    start: i64,
    end: i64,
    mask: i64,
    manual: bool,
    now: i64,
) -> Result<usize, String> {
    queue_asr_with_processor(
        tx,
        kind,
        target,
        version,
        start,
        end,
        mask,
        manual,
        now,
        ASR_ADAPTER_VERSION,
    )
}

#[allow(clippy::too_many_arguments)]
fn queue_asr_with_processor(
    tx: &Transaction<'_>,
    kind: &str,
    target: &str,
    version: &str,
    start: i64,
    end: i64,
    mask: i64,
    manual: bool,
    now: i64,
    processor_version: &str,
) -> Result<usize, String> {
    asr::schedule(
        tx,
        kind,
        target,
        version,
        start,
        end,
        mask,
        manual,
        now,
        processor_version,
    )
}

type ManualRangeRow = (String, i64, i64, i64, i64);

const VISIBLE_MANUAL_RANGES_SQL: &str =
    "SELECT id,started_ms,ended_ms,source_mask,revision FROM manual_ranges m
         WHERE (
           EXISTS(SELECT 1 FROM audio_blocks b WHERE b.media_path IS NOT NULL
             AND b.started_ms<m.ended_ms AND b.ended_ms>m.started_ms
             AND b.source IN (CASE WHEN m.source_mask&1!=0 THEN 'microphone' END,
               CASE WHEN m.source_mask&2!=0 THEN 'system' END))
           OR EXISTS(SELECT 1 FROM coarse_segments c WHERE c.started_ms<m.ended_ms
             AND c.ended_ms>m.started_ms
             AND ((c.source='microphone' AND m.source_mask&1!=0)
               OR (c.source='system' AND m.source_mask&2!=0)))
           OR EXISTS(SELECT 1 FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
             WHERE t.target_kind='range' AND t.target_id=m.id AND t.state='complete'
               AND t.target_version=cast(m.revision AS TEXT)
               AND p.started_ms<m.ended_ms AND p.ended_ms>m.started_ms
               AND ((p.source='microphone' AND m.source_mask&1!=0)
                 OR (p.source='system' AND m.source_mask&2!=0))))";

fn visible_manual_ranges(
    db: &Connection,
    start: i64,
    end: i64,
) -> Result<Vec<ManualRangeRow>, String> {
    let mut statement = db
        .prepare(&format!(
            "{VISIBLE_MANUAL_RANGES_SQL} AND started_ms<?2 AND ended_ms>?1 ORDER BY m.rowid"
        ))
        .map_err(|error| error.to_string())?;
    statement
        .query_map(params![start, end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// Shared availability projection for monthly and all-history date navigation.
/// Points keep their exact timestamp; intervals remain half-open.
fn calendar_material_sql() -> String {
    format!(
        "SELECT started_ms,ended_ms,0 AS point FROM audio_blocks
         WHERE source IN ('microphone','system') AND media_path IS NOT NULL
         UNION ALL SELECT started_ms,ended_ms,0 FROM coarse_segments
         UNION ALL SELECT started_ms,ended_ms,0 FROM named_events WHERE deleted_ms IS NULL
         UNION ALL SELECT anchor_ms,anchor_ms,1 FROM moments
         UNION ALL SELECT started_ms,ended_ms,0 FROM ({VISIBLE_MANUAL_RANGES_SQL})"
    )
}

fn valid_calendar_day(start: i64, end: i64) -> bool {
    matches!(end.checked_sub(start), Some(82_800_000..=90_000_000))
}

/// Return material anchors on either side of a local day in one database read,
/// without enumerating days or months. The GUI converts anchors to local days.
pub fn calendar_adjacent(
    db: &Connection,
    start: i64,
    end: i64,
    now: i64,
) -> Result<(Option<i64>, Option<i64>), String> {
    if !valid_calendar_day(start, end) {
        return Err("invalid calendar boundaries".into());
    }
    db.query_row(
        &format!(
            "WITH material AS ({}) SELECT
             min(CASE WHEN (point=1 AND started_ms>=?2) OR (point=0 AND ended_ms>?2)
                 THEN max(started_ms,?2) END),
             max(CASE WHEN started_ms<?1 THEN
                 CASE WHEN point=1 THEN started_ms ELSE min(ended_ms-1,?1-1,?3) END END)
             FROM material WHERE started_ms<=?3 AND (point=1 OR ended_ms>started_ms)",
            calendar_material_sql()
        ),
        params![start, end, now],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|error| error.to_string())
    .map(|(newer, older): (Option<i64>, Option<i64>)| (newer.filter(|at| *at <= now), older))
}

/// Calendar uses explicit local-midnight boundaries from the client, so DST and
/// timezone changes do not silently turn this into a UTC-day index.
pub fn calendar_days(db: &Connection, boundaries: &[i64], now: i64) -> Result<Vec<i64>, String> {
    if !(2..=32).contains(&boundaries.len())
        || boundaries
            .windows(2)
            .any(|pair| !valid_calendar_day(pair[0], pair[1]))
    {
        return Err("invalid calendar boundaries".into());
    }
    // Stream the month once. Per-day EXISTS used to repeat the same historical
    // scans up to 31 times, especially for empty days before the first recording.
    let start = boundaries[0];
    let end = *boundaries.last().unwrap();
    let mut present = [false; 31];
    let mut mark = |start: i64, end: i64| {
        for (index, pair) in boundaries.windows(2).enumerate() {
            if pair[0] <= now && start < pair[1] && end > pair[0] {
                present[index] = true;
            }
        }
    };
    let mut query = db
        .prepare(&format!(
            "SELECT started_ms,ended_ms,point FROM ({}) WHERE started_ms<?2
         AND started_ms<=?3 AND (point=1 OR ended_ms>started_ms)
         AND (ended_ms>?1 OR (point=1 AND started_ms>=?1))",
            calendar_material_sql()
        ))
        .map_err(|e| e.to_string())?;
    let mut rows = query
        .query(params![start, end, now])
        .map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let first: i64 = row.get(0).map_err(|e| e.to_string())?;
        let last: i64 = row.get(1).map_err(|e| e.to_string())?;
        let point: bool = row.get(2).map_err(|e| e.to_string())?;
        mark(first, if point { last.saturating_add(1) } else { last });
    }
    Ok(boundaries[..boundaries.len() - 1]
        .iter()
        .enumerate()
        .filter_map(|(index, day)| present[index].then_some(*day))
        .collect())
}

/// An open source session's confirmed tail must actually belong to the item.
/// Global recording alone cannot make a completed or historical card live.
pub fn mark_recording_items(
    db: &Connection,
    items: &mut [TimelineItem],
    active: &[AudioSource],
    now: i64,
) -> Result<(), String> {
    for source in active {
        let name = if *source == AudioSource::Microphone {
            "microphone"
        } else {
            "system"
        };
        let tail: Option<i64> = db.query_row("SELECT max(last_confirmed_ms) FROM recording_sessions WHERE state='open' AND source=?1", params![name], |row| row.get(0)).map_err(|e| e.to_string())?;
        let Some(tail) = tail.filter(|tail| *tail <= now && now.saturating_sub(*tail) <= 30_000)
        else {
            continue;
        };
        if let Some(item) = items.iter_mut().find(|item| {
            item.processing_state != "moment_only"
                && item.started_ms < tail
                && item.ended_ms >= tail
                && (item.sources == EventSources::Both
                    || (item.sources == EventSources::Microphone
                        && *source == AudioSource::Microphone)
                    || (item.sources == EventSources::System && *source == AudioSource::System))
        }) {
            item.recording_sources.push(*source);
        }
    }
    Ok(())
}

pub fn timeline(db: &Connection, start: i64, end: i64) -> Result<Vec<TimelineItem>, String> {
    validate_range(start, end)?;
    let mut statement = db
        .prepare(
            "SELECT id,started_ms,ended_ms,title,revision,source_mask,summary FROM named_events
        WHERE deleted_ms IS NULL AND started_ms < ?2 AND ended_ms > ?1 ORDER BY started_ms",
        )
        .map_err(|error| error.to_string())?;
    let events = statement
        .query_map(params![start, end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let manual_ranges = visible_manual_ranges(db, start, end)?;
    let mut output = Vec::new();
    let mut uncovered = Vec::new();
    for (source, bit) in [("microphone", 1), ("system", 2)] {
        let bounds: (Option<i64>, Option<i64>) = db
            .query_row(
                "SELECT min(started_ms),max(ended_ms) FROM (
              SELECT started_ms,ended_ms FROM audio_blocks
                WHERE source=?3 AND media_path IS NOT NULL AND started_ms < ?2 AND ended_ms > ?1
              UNION ALL SELECT started_ms,ended_ms FROM coarse_segments
                WHERE source=?3 AND started_ms < ?2 AND ended_ms > ?1)",
                params![start, end, source],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let mut first = bounds.0;
        let mut last = bounds.1;
        for (_, a, b, mask, _) in &manual_ranges {
            if mask & bit != 0 {
                first = Some(first.map_or(*a, |value| value.min(*a)));
                last = Some(last.map_or(*b, |value| value.max(*b)));
            }
        }
        let (Some(first), Some(last)) = (first, last) else {
            continue;
        };
        let first = first.max(start);
        let last = last.min(end);
        let mut cursor = first;
        for (_, a, b, _, _, mask, _) in &events {
            if mask & bit == 0 || *b <= cursor || *a >= last {
                continue;
            }
            let a = (*a).max(first);
            let b = (*b).min(last);
            if a > cursor {
                uncovered.push((cursor, a, bit));
            }
            cursor = cursor.max(b);
        }
        if cursor < last {
            uncovered.push((cursor, last, bit));
        }
    }
    let mut points: Vec<i64> = uncovered.iter().flat_map(|(a, b, _)| [*a, *b]).collect();
    points.sort_unstable();
    points.dedup();
    let mut merged: Vec<(i64, i64, i64)> = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if b <= a {
            continue;
        }
        let mask = uncovered
            .iter()
            .filter(|(s, e, _)| *s < b && *e > a)
            .fold(0, |m, (_, _, bit)| m | bit);
        if mask == 0 {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.1 == a && last.2 == mask {
                last.1 = b;
                continue;
            }
        }
        merged.push((a, b, mask));
    }
    let mut projected: Vec<(i64, i64, i64, Option<String>)> = Vec::new();
    for (a, b, mask) in merged {
        let mut cuts = vec![a, b];
        for (_, range_start, range_end, range_mask, _) in &manual_ranges {
            if range_mask & mask != 0 && *range_start < b && *range_end > a {
                cuts.push((*range_start).clamp(a, b));
                cuts.push((*range_end).clamp(a, b));
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (piece_start, piece_end) = (pair[0], pair[1]);
            let mut groups: std::collections::BTreeMap<Option<String>, i64> =
                std::collections::BTreeMap::new();
            for bit in [1, 2] {
                if mask & bit == 0 {
                    continue;
                }
                let manual_id = manual_ranges
                    .iter()
                    .rev()
                    .find(|(_, from, to, sources, _)| {
                        sources & bit != 0 && *from <= piece_start && *to >= piece_end
                    })
                    .map(|range| range.0.clone());
                *groups.entry(manual_id).or_default() |= bit;
            }
            for (manual_id, source_mask) in groups {
                projected.push((piece_start, piece_end, source_mask, manual_id));
            }
        }
    }
    projected.sort_by(|a, b| (&a.3, a.2, a.0).cmp(&(&b.3, b.2, b.0)));
    let mut coalesced: Vec<(i64, i64, i64, Option<String>)> = Vec::new();
    for span in projected {
        if let Some(last) = coalesced.last_mut() {
            if last.1 == span.0 && last.2 == span.2 && last.3 == span.3 {
                last.1 = span.1;
                continue;
            }
        }
        coalesced.push(span);
    }
    for (a, b, mask, manual_id) in coalesced {
        let mut item = unorganized(db, a, b, mask)?;
        if let Some(id) = manual_id {
            let revision = manual_ranges
                .iter()
                .find(|range| range.0 == id)
                .map(|range| range.4)
                .ok_or("manual range missing")?;
            item.manual_range_id = Some(Uuid::parse_str(&id).map_err(|e| e.to_string())?);
            item.processing_state = manual_range_state(db, &id, revision)?;
            item.transcription =
                transcription_progress(db, "range", &id, &revision.to_string(), false)?;
            item.manual_asr_state = None;
            item.manual_asr_started_ms = None;
            item.manual_asr_ended_ms = None;
            item.manual_asr_source = None;
        }
        if item.processing_state != "moment_only" {
            output.push(item);
        }
    }
    for (id, a, b, title, revision, mask, summary) in events {
        output.push(TimelineItem {
            completed_ms: completion::completed_at(db, &id)?,
            completion_state: completion::task_state(db, &id)?,
            id: Some(Uuid::parse_str(&id).map_err(|e| e.to_string())?),
            manual_range_id: None,
            started_ms: a.max(start),
            ended_ms: b.min(end),
            title: Some(title),
            summary,
            sources: sources_for(mask),
            revision: Some(revision),
            local_analysis_failures: asr::local_analysis_failures(
                db,
                a.max(start),
                b.min(end),
                mask,
            )?,
            fit_state: Some(event_fit_state(db, &id)?),
            audio_coverage: audio_coverage(db, a, b, mask)?,
            processing_state: event_state(db, &id, &asr_version(mask, a, b))?,
            transcription: transcription_progress(
                db,
                "event",
                &id,
                &asr_version(mask, a, b),
                false,
            )?,
            manual_asr_state: None,
            manual_asr_started_ms: None,
            manual_asr_ended_ms: None,
            manual_asr_source: None,
            recording_sources: Vec::new(),
            preview: timeline_preview(
                db,
                a.max(start),
                b.min(end),
                mask,
                Some((&id, &asr_version(mask, a, b))),
                None,
            )?,
            coarse_segments: Vec::new(),
            precise_segments: Vec::new(),
        });
    }
    // A comment, mark or screenshot may be the only retained material in an
    // otherwise quiet period. Keep its real timestamp visible on the same
    // timeline without inventing recorded audio for that interval.
    let mut statement = db.prepare(
        "SELECT DISTINCT anchor_ms FROM moments WHERE anchor_ms>=?1 AND anchor_ms<?2 ORDER BY anchor_ms"
    ).map_err(|error| error.to_string())?;
    let anchors = statement
        .query_map(params![start, end], |row| row.get::<_, i64>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut moment_gaps: std::collections::BTreeMap<(i64, i64), (i64, i64)> =
        std::collections::BTreeMap::new();
    for anchor in anchors {
        if output
            .iter()
            .any(|item| item.started_ms <= anchor && item.ended_ms > anchor)
        {
            continue;
        }
        let left = output
            .iter()
            .filter(|item| item.ended_ms <= anchor)
            .map(|item| item.ended_ms)
            .max()
            .unwrap_or(start);
        let right = output
            .iter()
            .filter(|item| item.started_ms > anchor)
            .map(|item| item.started_ms)
            .min()
            .unwrap_or(end);
        let group = moment_gaps.entry((left, right)).or_insert((anchor, anchor));
        group.0 = group.0.min(anchor);
        group.1 = group.1.max(anchor);
    }
    for (_, (first, last)) in moment_gaps {
        // A mark can precede the next sealed audio block. Keep that point in
        // its actual material-free gap; it must not widen or merge audio.
        output.push(unorganized(db, first, (last + 1).min(end), 3)?);
    }
    for item in output.iter_mut().filter(|item| item.id.is_none()) {
        let mask = match item.sources {
            EventSources::Microphone => 1,
            EventSources::System => 2,
            EventSources::Both => 3,
        };
        item.preview = timeline_preview(
            db,
            item.started_ms,
            item.ended_ms,
            mask,
            None,
            item.manual_range_id
                .as_ref()
                .map(Uuid::to_string)
                .as_deref(),
        )?;
    }
    output.sort_by(|a, b| b.started_ms.cmp(&a.started_ms));
    Ok(output)
}

// The list gets one bounded excerpt, never a full transcript or a second IPC per card.
// Reuse detail readers so current task versions and manual text corrections agree.
fn timeline_preview(
    db: &Connection,
    start: i64,
    end: i64,
    mask: i64,
    event: Option<(&str, &str)>,
    manual_id: Option<&str>,
) -> Result<Option<TimelinePreview>, String> {
    let mut offset = 0;
    loop {
        let precise = if let Some((id, version)) = event {
            precise_for(db, "event", id, version, start, end, 1, offset)?
        } else {
            range_precise_for(db, start, end, mask, 1, offset, manual_id)?
        };
        let Some(segment) = precise.into_iter().next() else {
            break;
        };
        if !segment.text.trim().is_empty() {
            return Ok(Some(bounded_preview(&segment.text, segment.source, true)));
        }
        offset += 1;
    }
    Ok(coarse_for(db, start, end, mask, 1, 0)?
        .into_iter()
        .next()
        .map(|segment| bounded_preview(&segment.text, segment.source, false)))
}

fn bounded_preview(text: &str, source: AudioSource, is_precise: bool) -> TimelinePreview {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let mut text: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    TimelinePreview {
        text,
        source,
        is_precise,
    }
}

pub fn timeline_gaps(
    db: &Connection,
    start: i64,
    end: i64,
    now: i64,
) -> Result<Vec<TimeGap>, String> {
    validate_range(start, end)?;
    let mut statement = db
        .prepare(
            "SELECT source,max(started_ms,?1),min(coalesce(ended_ms,?3),?2),reason
         FROM media_gaps WHERE started_ms<?2 AND coalesce(ended_ms,?3)>?1
         ORDER BY started_ms,source",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![start, end, now], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut gaps: Vec<TimeGap> = Vec::new();
    for row in rows {
        let (source, a, b, reason) = row.map_err(|error| error.to_string())?;
        if b - a < 1_000 {
            continue;
        }
        let source = match source.as_str() {
            "microphone" => AudioSource::Microphone,
            "system" => AudioSource::System,
            _ => return Err("invalid gap source".into()),
        };
        if let Some(last) = gaps.last_mut() {
            if last.source == source && last.reason == reason && a <= last.ended_ms + 1_000 {
                last.ended_ms = last.ended_ms.max(b);
                continue;
            }
        }
        gaps.push(TimeGap {
            source,
            started_ms: a,
            ended_ms: b,
            reason,
        });
    }
    Ok(gaps)
}

pub(crate) const RANGE_AUDIO_EXISTS: &str =
    "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path IS NOT NULL
		 AND started_ms<?2 AND ended_ms>?1 AND
		 source IN (CASE WHEN ?3&1!=0 THEN 'microphone' END, CASE WHEN ?3&2!=0 THEN 'system' END))";

fn unorganized(db: &Connection, start: i64, end: i64, mask: i64) -> Result<TimelineItem, String> {
    let has_audio: bool = db
        .query_row(RANGE_AUDIO_EXISTS, params![start, end, mask], |row| {
            row.get(0)
        })
        .map_err(|error| error.to_string())?;
    let has_sound: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE speech_active=1 AND retention_decision!='all_data'
         AND started_ms<?2 AND ended_ms>?1 AND
         source IN (CASE WHEN ?3&1!=0 THEN 'microphone' END, CASE WHEN ?3&2!=0 THEN 'system' END)
         UNION ALL SELECT 1 FROM coarse_segments WHERE started_ms<?2 AND ended_ms>?1 AND
         source IN (CASE WHEN ?3&1!=0 THEN 'microphone' END, CASE WHEN ?3&2!=0 THEN 'system' END))",
        params![start,end,mask], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    let status: Option<(String, Option<String>)> = db
        .query_row(
            "SELECT state,error_code FROM candidate_windows
        WHERE started_ms < ?2 AND ended_ms > ?1
        ORDER BY CASE WHEN state='failed' OR (state='pending' AND error_code IS NOT NULL)
          THEN 0 ELSE 1 END, ended_ms DESC LIMIT 1",
            params![start, end],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let manual_asr = db
        .query_row(
            "SELECT CASE WHEN state='complete'
          AND NOT EXISTS(SELECT 1 FROM precise_segments p WHERE p.task_id=cloud_tasks.id)
          THEN 'no_transcript' ELSE state END,started_ms,ended_ms,source FROM cloud_tasks
        WHERE target_kind='range' AND started_ms < ?2 AND ended_ms > ?1
          AND source IN (CASE WHEN ?3&1!=0 THEN 'microphone' END, CASE WHEN ?3&2!=0 THEN 'system' END)
          AND state!='superseded' ORDER BY created_ms DESC LIMIT 1",
            params![start, end, mask],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(TimelineItem {
        completed_ms: None,
        completion_state: None,
        id: None,
        manual_range_id: None,
        started_ms: start,
        ended_ms: end,
        title: None,
        summary: None,
        sources: sources_for(mask),
        revision: None,
        local_analysis_failures: asr::local_analysis_failures(db, start, end, mask)?,
        fit_state: None,
        transcription: None,
        audio_coverage: audio_coverage(db, start, end, mask)?,
        processing_state: if !has_sound {
            if has_audio {
                "raw_audio"
            } else {
                "moment_only"
            }
        } else {
            status.as_ref().map_or("pending_fit", |(state, error)| {
                candidate_status(state, error.as_deref())
            })
        }
        .to_owned(),
        manual_asr_state: manual_asr.as_ref().map(|task| task.0.clone()),
        manual_asr_started_ms: manual_asr.as_ref().map(|task| task.1),
        manual_asr_ended_ms: manual_asr.as_ref().map(|task| task.2),
        manual_asr_source: manual_asr.as_ref().map(|task| {
            if task.3 == "system" {
                AudioSource::System
            } else {
                AudioSource::Microphone
            }
        }),
        recording_sources: Vec::new(),
        preview: None,
        coarse_segments: Vec::new(),
        precise_segments: Vec::new(),
    })
}

fn candidate_status(state: &str, error: Option<&str>) -> &'static str {
    match state {
        "failed" => "fit_failed",
        "processing" => "fitting",
        "complete" => "unorganized",
        _ if error.is_some_and(credential_error) => "fit_credentials_unavailable",
        _ if error.is_some() => "fit_retrying",
        _ => "pending_fit",
    }
}

fn credential_error(error: &str) -> bool {
    error.starts_with("keychain_read_") || error == "credential_missing"
}

fn event_fit_state(db: &Connection, id: &str) -> Result<String, String> {
    let states: Vec<String> = {
        let mut stmt = db
            .prepare(
                "SELECT DISTINCT c.state,c.error_code FROM event_candidate_links l
            JOIN candidate_windows c ON c.id=l.candidate_id WHERE l.event_id=?1",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_map(params![id], |row| {
            let state: String = row.get(0)?;
            let error: Option<String> = row.get(1)?;
            Ok(candidate_status(&state, error.as_deref()).to_owned())
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    if states.is_empty() {
        return Ok("manual".to_owned());
    }
    for state in [
        "fit_failed",
        "fit_credentials_unavailable",
        "fit_retrying",
        "fitting",
        "pending_fit",
    ] {
        if states.iter().any(|s| s == state) {
            return Ok(state.to_owned());
        }
    }
    Ok("fitted".to_owned())
}

fn page(limit: u16, offset: u32) -> Result<(i64, i64), String> {
    if limit == 0 || limit > 200 || offset > 1_000_000 {
        return Err("invalid transcript page".to_owned());
    }
    Ok((i64::from(limit), i64::from(offset)))
}

pub fn event_detail(
    db: &Connection,
    id: Uuid,
    offset: u32,
    limit: u16,
) -> Result<TimelineItem, String> {
    let (limit, offset) = page(limit, offset)?;
    let row: Option<(i64, i64, String, i64, i64, Option<String>)> = db
        .query_row(
            "SELECT started_ms,ended_ms,title,revision,source_mask,summary FROM named_events
         WHERE id=?1 AND deleted_ms IS NULL",
            params![id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((start, end, title, revision, mask, summary)) = row else {
        return Err("event not found".to_owned());
    };
    let version = asr_version(mask, start, end);
    let mut item = TimelineItem {
        completed_ms: completion::completed_at(db, &id.to_string())?,
        completion_state: completion::task_state(db, &id.to_string())?,
        id: Some(id),
        manual_range_id: None,
        started_ms: start,
        ended_ms: end,
        title: Some(title),
        summary,
        sources: sources_for(mask),
        revision: Some(revision),
        local_analysis_failures: asr::local_analysis_failures(db, start, end, mask)?,
        fit_state: Some(event_fit_state(db, &id.to_string())?),
        audio_coverage: audio_coverage(db, start, end, mask)?,
        processing_state: event_state(db, &id.to_string(), &version)?,
        transcription: transcription_progress(db, "event", &id.to_string(), &version, true)?,
        manual_asr_state: None,
        manual_asr_started_ms: None,
        manual_asr_ended_ms: None,
        manual_asr_source: None,
        recording_sources: Vec::new(),
        preview: None,
        coarse_segments: coarse_for(db, start, end, mask, limit, offset)?,
        precise_segments: precise_for(
            db,
            "event",
            &id.to_string(),
            &version,
            start,
            end,
            limit,
            offset,
        )?,
    };
    fill_precise_revisions(db, &mut item.precise_segments)?;
    Ok(item)
}

pub fn range_detail(
    db: &Connection,
    start: i64,
    end: i64,
    sources: EventSources,
    offset: u32,
    limit: u16,
) -> Result<TimelineItem, String> {
    validate_range(start, end)?;
    let (limit, offset) = page(limit, offset)?;
    let mask = source_mask(sources);
    let mut item = unorganized(db, start, end, mask)?;
    let manual_range = visible_manual_ranges(db, start, end)?
        .into_iter()
        .rev()
        .find(|(_, from, to, sources, _)| *from <= start && *to >= end && sources & mask == mask)
        .map(|(id, _, _, _, revision)| (id, revision));
    if let Some((id, revision)) = &manual_range {
        item.manual_range_id = Some(Uuid::parse_str(id).map_err(|e| e.to_string())?);
        item.processing_state = manual_range_state(db, id, *revision)?;
        item.transcription = transcription_progress(db, "range", id, &revision.to_string(), true)?;
        item.manual_asr_state = None;
        item.manual_asr_started_ms = None;
        item.manual_asr_ended_ms = None;
        item.manual_asr_source = None;
    }
    item.coarse_segments = coarse_for(db, start, end, mask, limit, offset)?;
    item.precise_segments = range_precise_for(
        db,
        start,
        end,
        mask,
        limit,
        offset,
        manual_range.as_ref().map(|range| range.0.as_str()),
    )?;
    fill_precise_revisions(db, &mut item.precise_segments)?;
    Ok(item)
}

fn fill_precise_revisions(db: &Connection, segments: &mut [PreciseSegment]) -> Result<(), String> {
    crate::knowledge::fill_revisions(db, segments)
}

fn coarse_for(
    db: &Connection,
    start: i64,
    end: i64,
    mask: i64,
    limit: i64,
    offset: i64,
) -> Result<Vec<CoarseSegment>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id,source,started_ms,ended_ms,text FROM coarse_segments
        WHERE started_ms < ?2 AND ended_ms > ?1 AND trim(text)!=''
          AND source IN (CASE WHEN ?3&1!=0 THEN 'microphone' END, CASE WHEN ?3&2!=0 THEN 'system' END)
        ORDER BY started_ms,id LIMIT ?4 OFFSET ?5",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_map(params![start, end, mask, limit, offset], |row| {
        let source: String = row.get(1)?;
        Ok(CoarseSegment {
            id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
            source: if source == "microphone" {
                AudioSource::Microphone
            } else {
                AudioSource::System
            },
            started_ms: row.get(2)?,
            ended_ms: row.get(3)?,
            text: row.get(4)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

fn transcription_progress(
    db: &Connection,
    kind: &str,
    id: &str,
    version: &str,
    detail: bool,
) -> Result<Option<TranscriptionProgress>, String> {
    let mut stmt = db
        .prepare(
            "SELECT t.state,t.source,t.started_ms,t.ended_ms,
        EXISTS(SELECT 1 FROM precise_segments p WHERE p.task_id=t.id
          AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms),
        EXISTS(SELECT 1 FROM precise_segments p WHERE p.task_id=t.id
          AND p.started_ms<t.ended_ms AND p.ended_ms>t.started_ms
          AND (p.started_ms<t.started_ms OR p.ended_ms>t.ended_ms))
        FROM transcript_tasks t WHERE t.provider='asr' AND t.target_kind=?1
          AND t.target_id=?2 AND t.target_version=?3 AND t.state!='superseded'
        ORDER BY t.started_ms,t.ended_ms,t.source",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![kind, id, version], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, bool>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut progress = TranscriptionProgress::default();
    for row in rows {
        let (state, source, start, end, has_text, crossing) = row.map_err(|e| e.to_string())?;
        progress.total += 1;
        if state != "complete" {
            continue;
        }
        progress.complete += 1;
        progress.boundary_clipped |= crossing;
        if !has_text && !crossing {
            progress.empty += 1;
            if detail {
                progress.empty_ranges.push(UnrecognizedAudio {
                    source: if source == "system" {
                        AudioSource::System
                    } else {
                        AudioSource::Microphone
                    },
                    started_ms: start,
                    ended_ms: end,
                });
            }
        }
    }
    Ok((progress.total > 0).then_some(progress))
}

/// Exact per-source union; unlike timeline decoration this never rounds or bridges gaps.
fn audio_coverage(
    db: &Connection,
    start: i64,
    end: i64,
    mask: i64,
) -> Result<Vec<loppi_protocol::AudioCoverage>, String> {
    let mut output = Vec::new();
    for (source, bit, name) in [
        (loppi_protocol::AudioSource::Microphone, 1, "microphone"),
        (loppi_protocol::AudioSource::System, 2, "system"),
    ] {
        if mask & bit == 0 {
            continue;
        }
        let mut stmt = db.prepare("SELECT max(a,?1),min(b,?2),reason FROM (
            SELECT started_ms a,coalesce(ended_ms,?2) b,reason FROM media_gaps WHERE source=?3
            UNION ALL SELECT started_ms,ended_ms,'audio_unavailable' FROM audio_blocks WHERE source=?3 AND speech_active=1 AND media_path IS NULL
            UNION ALL SELECT from_ms,min(to_ms,committed_ms),CASE mode WHEN 'all_data' THEN 'cleared_all_data' ELSE 'cleared_audio' END FROM clear_ranges WHERE source_mask & ?4 != 0
        ) WHERE a<?2 AND b>?1 ORDER BY a,b").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map(params![start, end, name, bit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut duration_ms = 0;
        let mut interval_count = 0;
        let mut until = i64::MIN;
        let mut reasons = std::collections::BTreeSet::new();
        for row in rows {
            let (a, b, reason) = row.map_err(|e| e.to_string())?;
            if b <= a {
                continue;
            }
            if a > until {
                interval_count += 1;
            }
            duration_ms += (b - a.max(until)).max(0);
            until = until.max(b);
            reasons.insert(reason);
        }
        if duration_ms > 0 {
            output.push(loppi_protocol::AudioCoverage {
                source,
                duration_ms,
                interval_count,
                reasons: reasons.into_iter().collect(),
            });
        }
    }
    Ok(output)
}

fn event_state(db: &Connection, id: &str, version: &str) -> Result<String, String> {
    let waiting:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM event_asr_state WHERE event_id=?1 AND coalesce(planned_version,'')!=?2)",params![id,version],|r|r.get(0)).map_err(|e|e.to_string())?;
    if waiting && completion::completed_at(db, id)?.is_some() {
        return Ok("pending_asr".into());
    }
    if waiting {
        let (start, end, mask) = db
            .query_row(
                "SELECT started_ms,ended_ms,source_mask FROM named_events WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        return Ok(asr::settling_state(db, id, start, end, mask)?
            .unwrap_or("settling_asr")
            .into());
    }

    let mut stmt = db
        .prepare(
            "SELECT state,count(*) FROM transcript_tasks WHERE provider='asr'
        AND target_kind='event' AND target_id=?1 AND target_version=?2
        AND state!='superseded' GROUP BY state",
        )
        .map_err(|e| e.to_string())?;
    let states = stmt
        .query_map(params![id, version], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if states.is_empty() {
        return Ok("audio_unavailable".to_owned());
    }
    if states.iter().any(|(s, _)| s == "failed") {
        let progress = transcription_progress(db, "event", id, version, false)?.unwrap();
        return Ok(if progress.complete > progress.empty {
            "partial_asr_failed"
        } else {
            "asr_failed"
        }
        .to_owned());
    }
    if states.iter().all(|(s, _)| s == "complete") {
        let progress = transcription_progress(db, "event", id, version, false)?.unwrap();
        if progress.boundary_clipped {
            return Ok("partial_transcript".into());
        }
        if progress.empty > 0 && progress.empty < progress.total {
            return Ok("partial_transcript".to_owned());
        }
        return Ok(if progress.empty < progress.total {
            "transcribed"
        } else {
            "no_transcript"
        }
        .to_owned());
    }
    if states
        .iter()
        .any(|(s, _)| s == "processing" || s == "submitted")
    {
        return Ok("transcribing".to_owned());
    }
    Ok("pending_asr".to_owned())
}

fn manual_range_state(db: &Connection, id: &str, revision: i64) -> Result<String, String> {
    let mut stmt = db
        .prepare(
            "SELECT state FROM cloud_tasks WHERE provider='asr' AND target_kind='range'
             AND target_id=?1 AND target_version=?2 AND state!='superseded'",
        )
        .map_err(|e| e.to_string())?;
    let states = stmt
        .query_map(params![id, revision.to_string()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if states.is_empty() {
        return Ok("audio_unavailable".to_owned());
    }
    if states.iter().any(|state| state == "failed") {
        let progress =
            transcription_progress(db, "range", id, &revision.to_string(), false)?.unwrap();
        return Ok(if progress.complete > progress.empty {
            "partial_asr_failed"
        } else {
            "asr_failed"
        }
        .to_owned());
    }
    if states.iter().all(|state| state == "complete") {
        let progress =
            transcription_progress(db, "range", id, &revision.to_string(), false)?.unwrap();
        return Ok(if progress.boundary_clipped
            || (progress.empty > 0 && progress.empty < progress.total)
        {
            "partial_transcript"
        } else if progress.empty < progress.total {
            "transcribed"
        } else {
            "no_transcript"
        }
        .into());
    }
    if states
        .iter()
        .any(|state| state == "processing" || state == "submitted")
    {
        return Ok("transcribing".to_owned());
    }
    Ok("pending_asr".to_owned())
}

fn precise_for(
    db: &Connection,
    kind: &str,
    id: &str,
    version: &str,
    start: i64,
    end: i64,
    limit: i64,
    offset: i64,
) -> Result<Vec<PreciseSegment>, String> {
    let mut stmt = db
        .prepare(
            "SELECT p.id,p.source,p.started_ms,p.ended_ms,p.text,
             identity.speaker,p.speaker_label,
             (SELECT c.text FROM text_corrections c WHERE c.current_segment_id=p.id
              AND c.state='active' ORDER BY c.updated_ms DESC LIMIT 1),
             identity.person_id,identity.person_name,identity.person_origin
        FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
        JOIN precise_identity_projection identity ON identity.segment_id=p.id
        WHERE t.target_kind=?1 AND t.target_id=?2 AND t.target_version=?3 AND t.state='complete'
          AND p.started_ms < ?5 AND p.ended_ms > ?4
          ORDER BY p.started_ms,p.id LIMIT ?6 OFFSET ?7",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_map(
        params![kind, id, version, start, end, limit, offset],
        |row| {
            let source: String = row.get(1)?;
            let original: String = row.get(4)?;
            let correction: Option<String> = row.get(7)?;
            Ok(PreciseSegment {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                source: if source == "microphone" {
                    AudioSource::Microphone
                } else {
                    AudioSource::System
                },
                started_ms: row.get(2)?,
                ended_ms: row.get(3)?,
                text: correction.clone().unwrap_or_else(|| original.clone()),
                speaker: row.get(5)?,
                original_text: correction.as_ref().map(|_| original),
                corrected: correction.is_some(),
                person_id: row
                    .get::<_, Option<String>>(8)?
                    .map(|id| Uuid::parse_str(&id).unwrap()),
                person_name: row.get(9)?,
                person_origin: row.get(10)?,
                revision: 0,
                speaker_revision: 0,
            })
        },
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

fn range_precise_for(
    db: &Connection,
    start: i64,
    end: i64,
    mask: i64,
    limit: i64,
    offset: i64,
    manual_id: Option<&str>,
) -> Result<Vec<PreciseSegment>, String> {
    let mut stmt=db.prepare("SELECT p.id,p.source,p.started_ms,p.ended_ms,p.text,
             identity.speaker,p.speaker_label,
             (SELECT c.text FROM text_corrections c WHERE c.current_segment_id=p.id
              AND c.state='active' ORDER BY c.updated_ms DESC LIMIT 1),
             identity.person_id,identity.person_name,identity.person_origin
        FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
        JOIN precise_identity_projection identity ON identity.segment_id=p.id
        JOIN manual_ranges m ON m.id=t.target_id
        WHERE t.target_kind='range' AND t.state='complete' AND t.target_version=cast(m.revision AS TEXT)
          AND (?6 IS NULL OR m.id=?6)
          AND p.started_ms < ?2 AND p.ended_ms > ?1
          AND ((p.source='microphone' AND ?3&1!=0) OR (p.source='system' AND ?3&2!=0))
          ORDER BY p.started_ms,p.id LIMIT ?4 OFFSET ?5")
        .map_err(|e|e.to_string())?;
    stmt.query_map(params![start, end, mask, limit, offset, manual_id], |row| {
        let source: String = row.get(1)?;
        let original: String = row.get(4)?;
        let correction: Option<String> = row.get(7)?;
        Ok(PreciseSegment {
            id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
            source: if source == "microphone" {
                AudioSource::Microphone
            } else {
                AudioSource::System
            },
            started_ms: row.get(2)?,
            ended_ms: row.get(3)?,
            text: correction.clone().unwrap_or_else(|| original.clone()),
            speaker: row.get(5)?,
            original_text: correction.as_ref().map(|_| original),
            corrected: correction.is_some(),
            person_id: row
                .get::<_, Option<String>>(8)?
                .map(|id| Uuid::parse_str(&id).unwrap()),
            person_name: row.get(9)?,
            person_origin: row.get(10)?,
            revision: 0,
            speaker_revision: 0,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone)]
pub struct CloudTask {
    pub id: Uuid,
    pub provider: CloudProvider,
    pub target_kind: String,
    pub target_id: String,
    pub target_version: String,
    pub model_id: String,
    pub start: i64,
    pub end: i64,
    pub source: Option<String>,
    pub remote_id: Uuid,
    pub attempts: i64,
    pub submit_attempts: i64,
    pub updated_ms: i64,
}

pub fn recover_tasks(db: &mut Connection, now: i64) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    // A fresh signed process can recover access after an in-place bundle update.
    // Recheck only credential-blocked work; retain provider/network backoff and pause.
    wake_credential_tasks(&tx, None)?;
    tx.execute("UPDATE cloud_tasks SET state='pending',next_attempt_ms=?1 WHERE provider='llm' AND state='processing'",
        params![now]).map_err(|e|e.to_string())?;
    tx.execute(
        "UPDATE candidate_windows SET state='pending' WHERE state='processing'
        AND EXISTS(SELECT 1 FROM cloud_tasks t WHERE t.provider='llm' AND t.target_kind='candidate'
            AND t.target_id=candidate_windows.id AND t.target_version=candidate_windows.input_hash
            AND t.state='pending')",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("UPDATE cloud_tasks SET state='submitted',next_attempt_ms=?1 WHERE provider='asr' AND state='processing'",
        params![now]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
pub fn take_task(
    db: &mut Connection,
    provider: CloudProvider,
    now: i64,
) -> Result<Option<CloudTask>, String> {
    take_task_fair(db, provider, now, 0)
}

pub fn take_task_fair(
    db: &mut Connection,
    provider: CloudProvider,
    now: i64,
    turn: u64,
) -> Result<Option<CloudTask>, String> {
    if provider == CloudProvider::Asr {
        asr::refresh(db, now)?;
    }
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let enabled: bool = tx
        .query_row(
            "SELECT q.configured=1 AND q.paused=0 AND q.blocked_reason IS NULL AND q.cooldown_ms<=?2 AND c.account IS NOT NULL
             FROM cloud_queues q LEFT JOIN cloud_credential_refs c USING(provider) WHERE q.provider=?1",
            params![provider_name(provider), now],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !enabled {
        return Ok(None);
    }
    // Three priority turns retain manual preference; every fourth turn serves
    // the oldest eligible work regardless of priority. One priority turn is
    // newest-first for ASR so a long history import cannot hide current work.
    // Event fitting is chronological: its predecessor is part of the input.
    let order = match (provider, turn % 4) {
        (CloudProvider::Asr, 1) => "manual DESC,started_ms DESC,created_ms DESC",
        (_, 3) => "started_ms,created_ms",
        _ => "manual DESC,started_ms,created_ms",
    };
    let row:Option<(String,String,String,String,String,i64,i64,Option<String>,String,i64,i64,i64,Option<String>)>=tx.query_row(
        &format!("WITH local_work AS ({FIT_LOCAL_PENDING_RANGES})
         SELECT id,target_kind,target_id,target_version,model_id,started_ms,ended_ms,source,state,attempts,
            submit_attempts,updated_ms,remote_id
         FROM cloud_tasks AS task WHERE provider=?1 AND state IN ('pending','submitted') AND next_attempt_ms<=?2
           AND (provider!='llm' OR target_kind!='candidate' OR (
             (manual=1 OR NOT EXISTS(SELECT 1 FROM local_work
               WHERE started_ms<max(task.ended_ms,min(?2,task.started_ms+?4)) AND ended_ms>task.started_ms))
             AND NOT EXISTS(SELECT 1 FROM candidate_windows older
               WHERE older.state IN ('pending','processing')
                 AND older.started_ms<task.started_ms AND older.ended_ms>=task.started_ms-?3)))
         ORDER BY {order} LIMIT 1"),params![provider_name(provider),now,FIT_CONTINUATION_GAP_MS,CANDIDATE_POLICY.max_window_ms],
        |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,
            row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?,row.get(11)?,row.get(12)?)))
        .optional().map_err(|e|e.to_string())?;
    let Some((
        id,
        kind,
        target,
        version,
        model_id,
        start,
        end,
        source,
        state,
        attempts,
        submit_attempts,
        updated_ms,
        remote_id,
    )) = row
    else {
        return Ok(None);
    };
    // ASR's durable request ID is the logical task ID, recorded before any
    // network submission. A restart always queries this ID first.
    let next_state = if provider == CloudProvider::Asr {
        "submitted"
    } else {
        "processing"
    };
    tx.execute("UPDATE cloud_tasks SET state=?2,remote_id=CASE WHEN provider='asr' THEN coalesce(remote_id,id) ELSE remote_id END,
        updated_ms=CASE WHEN ?4='submitted' THEN updated_ms ELSE ?3 END,
        next_attempt_ms=?3+60000 WHERE id=?1",params![id,next_state,now,state])
        .map_err(|e|e.to_string())?;
    if provider == CloudProvider::Llm {
        tx.execute(
            "UPDATE candidate_windows SET state='processing',error_code=NULL
          WHERE id=?1 AND input_hash=?2",
            params![target, version],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Some(CloudTask {
        id: Uuid::parse_str(&id).map_err(|e| e.to_string())?,
        provider,
        target_kind: kind,
        target_id: target,
        target_version: version,
        model_id,
        start,
        end,
        source,
        remote_id: Uuid::parse_str(remote_id.as_deref().unwrap_or(&id))
            .map_err(|e| e.to_string())?,
        attempts,
        submit_attempts,
        updated_ms,
    }))
}

pub fn task_current(db: &Connection, task: &CloudTask) -> Result<bool, String> {
    let state: Option<String> = db
        .query_row(
            "SELECT state FROM cloud_tasks WHERE id=?1",
            params![task.id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if state.as_deref() == Some("superseded") || state.is_none() {
        return Ok(false);
    }
    if task.target_kind == "screenshot" {
        return db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM moments WHERE id=?1 AND kind='screenshot')",
                [&task.target_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string());
    }
    let erased: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM clear_ranges c WHERE (c.mode='all_data' OR (c.mode='raw_audio' AND (SELECT provider='asr' AND submit_attempts=0 FROM cloud_tasks WHERE id=?1)))
         AND c.committed_ms > (SELECT created_ms FROM cloud_tasks WHERE id=?1)
         AND c.from_ms<?3 AND c.to_ms>?2
         AND (?4 IS NULL OR (?4='microphone' AND c.source_mask&1!=0)
              OR (?4='system' AND c.source_mask&2!=0)))",
            params![task.id.to_string(), task.start, task.end, task.source],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if erased {
        return Ok(false);
    }
    if let Some(active) = asr::current(db, task)? {
        return Ok(active);
    }
    let current: Option<String> = match task.target_kind.as_str() {
        "reorganization" => db
            .query_row(
                "SELECT '1' FROM event_reorganization_requests WHERE id=?1",
                [&task.target_id],
                |r| r.get(0),
            )
            .optional(),
        "candidate" => db
            .query_row(
                "SELECT input_hash FROM candidate_windows WHERE id=?1",
                params![task.target_id],
                |row| row.get(0),
            )
            .optional(),
        "event" => db
            .query_row(
                "SELECT printf('%d:%d:%d',source_mask,started_ms,ended_ms) FROM named_events
            WHERE id=?1 AND deleted_ms IS NULL",
                params![task.target_id],
                |row| row.get(0),
            )
            .optional(),
        "range" => db
            .query_row(
                "SELECT cast(revision AS TEXT) FROM manual_ranges WHERE id=?1",
                params![task.target_id],
                |row| row.get(0),
            )
            .optional(),
        _ => return Ok(false),
    }
    .map_err(|e| e.to_string())?;
    Ok(current.as_deref() == Some(&task.target_version))
}

pub fn supersede_task(db: &Connection, task: &CloudTask, now: i64) -> Result<(), String> {
    db.execute(
        "UPDATE cloud_tasks SET state='superseded',updated_ms=?2 WHERE id=?1
        AND state IN ('pending','processing','submitted','failed')",
        params![task.id.to_string(), now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
pub fn retry_task(db: &Connection, id: Uuid, now: i64) -> Result<(), String> {
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    retry_task_in(&tx, id, now)?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn retry_task_in(tx: &Transaction<'_>, id: Uuid, now: i64) -> Result<(), String> {
    // A historical failed row can outlive its event/range. Validate the same
    // current membership as the worker before changing account or retry state.
    let task = cloud_task_by_id(tx, id)?;
    if !task_current(tx, &task)? {
        return Err("task is no longer current".into());
    }
    let row: Option<(String, Option<String>, String, i64)> = tx
        .query_row(
            "SELECT provider,error_code,state,submit_attempts FROM cloud_tasks WHERE id=?1 AND
           (state='failed' OR (state='complete' AND provider='asr' AND
             NOT EXISTS(SELECT 1 FROM precise_segments p WHERE p.task_id=cloud_tasks.id)))",
            params![id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((provider, error, state, submit_attempts)) = row else {
        return Err("task is not retryable".to_owned());
    };
    if task.target_kind == "reorganization" {
        completion::prepare_retry(tx, &task.target_id)?;
    }
    if error.as_deref() == Some("provider_http_402") {
        tx.execute(
            "UPDATE cloud_queues SET blocked_reason=NULL WHERE provider=?1",
            [&provider],
        )
        .map_err(|e| e.to_string())?;
    }
    if provider == "asr" && state == "failed" && error.as_deref().is_some_and(asr::rejected_result)
    {
        return asr::replan_failed(tx, id, now);
    }
    // Once a request may have been accepted, every uncertain failure first
    // queries its original remote ID. Only a confirmed missing request or an
    // explicitly retried completed empty result gets a new paid submission.
    let same_request = provider == "asr"
        && submit_attempts > 0
        && state == "failed"
        && error.as_deref() != Some("provider_missing_submitted_task");
    let new_remote = if provider == "asr" && !same_request {
        Some(Uuid::new_v4().to_string())
    } else {
        None
    };
    let count=tx.execute("UPDATE cloud_tasks SET state=CASE WHEN provider='asr' THEN 'submitted' ELSE 'pending' END,
        next_attempt_ms=?2,error_code=NULL,updated_ms=?2,attempts=0,retry_failures=0,
        remote_id=coalesce(?3,remote_id),
        submit_attempts=CASE WHEN ?3 IS NOT NULL THEN 0 ELSE submit_attempts END
        WHERE id=?1 AND (state='failed' OR (state='complete' AND provider='asr' AND
          NOT EXISTS(SELECT 1 FROM precise_segments p WHERE p.task_id=cloud_tasks.id)))",
        params![id.to_string(),now,new_remote]).map_err(|e|e.to_string())?;
    if count != 1 {
        return Err("task is not retryable".to_owned());
    }
    if provider == "llm" {
        tx.execute(
            "UPDATE candidate_windows SET state='pending',error_code=NULL
           WHERE id=(SELECT target_id FROM cloud_tasks WHERE id=?1)
             AND input_hash=(SELECT target_version FROM cloud_tasks WHERE id=?1)",
            params![id.to_string()],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn cloud_task_by_id(db: &Connection, id: Uuid) -> Result<CloudTask, String> {
    db.query_row("SELECT provider,target_kind,target_id,target_version,model_id,started_ms,ended_ms,source,remote_id,attempts,submit_attempts,updated_ms FROM cloud_tasks WHERE id=?1", [id.to_string()], |r| {
        Ok(CloudTask {
            id, provider: if r.get::<_,String>(0)? == "llm" { CloudProvider::Llm } else { CloudProvider::Asr },
            target_kind: r.get(1)?, target_id: r.get(2)?, target_version: r.get(3)?, model_id: r.get(4)?,
            start: r.get(5)?, end: r.get(6)?, source: r.get(7)?,
            remote_id: Uuid::parse_str(r.get::<_,Option<String>>(8)?.as_deref().unwrap_or(&id.to_string()))
                .map_err(|error| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(error)))?,
            attempts: r.get(9)?, submit_attempts: r.get(10)?, updated_ms: r.get(11)?,
        })
    }).map_err(|e| e.to_string())
}

pub fn defer_paused(db: &mut Connection, task: &CloudTask, now: i64) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let changed = tx
        .execute(
            "UPDATE cloud_tasks SET state=?2,next_attempt_ms=?3 WHERE id=?1
        AND state IN ('processing','submitted')",
            params![
                task.id.to_string(),
                if task.provider == CloudProvider::Llm {
                    "pending"
                } else {
                    "submitted"
                },
                now + 1_000
            ],
        )
        .map_err(|e| e.to_string())?;
    if changed == 1 && task.provider == CloudProvider::Llm {
        tx.execute(
            "UPDATE candidate_windows SET state='pending' WHERE id=?1 AND input_hash=?2
            AND state='processing'",
            params![task.target_id, task.target_version],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
pub fn task_failed(db: &Connection, task: &CloudTask, error: &str, now: i64) -> Result<(), String> {
    task_failed_with_retry(db, task, error, None, now)
}

pub fn task_failed_with_retry(
    db: &Connection,
    task: &CloudTask,
    error: &str,
    retry_after_ms: Option<i64>,
    now: i64,
) -> Result<(), String> {
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let db = &tx;
    let attempts = task.attempts + 1;
    let credential_wait = credential_error(error) || error == "provider_not_configured";
    // Waiting for a local credential is not a failed provider attempt. Keep
    // total attempts for diagnostics/backoff and a separate bounded retry fact.
    let retry_failures: i64 = db
        .query_row(
            "SELECT retry_failures FROM cloud_tasks WHERE id=?1",
            [task.id.to_string()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let retry_failures = retry_failures.saturating_add(i64::from(!credential_wait));
    let transient = error == "provider_deadline"
        || error == "provider_transport"
        || error == "provider_read_failed"
        || error == "provider_read_timeout"
        || error == "credential_missing"
        || error == "provider_not_configured"
        || error.starts_with("keychain_read_")
        || error == "provider_http_429"
        || error.starts_with("provider_http_5")
        || error == "provider_incomplete_output";
    // Model output is a proposal, so an occasional format/boundary rejection
    // can retry in the existing queue. Keep a finite total attempt budget;
    // deterministic ASR mapping failures must never enter this paid path.
    let invalid_fit_output = task.provider == CloudProvider::Llm
        && matches!(task.target_kind.as_str(), "candidate" | "reorganization")
        && (error == "provider_incomplete_output"
            || error == "provider_invalid_json_content"
            || error.starts_with("provider_invalid_json_content_")
            || error == "invalid_fit_schema"
            || error.starts_with("invalid_fit_boundary_"));
    let retry = if invalid_fit_output {
        retry_failures < 3
    } else {
        transient && (credential_wait || retry_failures < 8)
    };
    let scale = attempts.min(10);
    let base_delay = (30_000_i64 * scale * scale).min(10 * 60 * 1000);
    let jitter = (task.id.as_u128() % (base_delay as u128 / 5 + 1)) as i64;
    let delay = (base_delay + jitter).max(retry_after_ms.unwrap_or(0));
    let next = now.saturating_add(delay);
    if error == "provider_http_429" || (error == "provider_http_503" && retry_after_ms.is_some()) {
        db.execute(
            "UPDATE cloud_queues SET cooldown_ms=max(cooldown_ms,?2) WHERE provider=?1",
            params![provider_name(task.provider), next],
        )
        .map_err(|e| e.to_string())?;
    }
    let updated = db
        .execute(
            "UPDATE cloud_tasks SET state=?2,attempts=?3,next_attempt_ms=?4,
        error_code=?5,retry_failures=?7,updated_ms=CASE WHEN provider='asr' THEN updated_ms ELSE ?6 END
        WHERE id=?1 AND state IN ('processing','submitted')",
            params![
                task.id.to_string(),
                if retry {
                    if task.provider == CloudProvider::Asr {
                        "submitted"
                    } else {
                        "pending"
                    }
                } else {
                    "failed"
                },
                attempts,
                next,
                error,
                now
                ,retry_failures
            ],
        )
        .map_err(|e| e.to_string())?;
    if updated > 0 && error == "provider_http_402" {
        db.execute(
            "UPDATE cloud_queues SET blocked_reason='provider_http_402' WHERE provider=?1",
            [provider_name(task.provider)],
        )
        .map_err(|e| e.to_string())?;
    }
    if updated > 0 && task.provider == CloudProvider::Llm {
        db.execute(
            "UPDATE candidate_windows SET state=?2,error_code=?3 WHERE id=?1
            AND input_hash=?4",
            params![
                task.target_id,
                if retry { "pending" } else { "failed" },
                error,
                task.target_version
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn asr_wait(db: &Connection, task: &CloudTask, now: i64) -> Result<(), String> {
    db.execute(
        "UPDATE cloud_tasks SET state='submitted',next_attempt_ms=?2,
        error_code=NULL WHERE id=?1 AND state='submitted'",
        params![task.id.to_string(), now + 5_000],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn mark_submit_intent(db: &Connection, task: &CloudTask, now: i64) -> Result<(), String> {
    let count = db
        .execute(
            "UPDATE cloud_tasks SET submit_attempts=submit_attempts+1,
        remote_id=coalesce(remote_id,id),updated_ms=?2,next_attempt_ms=?2+60000
        WHERE id=?1 AND state='submitted'",
            params![task.id.to_string(), now],
        )
        .map_err(|e| e.to_string())?;
    if count == 1 {
        Ok(())
    } else {
        Err("asr_submit_task_no_longer_current".to_owned())
    }
}

pub fn llm_input(db: &Connection, task: &CloudTask) -> Result<String, String> {
    if task.target_kind == "reorganization" {
        return reorganization::input(db, task);
    }
    if task.target_kind == "event" {
        return rebuild::input(db, task);
    }
    let items = evidence(db, task.start, task.end)?;
    let mut lines = Vec::new();
    let mut catalog = Vec::new();
    let completed = completion::ranges(db, task.start, task.end)?;
    for item in items {
        // A coarse utterance can straddle the computational window. The
        // model owns only its intersection, not the full physical interval.
        // Keep original evidence untouched for adjacent windows and playback.
        let mut spans = vec![(item.start.max(task.start), item.end.min(task.end))];
        for &(a, b, mask) in &completed {
            if source_bit(&item.source) & mask == 0 {
                continue;
            }
            spans = spans
                .into_iter()
                .flat_map(|(x, y)| {
                    if a >= y || b <= x {
                        vec![(x, y)]
                    } else {
                        vec![(x, a.min(y)), (b.max(x), y)]
                    }
                })
                .filter(|(x, y)| y > x)
                .collect();
        }
        if spans.is_empty() {
            continue;
        }
        let evidence_index = item
            .text
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .map(|_| understanding::index(&mut catalog, (&item).into()));
        for (start_ms, end_ms) in spans {
            lines.push(FitObservation {
                evidence_index,
                start_ms,
                end_ms,
                source: item.source.clone(),
                text: item.text.clone().filter(|s| !s.trim().is_empty()),
            });
        }
    }
    let mut context = fit_context(db, task)?;
    understanding::prepare_context(db, &mut context, &mut catalog)?;
    for kind in ["previous_events", "linked_events"] {
        for event in context[kind].as_array_mut().unwrap() {
            event["summary_sentences"] = summary::indexed(event["summary"].as_str());
            event["summary_chars"] =
                serde_json::json!(event["summary"].as_str().map_or(0, |s| s.chars().count()));
        }
    }
    serde_json::to_string(&serde_json::json!({
        "candidate":{"start_ms":task.start,"end_ms":task.end},
        "summary_budget":{"target_chars":900,"tolerance_chars":1100},
        "vocabulary":crate::vocabulary::llm_context(db)?,
        "context":context,
        "observations":lines,
        "confirmed_ranges": completed.iter().map(|(a,b,m)| serde_json::json!({"start_ms":a,"end_ms":b,"sources":sources_for(*m)})).collect::<Vec<_>>(),
        "evidence":catalog
    }))
    .map_err(|e| e.to_string())
}

// This is the context actually sent to the model, not a second persisted owner.
// Compare it again on publication so a late response cannot restore an invalidated summary.
fn fit_context(db: &Connection, task: &CloudTask) -> Result<serde_json::Value, String> {
    let project = |row: &rusqlite::Row<'_>| -> rusqlite::Result<serde_json::Value> {
        Ok(serde_json::json!({"id":row.get::<_,String>(0)?,
            "start_ms":row.get::<_,i64>(1)?,"end_ms":row.get::<_,i64>(2)?,
            "title":row.get::<_,String>(3)?,"summary":row.get::<_,Option<String>>(4)?,
            "revision":row.get::<_,i64>(5)?,"source_mask":row.get::<_,i64>(6)?,
            "boundary_locked":row.get::<_,bool>(7)?,
            "understanding":row.get::<_,Option<String>>(8)?.and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())}))
    };
    // At most one latest predecessor per source mask (microphone/system/both).
    // Parallel activities must each have a legal continuation target.
    let mut previous_stmt = db.prepare(
        "SELECT id,started_ms,ended_ms,title,summary,revision,source_mask,boundary_locked,understanding_json
         FROM (SELECT *,row_number() OVER (PARTITION BY source_mask ORDER BY ended_ms DESC,id) AS position
           FROM named_events WHERE deleted_ms IS NULL AND completed_ms IS NULL AND origin='automatic'
             AND started_ms<?1 AND ended_ms>=?2
             AND id NOT IN (SELECT event_id FROM event_candidate_links WHERE candidate_id=?3))
         WHERE position=1 ORDER BY ended_ms DESC,id LIMIT 3"
    ).map_err(|e| e.to_string())?;
    let mut previous = previous_stmt
        .query_map(
            params![
                task.start,
                task.start - FIT_CONTINUATION_GAP_MS,
                task.target_id
            ],
            project,
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    // These bounded excerpts are context, never candidate-external output.
    for prior in &mut previous {
        let end = prior["end_ms"].as_i64().unwrap();
        let start = prior["start_ms"]
            .as_i64()
            .unwrap()
            .max(end - FIT_CONTEXT_TAIL_MS);
        let mask = prior["source_mask"].as_i64().unwrap();
        let items = evidence(db, start, end)?;
        let mut tail = Vec::new();
        for item in items
            .iter()
            .rev()
            .filter(|item| {
                source_bit(&item.source) & mask != 0
                    && item.text.as_ref().is_some_and(|s| !s.trim().is_empty())
            })
            .take(FIT_CONTEXT_MAX_LINES)
        {
            let text = item.text.as_ref().unwrap();
            tail.push(serde_json::json!({
                "start_ms": item.start.max(start), "end_ms": item.end.min(end),
                "source": item.source, "text": text.chars().take(FIT_CONTEXT_TEXT_CHARS).collect::<String>(),
                "text_truncated": text.chars().count()>FIT_CONTEXT_TEXT_CHARS
            }));
        }
        tail.reverse();
        prior["tail"] = serde_json::json!(tail);
    }
    let mut stmt = db
        .prepare(
            "SELECT e.id,e.started_ms,e.ended_ms,e.title,e.summary,e.revision,e.source_mask,e.boundary_locked,e.understanding_json,l.slot
        FROM event_candidate_links l JOIN named_events e ON e.id=l.event_id
        WHERE l.candidate_id=?1 AND e.deleted_ms IS NULL AND e.completed_ms IS NULL ORDER BY l.slot",
        )
        .map_err(|e| e.to_string())?;
    let linked = stmt
        .query_map(params![task.target_id], |row| {
            let mut value = project(row)?;
            value["slot"] = serde_json::json!(row.get::<_, i64>(9)?);
            Ok(value)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"previous_events":previous,"linked_events":linked}))
}

fn fit_summary(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.contains('\0') {
        return None;
    }
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

// A coarse-only summary cannot override a user's precise-text correction.
fn interpretation_for_scope(
    db: &Connection,
    start: i64,
    end: i64,
    mask: i64,
    summary: Option<String>,
) -> Result<Option<String>, String> {
    let corrected: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM text_corrections
        WHERE state IN ('active','pending_review') AND started_ms<?2 AND ended_ms>?1
        AND ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)))",
            params![start, end, mask],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(if corrected { None } else { summary })
}

pub fn invalidate_event_understanding(
    db: &Connection,
    start: i64,
    end: i64,
    mask: i64,
) -> Result<(), String> {
    db.execute("UPDATE named_events SET summary=NULL,understanding_json=NULL WHERE started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0",
        params![start,end,mask]).map_err(|e|e.to_string())?;
    Ok(())
}

pub const FIT_SYSTEM: &str = include_str!("events/fit_prompt.txt");

#[derive(serde::Serialize, serde::Deserialize)]
struct FitObservation {
    #[serde(default)]
    evidence_index: Option<usize>,
    start_ms: i64,
    end_ms: i64,
    source: String,
    text: Option<String>,
}

#[cfg(test)]
pub fn publish_fit(
    db: &mut Connection,
    task: &CloudTask,
    result: serde_json::Value,
    now: i64,
) -> Result<usize, String> {
    let input = llm_input(db, task)?;
    publish_fit_with_input(db, task, &input, result, now)
}

// Validate all ranges before publishing any sibling. No partial success from a
// malformed model timeline; user-owned conflicts are handled later as before.
fn validate_fit_ranges(
    events: &[serde_json::Value],
    start_ms: i64,
    end_ms: i64,
) -> Result<(), String> {
    let mut spans = Vec::new();
    for event in events {
        let (Some(start), Some(end)) = (event["start_ms"].as_i64(), event["end_ms"].as_i64())
        else {
            return Err("invalid_fit_boundary_type".into());
        };
        if end <= start || start < start_ms || end > end_ms {
            return Err("invalid_fit_boundary_range".into());
        }
        if let Ok(source) = serde_json::from_value::<EventSources>(event["sources"].clone()) {
            let mask = source_mask(source);
            if spans
                .iter()
                .any(|&(a, b, m)| start < b && end > a && mask & m != 0)
            {
                return Err("invalid_fit_boundary_overlap".into());
            }
            spans.push((start, end, mask));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_generated_event(
    tx: &Transaction<'_>,
    id: &str,
    start: i64,
    end: i64,
    mask: i64,
    title: &str,
    summary: Option<String>,
    understanding: Option<String>,
    now: i64,
) -> Result<(), String> {
    tx.execute("INSERT INTO named_events (id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,deleted_ms,created_ms,summary,understanding_json) VALUES (?1,?2,?3,?4,?5,1,'automatic',0,0,1,NULL,?6,?7,?8)",params![id,start,end,title,mask,now,summary,understanding]).map_err(|e|e.to_string())?;
    Ok(())
}

pub fn publish_fit_with_input(
    db: &mut Connection,
    task: &CloudTask,
    input: &str,
    result: serde_json::Value,
    now: i64,
) -> Result<usize, String> {
    if task.target_kind == "event" {
        return rebuild::publish(db, task, input, result, now);
    }
    let mut result = result;
    let sent_vocabulary: serde_json::Value =
        serde_json::from_str(input).map_err(|_| "invalid_fit_input")?;
    crate::vocabulary::normalize_generated(&mut result, &sent_vocabulary["vocabulary"]);
    let events = result["events"].as_array().ok_or("invalid_fit_schema")?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    if !task_current(&tx, task)? {
        tx.execute(
            "UPDATE cloud_tasks SET state='superseded',updated_ms=?2 WHERE id=?1",
            params![task.id.to_string(), now],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let sent: serde_json::Value = serde_json::from_str(input).map_err(|_| "invalid_fit_input")?;
    let current: serde_json::Value =
        serde_json::from_str(&llm_input(&tx, task)?).map_err(|_| "invalid_fit_input")?;
    let context_current = sent == current;
    if task.target_kind == "reorganization" {
        if !context_current {
            return Err("reorganization_revision_conflict".into());
        }
        let count = reorganization::apply(&tx, task, &sent, &result, now)?;
        tx.execute(
            "UPDATE cloud_tasks SET state='complete',error_code=NULL,updated_ms=?2 WHERE id=?1",
            params![task.id.to_string(), now],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(count);
    }
    validate_fit_ranges(events, task.start, task.end)?;
    // A stale semantic response must not erase a newer, still valid snapshot
    // (for example after a title-only edit). Compare actual observation anchors
    // as well as text: request-local indices alone do not identify evidence.
    let observations_current = sent["observations"] == current["observations"]
        && sent["observations"].as_array().is_some_and(|items| {
            items.iter().all(|item| {
                item["evidence_index"].as_u64().is_none_or(|i| {
                    sent["evidence"].get(i as usize) == current["evidence"].get(i as usize)
                })
            })
        });
    let mut applied = 0;
    // Decode independently, preserving original slots even when an entry is
    // unusable. Optional metadata never decides whether a time range survives.
    for (slot, event) in events.iter().enumerate() {
        let (Some(start), Some(end), Ok(sources)) = (
            event["start_ms"].as_i64(),
            event["end_ms"].as_i64(),
            serde_json::from_value::<EventSources>(event["sources"].clone()),
        ) else {
            continue;
        };
        let title = event["title"]
            .as_str()
            .filter(|title| validate_title(title).is_ok());
        let mask = source_mask(sources);
        let seen = evidence(&tx, start, end)?;
        if (mask & 1 != 0 && !seen.iter().any(|item| item.source == "microphone"))
            || (mask & 2 != 0 && !seen.iter().any(|item| item.source == "system"))
        {
            continue;
        }
        if suppressed(&tx, start, end, mask)? {
            continue;
        }
        let linked: Option<String> = tx
            .query_row(
                "SELECT event_id FROM event_candidate_links
            WHERE candidate_id=?1 AND slot=?2",
                params![task.target_id, slot as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let linked = match linked {
            Some(id) if completion::completed_at(&tx, &id)?.is_some() => None,
            other => other,
        };
        if let Some(linked_id) = linked {
            let old:Option<(i64,i64,i64,bool,bool,String,i64,Option<String>,Option<String>)>=tx.query_row(
                "SELECT started_ms,ended_ms,revision,boundary_locked,title_locked,title,source_mask,summary,understanding_json FROM named_events
                 WHERE id=?1 AND origin='automatic' AND deleted_ms IS NULL AND completed_ms IS NULL",
                 params![linked_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?)))
                 .optional().map_err(|e|e.to_string())?;
            if let Some((
                old_start,
                old_end,
                revision,
                boundary_locked,
                title_locked,
                old_title,
                old_mask,
                old_summary,
                old_understanding,
            )) = old
            {
                let links: i64 = tx
                    .query_row(
                        "SELECT count(*) FROM event_candidate_links WHERE event_id=?1",
                        params![linked_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let (new_start, new_end) = if boundary_locked {
                    (old_start, old_end)
                } else if links > 1 {
                    (old_start.min(start), old_end.max(end))
                } else {
                    (start, end)
                };
                let new_mask = if boundary_locked {
                    old_mask
                } else if links > 1 {
                    old_mask | mask
                } else {
                    mask
                };
                let new_title = if title_locked {
                    old_title.as_str()
                } else {
                    title.unwrap_or(old_title.as_str())
                };
                let linked_context =
                    sent["context"]["linked_events"]
                        .as_array()
                        .and_then(|entries| {
                            entries
                                .iter()
                                .find(|entry| entry["slot"].as_u64() == Some(slot as u64))
                        });
                let keep_current = !context_current
                    && observations_current
                    && (old_start, old_end, old_mask) == (new_start, new_end, new_mask);
                let current_linked =
                    current["context"]["linked_events"]
                        .as_array()
                        .and_then(|items| {
                            items
                                .iter()
                                .find(|entry| entry["id"].as_str() == Some(&linked_id))
                        });
                let summary = if keep_current {
                    old_summary.clone()
                } else if boundary_locked
                    || linked_context
                        .is_none_or(|entry| entry["revision"].as_i64() != Some(revision))
                    || ((new_start < task.start || new_end > task.end) && old_summary.is_none())
                {
                    None
                } else {
                    if context_current {
                        summary::apply(
                            &event["summary"],
                            linked_context,
                            &sent,
                            new_start,
                            new_end,
                            new_mask,
                        )
                    } else {
                        None
                    }
                };
                let summary = interpretation_for_scope(&tx, new_start, new_end, new_mask, summary)?;
                let understanding = if keep_current
                    && current_linked.is_some_and(|entry| !entry["understanding"].is_null())
                {
                    old_understanding.clone()
                } else if !context_current
                    || boundary_locked
                    || linked_context
                        .is_none_or(|entry| entry["revision"].as_i64() != Some(revision))
                    || ((new_start < task.start || new_end > task.end)
                        && linked_context.is_none_or(|entry| entry["understanding"].is_null()))
                {
                    None
                } else {
                    understanding::normalize(
                        &event["understanding"],
                        &sent,
                        linked_context,
                        new_start,
                        new_end,
                        new_mask,
                    )
                };
                let understanding =
                    interpretation_for_scope(&tx, new_start, new_end, new_mask, understanding)?;
                if old_start == new_start
                    && old_end == new_end
                    && old_mask == new_mask
                    && old_title == new_title
                    && old_summary == summary
                    && old_understanding == understanding
                {
                    continue;
                }
                if fit_nonoverlap(&tx, new_start, new_end, new_mask, Some(&linked_id))? {
                    tx.execute(
                        "UPDATE named_events SET started_ms=?2,ended_ms=?3,title=?4,
                        source_mask=?5,summary=?6,understanding_json=?7,revision=revision+1 WHERE id=?1",
                        params![linked_id, new_start, new_end, new_title, new_mask, summary, understanding],
                    )
                    .map_err(|e| e.to_string())?;
                    if old_start != new_start || old_end != new_end || old_mask != new_mask {
                        queue_asr(
                            &tx,
                            "event",
                            &linked_id,
                            &asr_version(new_mask, new_start, new_end),
                            new_start,
                            new_end,
                            new_mask,
                            false,
                            now,
                        )?;
                    }
                    applied += 1;
                }
            }
            continue;
        }
        if let Some(prior_id) = event["merge_with_event_id"].as_str() {
            let prior:Option<(i64,i64,i64,bool,bool,i64,Option<String>,Option<String>)>=tx.query_row(
                "SELECT started_ms,ended_ms,revision,boundary_locked,title_locked,source_mask,summary,understanding_json FROM named_events
                 WHERE id=?1 AND origin='automatic' AND deleted_ms IS NULL AND completed_ms IS NULL",
                params![prior_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?)))
                .optional().map_err(|e|e.to_string())?;
            if let Some((
                prior_start,
                prior_end,
                prior_revision,
                false,
                title_locked,
                prior_mask,
                prior_summary,
                prior_understanding,
            )) = prior
            {
                let prior_context =
                    sent["context"]["previous_events"]
                        .as_array()
                        .and_then(|items| {
                            items
                                .iter()
                                .find(|entry| entry["id"].as_str() == Some(prior_id))
                        });
                if prior_context.is_some()
                    && prior_start < task.start
                    && start.saturating_sub(prior_end) <= FIT_CONTINUATION_GAP_MS
                    && fit_nonoverlap(
                        &tx,
                        prior_start,
                        prior_end.max(end),
                        prior_mask | mask,
                        Some(prior_id),
                    )?
                {
                    let merged_mask = prior_mask | mask;
                    let merged_end = prior_end.max(end);
                    let keep_current = !context_current
                        && observations_current
                        && (prior_end, prior_mask) == (merged_end, merged_mask);
                    let current_prior =
                        current["context"]["previous_events"]
                            .as_array()
                            .and_then(|items| {
                                items
                                    .iter()
                                    .find(|entry| entry["id"].as_str() == Some(prior_id))
                            });
                    let summary = if keep_current {
                        prior_summary.clone()
                    } else if prior_summary.is_some()
                        && prior_context.and_then(|entry| entry["revision"].as_i64())
                            == Some(prior_revision)
                    {
                        if context_current {
                            summary::apply(
                                &event["summary"],
                                prior_context,
                                &sent,
                                prior_start,
                                merged_end,
                                merged_mask,
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let summary = interpretation_for_scope(
                        &tx,
                        prior_start,
                        merged_end,
                        merged_mask,
                        summary,
                    )?;
                    let understanding = if keep_current
                        && current_prior.is_some_and(|entry| !entry["understanding"].is_null())
                    {
                        prior_understanding
                    } else if context_current
                        && prior_context.is_some_and(|entry| {
                            !entry["understanding"].is_null()
                                && entry["revision"].as_i64() == Some(prior_revision)
                        })
                    {
                        understanding::normalize(
                            &event["understanding"],
                            &sent,
                            prior_context,
                            prior_start,
                            merged_end,
                            merged_mask,
                        )
                    } else {
                        None
                    };
                    let understanding = interpretation_for_scope(
                        &tx,
                        prior_start,
                        merged_end,
                        merged_mask,
                        understanding,
                    )?;
                    // The prior ID is checked in the query and retained as the
                    // same event when an activity spans compute windows.
                    tx.execute(
                        "UPDATE named_events SET ended_ms=?2,source_mask=?5,
                        title=CASE WHEN ?4 THEN title ELSE ?3 END,summary=?6,understanding_json=?7,revision=revision+1
                        WHERE id=?1",
                        params![
                            prior_id,
                            merged_end,
                            title.unwrap_or("普通会话"),
                            title_locked || title.is_none(),
                            merged_mask,
                            summary,
                            understanding
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                    queue_asr(
                        &tx,
                        "event",
                        prior_id,
                        &asr_version(merged_mask, prior_start, merged_end),
                        prior_start,
                        merged_end,
                        merged_mask,
                        false,
                        now,
                    )?;
                    tx.execute(
                        "INSERT INTO event_candidate_links VALUES (?1,?2,?3) ON CONFLICT(candidate_id,slot) DO UPDATE SET event_id=excluded.event_id",
                        params![task.target_id, slot as i64, prior_id],
                    )
                    .map_err(|e| e.to_string())?;
                    applied += 1;
                    continue;
                }
            }
        }
        if !fit_nonoverlap(&tx, start, end, mask, None)? {
            continue;
        }
        let summary = if context_current {
            summary::apply(&event["summary"], None, &sent, start, end, mask)
        } else {
            None
        };
        let summary = interpretation_for_scope(&tx, start, end, mask, summary)?;
        let understanding = if context_current {
            understanding::normalize(&event["understanding"], &sent, None, start, end, mask)
        } else {
            None
        };
        let understanding = interpretation_for_scope(&tx, start, end, mask, understanding)?;
        let id = Uuid::new_v4().to_string();
        insert_generated_event(
            &tx,
            &id,
            start,
            end,
            mask,
            title.unwrap_or("普通会话"),
            summary,
            understanding,
            now,
        )?;
        tx.execute(
            "INSERT INTO event_candidate_links VALUES (?1,?2,?3) ON CONFLICT(candidate_id,slot) DO UPDATE SET event_id=excluded.event_id",
            params![task.target_id, slot as i64, id],
        )
        .map_err(|e| e.to_string())?;
        queue_asr(
            &tx,
            "event",
            &id,
            &asr_version(mask, start, end),
            start,
            end,
            mask,
            false,
            now,
        )?;
        applied += 1;
    }
    // Omitted or invalid entries are not deletion instructions. Keep previously
    // published slots; explicit user edits/cleanup still own removal.
    tx.execute(
        "UPDATE cloud_tasks SET state='complete',error_code=NULL,updated_ms=?2 WHERE id=?1",
        params![task.id.to_string(), now],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("UPDATE candidate_windows SET state='complete',error_code=NULL WHERE id=?1 AND input_hash=?2",
        params![task.target_id,task.target_version]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(applied)
}

pub fn publish_asr(
    db: &mut Connection,
    task: &CloudTask,
    utterances: &[crate::cloud::AsrUtterance],
    now: i64,
) -> Result<usize, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let completed: bool = tx
        .query_row(
            "SELECT state='complete' FROM cloud_tasks WHERE id=?1",
            [task.id.to_string()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if completed {
        return Ok(0);
    }
    if !task_current(&tx, task)? {
        tx.execute(
            "UPDATE cloud_tasks SET state='superseded',updated_ms=?2 WHERE id=?1",
            params![task.id.to_string(), now],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let source = task
        .source
        .as_deref()
        .ok_or_else(|| "asr task missing source".to_owned())?;
    let mapped = if let Some(input) = asr::input(&tx, task.id)? {
        asr::map(&input, utterances)?
    } else {
        utterances
            .iter()
            .map(|u| {
                let mut item = u.clone();
                item.start_ms = task
                    .start
                    .checked_add(u.start_ms)
                    .ok_or("provider_invalid_timing")?;
                item.end_ms = task
                    .start
                    .checked_add(u.end_ms)
                    .ok_or("provider_invalid_timing")?;
                Ok(item)
            })
            .collect::<Result<Vec<_>, String>>()?
    };
    // The submitted input owns the transcript clock. Retention, VAD and later
    // capture-gap metadata govern playback/voice, not whether text is kept.
    let mut inserted_segments = Vec::new();
    for utterance in &mapped {
        let start = utterance.start_ms;
        let end = utterance.end_ms;
        if start < task.start || end > task.end || end <= start {
            return Err("provider_invalid_timing".into());
        }
        let segment_id = Uuid::new_v4();
        tx.execute(
            "INSERT INTO precise_segments VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                segment_id.to_string(),
                task.id.to_string(),
                source,
                start,
                end,
                utterance.text,
                task.id.to_string(),
                utterance.speaker
            ],
        )
        .map_err(|e| e.to_string())?;
        inserted_segments.push((segment_id, source.to_owned(), start, end));
        if let Some(speaker) = &utterance.speaker {
            crate::knowledge::register_speaker(&tx, &task.id.to_string(), speaker)?;
            crate::knowledge::enqueue_voice_observation(
                &tx,
                segment_id,
                &format!("{}:{speaker}", task.id),
                source,
                start,
                end,
                now,
            )?;
        }
    }
    crate::knowledge::reconcile_corrections(&tx, &inserted_segments, now)?;
    tx.execute(
        "UPDATE cloud_tasks SET state='complete',updated_ms=?2,error_code=NULL WHERE id=?1",
        params![task.id.to_string(), now],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(utterances.len())
}

pub(crate) fn utterance_has_retained_audio(
    db: &Connection,
    source: &str,
    start: i64,
    end: i64,
    allow_cleared_original: bool,
) -> Result<bool, String> {
    // Each block is timestamped from a wall-clock anchor, while its length is
    // measured in audio frames. Adjacent live blocks can therefore differ by
    // a few milliseconds even though the samples are continuous. Keep the
    // real gap check below and reject larger holes caused by discarded audio.
    const BLOCK_CLOCK_TOLERANCE_MS: i64 = 10;
    let gap: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM media_gaps WHERE source=?1
                AND started_ms<?3 AND coalesce(ended_ms,?3)>?2)",
            params![source, start, end],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if gap {
        return Ok(false);
    }
    let mut statement = db
        .prepare(
            "SELECT started_ms,ended_ms FROM audio_blocks WHERE source=?1
            AND (media_path IS NOT NULL OR (?4 AND retention_decision='raw_audio'))
            AND started_ms<?3 AND ended_ms>?2
            ORDER BY started_ms,ended_ms",
        )
        .map_err(|e| e.to_string())?;
    let spans = statement
        .query_map(params![source, start, end, allow_cleared_original], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut covered = start;
    for span in spans {
        let (block_start, block_end) = span.map_err(|e| e.to_string())?;
        if block_start - covered > BLOCK_CLOCK_TOLERANCE_MS {
            return Ok(false);
        }
        covered = covered.max(block_end);
        if covered >= end {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug, Clone)]
pub struct AudioPart {
    pub start: i64,
    pub end: i64,
    pub frames: usize,
    pub path: PathBuf,
}

pub fn audio_manifest(db: &Connection, task: &CloudTask) -> Result<Vec<AudioPart>, String> {
    let source = task
        .source
        .as_deref()
        .ok_or_else(|| "ASR source missing".to_owned())?;
    let mut statement = db
        .prepare(
            "SELECT started_ms,ended_ms,frames,media_path FROM audio_blocks
        WHERE source=?1 AND media_path IS NOT NULL AND started_ms < ?3 AND ended_ms > ?2
        ORDER BY started_ms,id",
        )
        .map_err(|e| e.to_string())?;
    statement
        .query_map(params![source, task.start, task.end], |row| {
            Ok(AudioPart {
                start: row.get(0)?,
                end: row.get(1)?,
                frames: row.get::<_, i64>(2)? as usize,
                path: PathBuf::from(row.get::<_, String>(3)?),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn voice_manifest(
    db: &Connection,
    source: &str,
    start: i64,
    end: i64,
) -> Result<(CloudTask, Vec<AudioPart>), String> {
    if end - start < 400 || end - start > 15_000 || !matches!(source, "microphone" | "system") {
        return Err("speaker audio range unsuitable".to_owned());
    }
    if !utterance_has_retained_audio(db, source, start, end, false)? {
        return Err("speaker audio unavailable".to_owned());
    }
    let task = CloudTask {
        id: Uuid::nil(),
        provider: CloudProvider::Asr,
        target_kind: "range".into(),
        target_id: String::new(),
        target_version: String::new(),
        model_id: String::new(),
        start,
        end,
        source: Some(source.to_owned()),
        remote_id: Uuid::nil(),
        attempts: 0,
        submit_attempts: 0,
        updated_ms: 0,
    };
    let parts = audio_manifest(db, &task)?;
    Ok((task, parts))
}

pub fn voice_pcm_16k(
    directory: &Path,
    task: &CloudTask,
    parts: &[AudioPart],
    context: &mut loppi_protocol::LogContext,
) -> Result<Vec<f32>, String> {
    let wav = assemble_wav(directory, task, parts)?;
    let samples: Vec<f32> = wav
        .get(44..)
        .ok_or_else(|| "speaker WAV invalid".to_owned())?
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0)
        .collect();
    let power =
        samples.iter().map(|value| value * value).sum::<f32>() / samples.len().max(1) as f32;
    let clipped = samples.iter().filter(|value| value.abs() > 0.98).count();
    context.rms_millionths = Some((power.sqrt() * 1_000_000.0) as u64);
    context.clipped_ppm = Some((clipped * 1_000_000 / samples.len().max(1)) as u64);
    context.sample_frames = Some(samples.len() as u64);
    if power.sqrt() < 0.005 || clipped * 20 > samples.len() {
        return Err("speaker audio level unsuitable".to_owned());
    }
    Ok(samples)
}

/// Concatenate only explicitly selected speech ranges; intervening audio is never included.
pub fn voice_input_16k(
    directory: &Path,
    manifests: &[(CloudTask, Vec<AudioPart>)],
    composite: bool,
    context: &mut loppi_protocol::LogContext,
) -> Result<Vec<f32>, String> {
    let mut pcm = Vec::new();
    for (task, parts) in manifests {
        let piece = voice_pcm_16k(directory, task, parts, context)?;
        if composite && piece.iter().filter(|v| v.abs() >= 0.98).count() * 100 > piece.len() {
            return Err("speaker audio level unsuitable".into());
        }
        pcm.extend(piece);
    }
    Ok(pcm)
}

pub fn recover_asr_entry(entry: &std::fs::DirEntry) {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    let Some(suffix) = name.strip_prefix(".asr-ogg-") else {
        return;
    };
    if suffix.len() == 12
        && suffix.bytes().all(|b| b.is_ascii_alphanumeric())
        && entry
            .file_type()
            .is_ok_and(|t| t.is_dir() && !t.is_symlink())
    {
        let _ = std::fs::remove_dir_all(entry.path());
    }
}
#[cfg(test)]
pub fn recover_asr_staging(directory: &Path) {
    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            recover_asr_entry(&entry);
        }
    }
}

/// Build one cloud input on the real-time axis. Local blocks may have gaps;
/// those gaps remain silence so the provider's relative times still map to
/// `task.start + offset` without a second stitch map. Encode in ten-second
/// slabs to avoid holding a whole meeting as decoded PCM in the core.
pub fn assemble_ogg(
    directory: &Path,
    task: &CloudTask,
    parts: &[AudioPart],
) -> Result<Vec<u8>, String> {
    let duration_ms = task.end - task.start;
    if duration_ms <= 0 || duration_ms > ASR_WINDOW_MS + 11_000 {
        return Err("ASR task duration invalid".to_owned());
    }
    for part in parts {
        let valid_path = part.path.components().count() == 3
            && part
                .path
                .components()
                .next()
                .is_some_and(|c| c.as_os_str() == "media")
            && part.path.extension().is_some_and(|e| e == "opus");
        if !valid_path || part.frames == 0 || part.frames > 48_000 * 11 {
            return Err("invalid media manifest".to_owned());
        }
    }
    let scratch = tempfile::Builder::new()
        .prefix(".asr-ogg-")
        .rand_bytes(12)
        .tempdir_in(directory)
        .map_err(|error| error.to_string())?;
    let mut encoded = crate::media::OpusBlock::create(scratch.path(), Uuid::new_v4())?;
    let mut had_media = false;
    let mut chunk_start = task.start;
    while chunk_start < task.end {
        let chunk_end = (chunk_start + 10_000).min(task.end);
        let mut pcm = vec![0.0f32; ((chunk_end - chunk_start) as usize) * 48];
        for part in parts {
            let clip_start = part.start.max(chunk_start);
            let clip_end = part.end.min(chunk_end);
            if clip_end <= clip_start {
                continue;
            }
            let audio = crate::media::decode_block(&directory.join(&part.path), part.frames)?;
            let src_offset = ((clip_start - part.start) as usize * 48).min(audio.len());
            let dst_offset = ((clip_start - chunk_start) as usize * 48).min(pcm.len());
            let span = ((clip_end - clip_start) as usize * 48)
                .min(audio.len() - src_offset)
                .min(pcm.len() - dst_offset);
            pcm[dst_offset..dst_offset + span]
                .copy_from_slice(&audio[src_offset..src_offset + span]);
            had_media |= span > 0;
        }
        for frame in pcm.chunks(960) {
            encoded.push(frame)?;
        }
        chunk_start = chunk_end;
    }
    if !had_media {
        return Err("ASR source audio unavailable".to_owned());
    }
    let sealed = encoded.seal()?;
    if sealed.bytes > 10_000_000 {
        return Err("asr_audio_size".to_owned());
    }
    std::fs::read(sealed.path).map_err(|error| error.to_string())
}

pub fn assemble_wav(
    directory: &Path,
    task: &CloudTask,
    parts: &[AudioPart],
) -> Result<Vec<u8>, String> {
    let duration_ms = task.end - task.start;
    if duration_ms <= 0 || duration_ms > ASR_WINDOW_MS + 11_000 {
        return Err("ASR task duration invalid".to_owned());
    }
    let frames = ((duration_ms as usize) * 48).max(1);
    let mut pcm = vec![0.0f32; frames];
    let mut had_media = false;
    for part in parts {
        let valid_path = part.path.components().count() == 3
            && part
                .path
                .components()
                .next()
                .is_some_and(|c| c.as_os_str() == "media")
            && part.path.extension().is_some_and(|e| e == "opus");
        if !valid_path || part.frames == 0 || part.frames > 48_000 * 11 {
            return Err("invalid media manifest".to_owned());
        }
        let audio = crate::media::decode_block(&directory.join(&part.path), part.frames)?;
        let clip_start = part.start.max(task.start);
        let clip_end = part.end.min(task.end);
        if clip_end <= clip_start {
            continue;
        }
        let src_offset = ((clip_start - part.start) as usize * 48).min(audio.len());
        let dst_offset = ((clip_start - task.start) as usize * 48).min(pcm.len());
        let span = ((clip_end - clip_start) as usize * 48)
            .min(audio.len() - src_offset)
            .min(pcm.len() - dst_offset);
        pcm[dst_offset..dst_offset + span].copy_from_slice(&audio[src_offset..src_offset + span]);
        had_media |= span > 0;
    }
    if !had_media {
        return Err("ASR source audio unavailable".to_owned());
    }
    let resampler = LinearResampler::create(48_000, 16_000)
        .ok_or_else(|| "ASR resampler unavailable".to_owned())?;
    let mut mono = resampler.resample(&pcm, true);
    let samples = (duration_ms as usize) * 16;
    mono.truncate(samples);
    mono.resize(samples, 0.0);
    let data_bytes = mono.len() * 2;
    let mut wav = Vec::with_capacity(44 + data_bytes);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((data_bytes + 36) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for sample in mono {
        wav.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    Ok(wav)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        crate::Store::new(db).unwrap().db
    }

    fn summary_fixture() -> (Connection, CloudTask) {
        let mut db = db();
        coarse(
            &db,
            "summary-speech",
            1_000,
            10_000,
            "比较页面布局，保留色块。",
        );
        media(&db, "summary-audio", 1_000, 10_000);
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        refresh_candidates(&mut db, 100_000, true).unwrap();
        let task = take_task(&mut db, CloudProvider::Llm, 100_000)
            .unwrap()
            .unwrap();
        (db, task)
    }

    fn summary_result(task: &CloudTask, summary: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"events":[{"start_ms":task.start,"end_ms":task.end,
            "title":"界面布局讨论","summary":summary,"sources":"microphone","merge_with_event_id":null}]})
    }

    #[test]
    fn automatic_candidates_close_on_wall_clock_deadline_and_remain_idempotent() {
        let mut db = db();
        let start = 1_000;
        let deadline = start + CANDIDATE_POLICY.max_window_ms;
        // Real observed failure: the last sentence ends seven milliseconds short.
        coarse(&db, "almost-one-hour", start, deadline - 7, "持续讨论");
        assert_eq!(refresh_candidates(&mut db, deadline - 1, false).unwrap(), 0);
        assert_eq!(refresh_candidates(&mut db, deadline, false).unwrap(), 1);
        assert_eq!(
            refresh_candidates(&mut db, deadline + 15_000, false).unwrap(),
            0
        );
        coarse(
            &db,
            "next-window",
            deadline + 1,
            deadline + 10_000,
            "下一段",
        );
        assert_eq!(
            refresh_candidates(&mut db, deadline + 10_001, false).unwrap(),
            0
        );
        assert_eq!(
            refresh_candidates(
                &mut db,
                deadline + 10_000 + CANDIDATE_POLICY.quiet_cut_ms,
                false
            )
            .unwrap(),
            1
        );
        let count: i64 = db
            .query_row("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn credential_backoff_is_visible_and_rechecked_after_restart_or_save() {
        let (mut db, task) = summary_fixture();
        task_failed(&db, &task, "keychain_read_-25293", 100_001).unwrap();
        assert_eq!(
            unorganized(&db, 0, 110_000, 1).unwrap().processing_state,
            "fit_credentials_unavailable"
        );
        assert!(
            take_task(&mut db, CloudProvider::Llm, 100_002)
                .unwrap()
                .is_none()
        );
        set_paused(&db, CloudProvider::Llm, true).unwrap();
        recover_tasks(&mut db, 100_003).unwrap();
        assert!(
            take_task(&mut db, CloudProvider::Llm, 100_004)
                .unwrap()
                .is_none()
        );
        set_paused(&db, CloudProvider::Llm, false).unwrap();
        let retried = take_task(&mut db, CloudProvider::Llm, 100_005)
            .unwrap()
            .unwrap();
        assert_eq!(retried.id, task.id);
        assert_eq!(
            unorganized(&db, 0, 110_000, 1).unwrap().processing_state,
            "fitting"
        );
        task_failed(&db, &retried, "credential_missing", 100_006).unwrap();
        set_configured_with_credential(
            &db,
            CloudProvider::Llm,
            "deepseek-flash",
            "test:replacement",
        )
        .unwrap();
        let retried = take_task(&mut db, CloudProvider::Llm, 100_007)
            .unwrap()
            .unwrap();
        task_failed(&db, &retried, "provider_transport", 100_008).unwrap();
        assert_eq!(
            unorganized(&db, 0, 110_000, 1).unwrap().processing_state,
            "fit_retrying"
        );
        recover_tasks(&mut db, 100_009).unwrap();
        set_configured_with_credential(
            &db,
            CloudProvider::Llm,
            "deepseek-flash",
            "test:replacement",
        )
        .unwrap();
        assert!(
            take_task(&mut db, CloudProvider::Llm, 100_010)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn newer_complete_candidate_does_not_hide_earlier_blocked_fit() {
        let (mut db, task) = summary_fixture();
        task_failed(&db, &task, "keychain_read_-25293", 100_001).unwrap();
        db.execute(
            "INSERT INTO candidate_windows VALUES ('newer',20000,30000,'hash','complete',NULL)",
            [],
        )
        .unwrap();
        assert_eq!(
            unorganized(&db, 0, 110_000, 1).unwrap().processing_state,
            "fit_credentials_unavailable"
        );
        let event = create_event(
            &mut db,
            1_000,
            30_000,
            "讨论",
            EventSources::Microphone,
            100_002,
        )
        .unwrap();
        db.execute(
            "INSERT INTO event_candidate_links VALUES (?1,0,?2)",
            params![task.target_id, event.to_string()],
        )
        .unwrap();
        assert_eq!(
            event_fit_state(&db, &event.to_string()).unwrap(),
            "fit_credentials_unavailable"
        );
        assert_eq!(
            candidate_status("complete", Some("keychain_read_-25293")),
            "unorganized"
        );
    }

    #[test]
    fn prompt_output_examples_follow_event_contract() {
        use serde_json::{Value, json};

        let normalized = FIT_SYSTEM.replace("\r\n", "\n");
        let blocks: Vec<Value> = normalized
            .split("```json\n")
            .skip(1)
            .map(|block| serde_json::from_str(block.split_once("\n```").unwrap().0).unwrap())
            .collect();
        let [schema, point, update, compress] = blocks.as_slice() else {
            panic!("expected the output schema and three incremental edit examples");
        };
        for (example, kind) in [
            (point, "point"),
            (update, "summary_edit"),
            (compress, "summary_edit"),
        ] {
            let required = schema["$defs"][kind]["required"].as_array().unwrap();
            assert_eq!(example.as_object().unwrap().len(), required.len());
            for key in required {
                assert!(example.get(key.as_str().unwrap()).is_some());
            }
        }

        // Exercise the displayed edits against their documented prior content.
        let evidence: Vec<Value> = (0..12)
            .map(|i| {
                json!({
                    "segment_id":format!("example-{i}"), "source":"microphone",
                    "start_ms":if i == 0 { 20000 } else { 10000 },
                    "end_ms":if i == 0 { 30000 } else { 20000 }
                })
            })
            .collect();
        let input = json!({"candidate":{"start_ms":20000,"end_ms":30000},
            "observations":[{"evidence_index":0}], "evidence":evidence});
        let mut prior = json!({"start_ms":10000,"end_ms":20000,
        "summary":"甲主张扩容，乙担心闲置，尚未决定。",
        "understanding":{"topics":[],"mentioned_objects":[],"key_points":[
            {"text":"甲主张扩容","evidence_indices":[10]},
            {"text":"乙担心扩容后闲置","evidence_indices":[10]},
            {"text":"尚未决定是否扩容","evidence_indices":[10]}
        ]}});
        let result: Value = serde_json::from_str(
            &understanding::normalize(
                &json!({"topics":[],"key_points":[point],"mentioned_objects":[]}),
                &input,
                Some(&prior),
                10000,
                30000,
                1,
            )
            .unwrap(),
        )
        .unwrap();
        let texts: Vec<&str> = result["key_points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| point["text"].as_str().unwrap())
            .collect();
        assert_eq!(
            texts,
            ["甲主张扩容", "乙担心扩容后闲置", "现决定暂不扩容。"]
        );
        assert_eq!(
            summary::apply(&json!([update]), Some(&prior), &input, 10000, 30000, 1).as_deref(),
            Some("甲主张扩容，乙担心闲置，现决定暂不扩容。")
        );

        prior["summary"] = json!(
            "当前已经实际完成的处理量是70单。 讨论中假设达到100单，那样才会与头部持平，但现在还未达到。"
        );
        prior["understanding"]["key_points"] = json!([
            {"text":"当前处理量70单。","evidence_indices":[10]},
            {"text":"100单是假设比较值。","evidence_indices":[11]}
        ]);
        assert_eq!(
            summary::apply(&json!([compress]), Some(&prior), &input, 10000, 30000, 1).as_deref(),
            Some("实有70单；100单与头部持平仍是假设。")
        );
    }

    #[test]
    fn screenshot_migration_preserves_existing_cloud_tasks_references_and_views() {
        let db = Connection::open_in_memory().unwrap();
        // Seed exactly the released schema before opening it with the new code.
        db.execute_batch(include_str!("../tests/cloud_tasks_v1.sql"))
            .unwrap();
        db.execute_batch("CREATE TABLE reference(task_id TEXT REFERENCES cloud_tasks(id));
          INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,state,created_ms,updated_ms)
          VALUES('old','asr','event','e','1','bigmodel','v1',1,2,'submitted',1,1);
          INSERT INTO reference VALUES('old');
          CREATE VIEW saved_view AS SELECT id FROM cloud_tasks;
          PRAGMA foreign_keys=ON;").unwrap();
        init_cloud_tasks(&db).unwrap();
        init_cloud_tasks(&db).unwrap();
        assert_eq!(
            db.query_row::<String, _, _>("SELECT id FROM saved_view", [], |r| r.get(0))
                .unwrap(),
            "old"
        );
        assert_eq!(
            db.query_row::<String, _, _>(
                "SELECT t.state FROM reference r JOIN cloud_tasks t ON r.task_id=t.id",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            "submitted"
        );
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get(0))
                .unwrap(),
            0
        );
        assert!(
            !db.pragma_query_value::<bool, _>(None, "legacy_alter_table", |r| r.get(0))
                .unwrap()
        );
    }

    #[test]
    fn summary_migration_is_idempotent_and_does_not_requeue_legacy_events() {
        let (mut db, task) = summary_fixture();
        publish_fit(
            &mut db,
            &task,
            summary_result(&task, serde_json::Value::Null),
            100_001,
        )
        .unwrap();
        crate::context::remove_index_for_legacy_fixture(&db);
        db.execute_batch("ALTER TABLE named_events DROP COLUMN summary; DELETE FROM core_migrations WHERE name='event_summary_v1'").unwrap();
        init(&db).unwrap();
        init(&db).unwrap();
        let items = timeline(&db, 0, 100_000).unwrap();
        assert!(items[0].summary.is_none());
        assert_eq!(refresh_candidates(&mut db, 100_002, true).unwrap(), 0);
        assert!(
            take_task(&mut db, CloudProvider::Llm, 100_002)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn summary_roundtrip_changes_without_requeuing_asr_and_invalidates_on_boundary_edit() {
        let (mut db, task) = summary_fixture();
        publish_fit(
            &mut db,
            &task,
            summary_result(&task, serde_json::json!(" 比较布局，\n保留色块。 ")),
            100_001,
        )
        .unwrap();
        let item = timeline(&db, 0, 100_000).unwrap().remove(0);
        let id = item.id.unwrap();
        assert_eq!(item.summary.as_deref(), Some("比较布局， 保留色块。"));
        assert_eq!(event_detail(&db, id, 0, 10).unwrap().summary, item.summary);
        assert_ne!(item.preview.unwrap().text, item.summary.clone().unwrap());
        let task_count: i64 = db
            .query_row("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
            .unwrap();
        // A recomputation may refine the summary while keeping title and boundaries identical.
        publish_fit(
            &mut db,
            &task,
            summary_result(&task, serde_json::json!("比较页面方案，倾向保留色块。")),
            100_002,
        )
        .unwrap();
        let detail = event_detail(&db, id, 0, 10).unwrap();
        assert_eq!(
            detail.summary.as_deref(),
            Some("比较页面方案，倾向保留色块。")
        );
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM cloud_tasks", [], |r| r.get(0))
                .unwrap(),
            task_count
        );
        edit_event(
            &mut db,
            id,
            detail.revision.unwrap(),
            1_000,
            10_000,
            "我的标题",
            None,
            100_003,
        )
        .unwrap();
        let detail = event_detail(&db, id, 0, 10).unwrap();
        assert!(detail.summary.is_some());
        edit_event(
            &mut db,
            id,
            detail.revision.unwrap(),
            2_000,
            10_000,
            "我的标题",
            None,
            100_004,
        )
        .unwrap();
        assert!(event_detail(&db, id, 0, 10).unwrap().summary.is_none());
        publish_fit(
            &mut db,
            &task,
            summary_result(&task, serde_json::json!("旧边界的内容")),
            100_005,
        )
        .unwrap();
        assert!(event_detail(&db, id, 0, 10).unwrap().summary.is_none());
    }

    #[test]
    fn invalid_summary_does_not_discard_the_event() {
        for summary in [
            serde_json::Value::Null,
            serde_json::json!(" "),
            serde_json::json!("bad\0text"),
            serde_json::json!({"unexpected":"object"}),
            serde_json::json!(42),
        ] {
            let (mut db, task) = summary_fixture();
            publish_fit(&mut db, &task, summary_result(&task, summary), 100_001).unwrap();
            let detail = timeline(&db, 0, 100_000).unwrap();
            assert_eq!(detail.iter().filter(|item| item.id.is_some()).count(), 1);
            assert!(detail[0].summary.is_none());
        }
    }

    #[test]
    fn summary_late_response_does_not_restore_invalidated_context() {
        let (mut db, task) = summary_fixture();
        publish_fit(
            &mut db,
            &task,
            summary_result(&task, serde_json::json!("旧摘要")),
            100_001,
        )
        .unwrap();
        let input = llm_input(&db, &task).unwrap();
        invalidate_event_understanding(&db, 2_000, 3_000, 1).unwrap();
        publish_fit_with_input(
            &mut db,
            &task,
            &input,
            summary_result(&task, serde_json::json!("带有已删除内容的摘要")),
            100_002,
        )
        .unwrap();
        assert!(timeline(&db, 0, 100_000).unwrap()[0].summary.is_none());
    }

    #[test]
    fn cloud_fairness_and_persistent_cooldown_preserve_paid_identity_and_pause() {
        let mut db = db();
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let mut ids = Vec::new();
        for (start, manual) in [(100, 0), (200, 1), (300, 1), (400, 1)] {
            let id = Uuid::new_v4();
            ids.push(id);
            db.execute("INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,
                started_ms,ended_ms,manual,state,created_ms,updated_ms)
                VALUES(?1,'asr','range',?1,'1','bigmodel','v',?2,?2+50,?3,'pending',1,1)", params![id.to_string(),start,manual]).unwrap();
        }
        let first = take_task_fair(&mut db, CloudProvider::Asr, 1000, 0)
            .unwrap()
            .unwrap();
        assert_eq!(first.id, ids[1]);
        assert_eq!(
            take_task_fair(&mut db, CloudProvider::Asr, 1000, 1)
                .unwrap()
                .unwrap()
                .id,
            ids[3]
        );
        assert_eq!(
            take_task_fair(&mut db, CloudProvider::Asr, 1000, 2)
                .unwrap()
                .unwrap()
                .id,
            ids[2]
        );
        assert_eq!(
            take_task_fair(&mut db, CloudProvider::Asr, 1000, 3)
                .unwrap()
                .unwrap()
                .id,
            ids[0]
        );
        mark_submit_intent(&db, &first, 1001).unwrap();
        task_failed_with_retry(&db, &first, "provider_http_429", Some(600_000), 1002).unwrap();
        recover_tasks(&mut db, 1003).unwrap();
        assert!(
            take_task_fair(&mut db, CloudProvider::Asr, 500_000, 0)
                .unwrap()
                .is_none()
        );
        db.execute(
            "UPDATE cloud_tasks SET next_attempt_ms=9999999 WHERE id!=?1",
            [first.id.to_string()],
        )
        .unwrap();
        db.execute("UPDATE cloud_queues SET paused=1 WHERE provider='asr'", [])
            .unwrap();
        assert!(
            take_task_fair(&mut db, CloudProvider::Asr, 601_003, 0)
                .unwrap()
                .is_none()
        );
        db.execute("UPDATE cloud_queues SET paused=0 WHERE provider='asr'", [])
            .unwrap();
        let retry = take_task_fair(&mut db, CloudProvider::Asr, 601_003, 0)
            .unwrap()
            .unwrap();
        assert_eq!(retry.remote_id, first.remote_id);
        assert_eq!(retry.submit_attempts, 1);
    }

    fn coarse(db: &Connection, _id: &str, start: i64, end: i64, text: &str) {
        db.execute("INSERT INTO coarse_segments VALUES (?1,'block','session','microphone',?2,?3,?4,'test')",
            params![Uuid::new_v4().to_string(),start,end,text]).unwrap();
    }

    fn media(db: &Connection, id: &str, start: i64, end: i64) {
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
            frames,bytes,media_path,retention_decision,analysis_state,speech_active)
            VALUES (?1,'session','microphone',?2,?3,0,480000,100,'media/microphone/example.opus',
            'retain','complete',0)",
            params![id, start, end],
        )
        .unwrap();
    }

    #[test]
    fn coverage_clips_unions_and_keeps_sources_and_cleanup_independent() {
        let db = db();
        db.execute_batch(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES
            ('a','microphone',0,1393,'machine_sleep'),
            ('b','microphone',1393,4676,'source_start_gap'),
            ('duplicate','microphone',0,1393,'machine_sleep'),
            ('overlap','microphone',1000,2000,'source_failure'),
            ('other','system',0,10000,'manual_pause'),
            ('open','microphone',8000,NULL,'source_failure');
            INSERT INTO clear_ranges VALUES('clear',6000,7000,1,'raw_audio',11000);",
        )
        .unwrap();
        let c = audio_coverage(&db, 1000, 9000, 1).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].duration_ms, 5676);
        assert_eq!(c[0].interval_count, 3);
        assert!(c[0].reasons.contains(&"cleared_audio".into()));
        assert!(!c[0].reasons.contains(&"manual_pause".into()));
        assert_eq!(
            audio_coverage(&db, 1000, 9000, 2).unwrap()[0].duration_ms,
            8000
        );
        assert!(audio_coverage(&db, 10000, 11000, 2).unwrap().is_empty());
        // Whole-day cleanup must not claim later, newly recorded audio was deleted.
        db.execute(
            "INSERT INTO clear_ranges VALUES('future',0,20000,2,'raw_audio',10000)",
            [],
        )
        .unwrap();
        assert!(audio_coverage(&db, 10000, 11000, 2).unwrap().is_empty());
    }

    #[test]
    fn historical_sleep_gaps_do_not_override_completed_asr() {
        let mut db = db();
        media(&db, "saved", 1000, 6000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let id = create_event(
            &mut db,
            1000,
            6000,
            "synthetic",
            EventSources::Microphone,
            7000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 7001)
            .unwrap()
            .unwrap();
        publish_asr(
            &mut db,
            &task,
            &[crate::cloud::AsrUtterance {
                words: None,
                start_ms: 1000,
                end_ms: 2000,
                text: "synthetic".into(),
                speaker: None,
            }],
            7002,
        )
        .unwrap();
        db.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES('sleep','microphone',3000,4393,'machine_sleep')",[]).unwrap();
        let detail = event_detail(&db, id, 0, 100).unwrap();
        assert_eq!(detail.processing_state, "transcribed");
        assert_eq!(detail.audio_coverage[0].duration_ms, 1393);
        let list = timeline(&db, 0, 10000).unwrap();
        let item = list.iter().find(|i| i.id == Some(id)).unwrap();
        assert_eq!(item.processing_state, "transcribed");
        assert_eq!(item.audio_coverage[0].duration_ms, 1393);
    }

    #[test]
    fn older_failed_cloud_task_remains_reachable_after_many_completions() {
        let db = db();
        let failed = Uuid::new_v4();
        for index in 0..60 {
            let id = if index == 0 { failed } else { Uuid::new_v4() };
            db.execute(
                "INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,
                 model_id,processor_version,started_ms,ended_ms,source,state,error_code,
                 created_ms,updated_ms)
                 VALUES (?1,'asr','range',?2,'1','test','1',?3,?4,'microphone',?5,?6,?3,?3)",
                params![
                    id.to_string(),
                    id.to_string(),
                    index * 2_000,
                    index * 2_000 + 1_000,
                    if index == 0 { "failed" } else { "complete" },
                    if index == 0 {
                        Some("provider_failed")
                    } else {
                        None
                    },
                ],
            )
            .unwrap();
        }
        let visible = task_views(&db, CloudProvider::Asr, 50).unwrap();
        assert_eq!(visible.len(), 50);
        assert_eq!(visible[0].id, failed);
        assert_eq!(visible[0].state, "failed");
    }

    #[test]
    fn cloud_ogg_keeps_real_time_gaps_and_exact_duration() {
        let directory = tempfile::tempdir().unwrap();
        let media_directory = directory.path().join("media/microphone");
        let mut parts = Vec::new();
        for (start, end) in [(0, 1_000), (2_000, 3_000)] {
            let id = Uuid::new_v4();
            let mut block = crate::media::OpusBlock::create(&media_directory, id).unwrap();
            let pcm: Vec<f32> = (0..48_000)
                .map(|sample| {
                    (sample as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.2
                })
                .collect();
            block.push(&pcm).unwrap();
            block.seal().unwrap();
            parts.push(AudioPart {
                start,
                end,
                frames: 48_000,
                path: PathBuf::from(format!("media/microphone/{id}.opus")),
            });
        }
        let task = CloudTask {
            id: Uuid::new_v4(),
            provider: CloudProvider::Asr,
            target_kind: "event".to_owned(),
            target_id: String::new(),
            target_version: String::new(),
            model_id: String::new(),
            start: 0,
            end: 3_000,
            source: Some("microphone".to_owned()),
            remote_id: Uuid::new_v4(),
            attempts: 0,
            submit_attempts: 0,
            updated_ms: 0,
        };
        let ogg = assemble_ogg(directory.path(), &task, &parts).unwrap();
        let output = directory.path().join("cloud.opus");
        std::fs::write(&output, ogg).unwrap();
        let decoded = crate::media::decode_block(&output, 144_000).unwrap();
        let peak = |samples: &[f32]| samples.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(peak(&decoded[4_800..43_200]) > 0.05);
        assert!(peak(&decoded[100_800..139_200]) > 0.05);
        assert!(peak(&decoded[57_600..86_400]) < 0.005);
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".asr-ogg-")
        }));
    }

    #[test]
    fn recovery_removes_only_private_crashed_asr_staging() {
        let directory = tempfile::tempdir().unwrap();
        let stale = directory.path().join(".asr-ogg-ABCdef123456");
        let unrelated = directory.path().join(".asr-ogg-user-notes");
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join("audio.pending"), b"private").unwrap();
        std::fs::create_dir(&unrelated).unwrap();
        recover_asr_staging(directory.path());
        assert!(!stale.exists());
        assert!(unrelated.exists());
    }

    fn confirmed_silence_block(db: &Connection, start: i64, end: i64) {
        db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,retention_decision,analysis_state,speech_active) VALUES(?1,'session','microphone',?2,?3,0,?4,0,'discard_no_speech_v2','complete',0)",params![Uuid::new_v4().to_string(),start,end,(end-start)*48]).unwrap();
    }

    #[test]
    fn short_cloud_speaker_split_is_unknown_until_confirmed() {
        let mut db = db();
        for (id, start, end) in [
            ("a1", 1_000, 11_000),
            ("a2", 12_000, 22_000),
            ("b1", 23_000, 33_000),
            ("b2", 34_000, 44_000),
            ("tiny", 45_000, 47_000),
        ] {
            media(&db, id, start, end);
        }
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        for start in [11_000, 22_000, 33_000, 44_000] {
            confirmed_silence_block(&db, start, start + 1000);
        }
        let event = create_event(
            &mut db,
            1_000,
            47_000,
            "会话",
            EventSources::Microphone,
            50_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 50_001)
            .unwrap()
            .unwrap();
        let utterances = [
            (0, 10_000, "1"),
            (11_000, 21_000, "1"),
            (22_000, 32_000, "2"),
            (33_000, 43_000, "2"),
            (44_000, 46_000, "3"),
        ]
        .into_iter()
        .map(|(start_ms, end_ms, speaker)| crate::cloud::AsrUtterance {
            words: None,
            start_ms,
            end_ms,
            text: "你好".to_owned(),
            speaker: Some(speaker.to_owned()),
        })
        .collect::<Vec<_>>();
        publish_asr(&mut db, &task, &utterances, 50_002).unwrap();
        let detail = event_detail(&db, event, 0, 100).unwrap();
        assert_eq!(detail.precise_segments.len(), 5);
        assert!(
            detail.precise_segments[..4]
                .iter()
                .all(|s| s.speaker.is_some())
        );
        assert_eq!(detail.precise_segments[4].speaker, None);
        assert_ne!(detail.precise_segments[4].speaker_revision, 0);
        let unknown = detail.precise_segments[4].id;
        let person = {
            let tx = db.transaction().unwrap();
            let person = crate::knowledge::create_person(&tx, "小林", 50_003).unwrap();
            tx.commit().unwrap();
            person
        };
        db.execute(
            "INSERT INTO speaker_bindings
             (id,original_segment_id,current_segment_id,person_id,source,started_ms,ended_ms,
              origin,apply_scope,state,created_ms,updated_ms)
             VALUES (?1,?2,?2,?3,'microphone',45000,47000,'automatic','group','active',50004,50004)",
            params![Uuid::new_v4().to_string(), unknown.to_string(), person.to_string()],
        )
        .unwrap();
        let detail = event_detail(&db, event, 0, 100).unwrap();
        let short = &detail.precise_segments[4];
        assert_eq!(short.speaker, None);
        assert_eq!(short.person_id, None);
        assert_eq!(short.person_name, None);
        assert!(
            crate::knowledge::search(&db, Some("你好"), 0, 50_000, Some(person), 0, 100)
                .unwrap()
                .is_empty()
        );
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("identity.tar");
        crate::archive::export(
            &db,
            directory.path(),
            0,
            50_000,
            EventSources::Microphone,
            false,
            false,
            &destination,
            &std::sync::Mutex::new(()),
        )
        .unwrap();
        let mut archive = tar::Archive::new(std::fs::File::open(destination).unwrap());
        let mut exported = String::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            if entry.path().unwrap().as_ref() == std::path::Path::new("precise.jsonl") {
                use std::io::Read;
                entry.read_to_string(&mut exported).unwrap();
            }
        }
        let short_export: serde_json::Value = exported
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|value| value["id"] == unknown.to_string())
            .unwrap();
        assert!(short_export["speaker"].is_null());
        assert!(short_export["person_id"].is_null());
        let tx = db.transaction().unwrap();
        crate::knowledge::expect_revision(&tx, "speaker_group", unknown, short.speaker_revision)
            .unwrap();
        crate::knowledge::bind_speaker(&tx, unknown, person, true, 50_005).unwrap();
        tx.commit().unwrap();
        let detail = event_detail(&db, event, 0, 100).unwrap();
        assert_eq!(detail.precise_segments[4].speaker, None);
        assert_eq!(
            detail.precise_segments[4].person_name.as_deref(),
            Some("小林")
        );
    }

    #[test]
    fn normalized_response_keeps_request_bounds_despite_late_gap_metadata() {
        let mut db = db();
        media(&db, "saved", 1_000, 11_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        create_event(
            &mut db,
            1_000,
            11_000,
            "对话",
            EventSources::Microphone,
            12_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 12_001)
            .unwrap()
            .unwrap();
        let response = serde_json::json!({"result":{"utterances":[{
            "start_time":0,"end_time":20_000,"text":"开始后来",
            "words":[{"start_time":100,"end_time":500,"text":"开始"},
                {"start_time":9500,"end_time":9900,"text":"后来"}]
        }]}});
        let items = crate::cloud::parse_asr_result(&response).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].start_ms, items[1].end_ms), (0, 20_000));
        assert_eq!(
            publish_asr(&mut db, &task, &items, 12_002).unwrap_err(),
            "provider_invalid_timing"
        );
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM precise_segments", [], |r| r.get(0))
                .unwrap(),
            0
        );
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES ('gap','microphone',1000,1050,'source_failure')",
            [],
        )
        .unwrap();
        let mut valid = response;
        valid["result"]["utterances"][0]["end_time"] = serde_json::json!(10_000);
        let items = crate::cloud::parse_asr_result(&valid).unwrap();
        assert_eq!(publish_asr(&mut db, &task, &items, 12_003).unwrap(), 2);
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM precise_segments", [], |r| r.get(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn asr_windows_cut_long_silence_but_keep_short_pauses_and_real_origins() {
        let mut db = db();
        media(&db, "first", 1_000, 6_000);
        media(&db, "short", 9_000, 14_000);
        confirmed_silence_block(&db, 6_000, 9_000);
        media(&db, "long", 17_001, 22_001);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event = create_event(
            &mut db,
            1_000,
            22_001,
            "对话",
            EventSources::Microphone,
            23_000,
        )
        .unwrap();
        let spans = {
            let mut statement=db.prepare("SELECT started_ms,ended_ms FROM cloud_tasks WHERE target_id=?1 ORDER BY started_ms").unwrap();
            statement
                .query_map([event.to_string()], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(spans, [(1_000, 14_000), (17_001, 22_001)]);
        let first = take_task(&mut db, CloudProvider::Asr, 23_001)
            .unwrap()
            .unwrap();
        publish_asr(
            &mut db,
            &first,
            &[crate::cloud::AsrUtterance {
                words: None,
                start_ms: 1_000,
                end_ms: 2_000,
                text: "第一句".into(),
                speaker: None,
            }],
            23_002,
        )
        .unwrap();
        let second = take_task(&mut db, CloudProvider::Asr, 23_003)
            .unwrap()
            .unwrap();
        publish_asr(
            &mut db,
            &second,
            &[crate::cloud::AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 1_000,
                text: "第二句".into(),
                speaker: None,
            }],
            23_004,
        )
        .unwrap();
        let detail = event_detail(&db, event, 0, 100).unwrap();
        assert_eq!(detail.processing_state, "transcribed");
        assert_eq!(
            detail
                .precise_segments
                .iter()
                .map(|p| (p.started_ms, p.ended_ms))
                .collect::<Vec<_>>(),
            [(2_000, 3_000), (17_501, 18_001)]
        );
    }

    #[test]
    fn cloud_text_survives_encoded_silence_while_voice_requires_retained_audio() {
        let mut db = db();
        media(&db, "first", 1_000, 6_000);
        media(&db, "second", 7_000, 11_000);
        confirmed_silence_block(&db, 6_000, 7_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event = create_event(
            &mut db,
            1_000,
            11_000,
            "对话",
            EventSources::Microphone,
            12_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 12_001)
            .unwrap()
            .unwrap();
        let sentence = crate::cloud::AsrUtterance {
            words: None,
            start_ms: 3_000,
            end_ms: 7_000,
            text: "跨过一句短暂停顿".to_owned(),
            speaker: Some("1".to_owned()),
        };
        publish_asr(&mut db, &task, &[sentence], 12_002).unwrap();
        assert_eq!(
            event_detail(&db, event, 0, 10)
                .unwrap()
                .precise_segments
                .len(),
            1
        );
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES ('failure','microphone',6000,7000,'source_failure')",
            [],
        )
        .unwrap();
        assert!(voice_manifest(&db, "microphone", 4_000, 8_000).is_err());
    }

    #[test]
    fn explicit_event_rerun_replaces_projection_and_restores_exact_correction() {
        let mut db = db();
        media(&db, "saved", 1_000, 6_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event = create_event(
            &mut db,
            1_000,
            6_000,
            "会话",
            EventSources::Microphone,
            7_000,
        )
        .unwrap();
        let first = take_task(&mut db, CloudProvider::Asr, 7_001)
            .unwrap()
            .unwrap();
        let utterance = crate::cloud::AsrUtterance {
            words: None,
            start_ms: 0,
            end_ms: 3_000,
            text: "原文".to_owned(),
            speaker: Some("1".to_owned()),
        };
        publish_asr(&mut db, &first, &[utterance.clone()], 7_002).unwrap();
        let first_segment = event_detail(&db, event, 0, 10).unwrap().precise_segments[0].id;
        let tx = db.transaction().unwrap();
        crate::knowledge::correct_text(&tx, first_segment, "人工修正", 7_003).unwrap();
        let preview = timeline(&tx, 0, 10_000)
            .unwrap()
            .into_iter()
            .find(|item| item.id == Some(event))
            .unwrap()
            .preview
            .unwrap();
        assert_eq!(preview.text, "人工修正");
        assert!(preview.is_precise);
        retranscribe_event_in(&tx, event, 1, 7_004).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            event_detail(&db, event, 0, 10)
                .unwrap()
                .precise_segments
                .len(),
            0
        );
        assert_eq!(
            event_detail(&db, event, 0, 10).unwrap().processing_state,
            "pending_asr"
        );
        assert_eq!(
            publish_asr(&mut db, &first, &[utterance.clone()], 7_005).unwrap(),
            0
        );
        assert!(
            timeline(&db, 0, 10_000)
                .unwrap()
                .into_iter()
                .find(|item| item.id == Some(event))
                .unwrap()
                .preview
                .is_none()
        );
        let second = take_task(&mut db, CloudProvider::Asr, 7_006)
            .unwrap()
            .unwrap();
        assert_ne!(first.id, second.id);
        publish_asr(&mut db, &second, &[utterance.clone()], 7_007).unwrap();
        let detail = event_detail(&db, event, 0, 10).unwrap();
        assert_eq!(detail.precise_segments.len(), 1);
        assert_eq!(detail.precise_segments[0].text, "人工修正");
        assert!(detail.precise_segments[0].corrected);
        let tx = db.transaction().unwrap();
        retranscribe_event_in(&tx, event, 1, 7_008).unwrap();
        tx.commit().unwrap();
        let third = take_task(&mut db, CloudProvider::Asr, 7_009)
            .unwrap()
            .unwrap();
        assert_ne!(second.id, third.id);
        publish_asr(&mut db, &third, &[utterance], 7_010).unwrap();
        assert_eq!(
            event_detail(&db, event, 0, 10)
                .unwrap()
                .precise_segments
                .len(),
            1
        );
    }

    #[test]
    fn manual_fine_range_splits_unorganized_projection_without_moving_audio() {
        let mut db = db();
        let hour = 3_600_000;
        coarse(&db, "one", 15 * 60_000, 16 * 60_000, "一句话");
        coarse(&db, "two", 16 * 60_000, 17 * 60_000, "接着说");
        coarse(
            &db,
            "three",
            hour + 10 * 60_000,
            hour + 11 * 60_000,
            "另一件事",
        );
        media(&db, "day", 0, 3 * hour);
        assert_eq!(refresh_candidates(&mut db, 3 * hour, true).unwrap(), 2);
        let count: i64 = db
            .query_row("SELECT count(*) FROM candidate_windows", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let items = timeline(&db, 0, 3 * hour).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].title.is_none());
        assert_eq!((items[0].started_ms, items[0].ended_ms), (0, 3 * hour));

        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        transcribe_range(
            &mut db,
            15 * 60_000,
            16 * 60_000,
            EventSources::Microphone,
            3 * hour + 1,
        )
        .unwrap();
        let items = timeline(&db, 0, 3 * hour).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!((items[2].started_ms, items[2].ended_ms), (0, 15 * 60_000));
        assert_eq!(
            (items[1].started_ms, items[1].ended_ms),
            (15 * 60_000, 16 * 60_000)
        );
        assert!(items[1].id.is_none());
        assert!(items[1].manual_range_id.is_some());
        assert_eq!(items[1].processing_state, "pending_asr");
        assert_eq!(
            (items[0].started_ms, items[0].ended_ms),
            (16 * 60_000, 3 * hour)
        );
        assert!(items[0].manual_range_id.is_none());
        let detail = range_detail(
            &db,
            15 * 60_000,
            16 * 60_000,
            EventSources::Microphone,
            0,
            100,
        )
        .unwrap();
        assert_eq!(detail.manual_range_id, items[1].manual_range_id);
        assert_eq!(detail.processing_state, "pending_asr");
    }

    #[test]
    fn named_event_takes_priority_over_overlapping_manual_fine_range() {
        let mut db = db();
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        media(&db, "day", 0, 3_600_000);
        let range_id = transcribe_range(
            &mut db,
            600_000,
            2_400_000,
            EventSources::Microphone,
            3_600_001,
        )
        .unwrap();
        let event_id = create_event(
            &mut db,
            1_200_000,
            1_800_000,
            "会议",
            EventSources::Microphone,
            3_600_002,
        )
        .unwrap();
        let items = timeline(&db, 0, 3_600_000).unwrap();
        let mut chronological = items.clone();
        chronological.reverse();
        assert_eq!(chronological.len(), 5);
        assert_eq!(
            (chronological[0].started_ms, chronological[0].ended_ms),
            (0, 600_000)
        );
        assert_eq!(chronological[1].manual_range_id, Some(range_id));
        assert_eq!(
            (chronological[1].started_ms, chronological[1].ended_ms),
            (600_000, 1_200_000)
        );
        assert_eq!(chronological[2].id, Some(event_id));
        assert_eq!(
            (chronological[2].started_ms, chronological[2].ended_ms),
            (1_200_000, 1_800_000)
        );
        assert_eq!(chronological[3].manual_range_id, Some(range_id));
        assert_eq!(
            (chronological[3].started_ms, chronological[3].ended_ms),
            (1_800_000, 2_400_000)
        );
        assert_eq!(
            (chronological[4].started_ms, chronological[4].ended_ms),
            (2_400_000, 3_600_000)
        );
    }

    #[test]
    fn newer_manual_selection_replaces_only_its_overlapping_interval() {
        let mut db = db();
        media(&db, "day", 1_000, 60_000);
        let outer =
            transcribe_range(&mut db, 10_000, 50_000, EventSources::Microphone, 60_001).unwrap();
        let inner =
            transcribe_range(&mut db, 20_000, 30_000, EventSources::Microphone, 60_002).unwrap();
        let mut items = timeline(&db, 0, 60_000).unwrap();
        items.reverse();
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].manual_range_id, None);
        assert_eq!((items[1].started_ms, items[1].ended_ms), (10_000, 20_000));
        assert_eq!(items[1].manual_range_id, Some(outer));
        assert_eq!((items[2].started_ms, items[2].ended_ms), (20_000, 30_000));
        assert_eq!(items[2].manual_range_id, Some(inner));
        assert_eq!((items[3].started_ms, items[3].ended_ms), (30_000, 50_000));
        assert_eq!(items[3].manual_range_id, Some(outer));
        assert_eq!(items[4].manual_range_id, None);
    }

    #[test]
    fn cleared_manual_range_without_material_does_not_leave_an_empty_event() {
        let mut db = db();
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        media(&db, "day", 1_000, 60_000);
        transcribe_range(&mut db, 10_000, 20_000, EventSources::Microphone, 60_001).unwrap();
        db.execute("DELETE FROM audio_blocks", []).unwrap();
        assert!(timeline(&db, 0, 60_000).unwrap().is_empty());
        let detail = range_detail(&db, 10_000, 20_000, EventSources::Microphone, 0, 100).unwrap();
        assert!(detail.manual_range_id.is_none());
    }

    #[test]
    fn manual_range_transcribes_only_the_selected_source() {
        let mut db = db();
        media(&db, "mic", 1_000, 6_000);
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
            frames,bytes,media_path,retention_decision,analysis_state,speech_active)
            VALUES ('system','session','system',1000,6000,0,240000,100,
            'media/system/example.opus','retain','complete',0)",
            [],
        )
        .unwrap();
        let range =
            transcribe_range(&mut db, 1_000, 6_000, EventSources::Microphone, 7_000).unwrap();
        let sources: Vec<String> = {
            let mut statement = db
                .prepare("SELECT source FROM cloud_tasks WHERE target_id=?1")
                .unwrap();
            statement
                .query_map(params![range.to_string()], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(sources, ["microphone"]);
        let items = timeline(&db, 0, 7_000).unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            items.iter().any(|item| item.manual_range_id == Some(range)
                && item.sources == EventSources::Microphone)
        );
        assert!(
            items
                .iter()
                .any(|item| item.manual_range_id.is_none() && item.sources == EventSources::System)
        );
    }

    #[test]
    fn manual_range_receipt_lists_real_tasks_and_current_states() {
        let mut db = db();
        media(&db, "mic-first", 1_000, 6_000);
        media(&db, "mic-later", 930_000, 935_000);
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
             frames,bytes,media_path,retention_decision,analysis_state,speech_active)
             VALUES ('system','session','system',1000,6000,0,240000,100,
             'media/system/example.opus','retain','complete',0)",
            [],
        )
        .unwrap();
        let tx = db.transaction().unwrap();
        let (range_id, tasks) =
            transcribe_range_in(&tx, 1_000, 935_000, EventSources::Both, 940_000).unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(
            tasks
                .iter()
                .filter(|task| task.source == Some(AudioSource::Microphone))
                .count(),
            2
        );
        assert_eq!(
            tasks
                .iter()
                .filter(|task| task.source == Some(AudioSource::System))
                .count(),
            1
        );
        assert!(
            tasks
                .iter()
                .all(|task| task.target_id == range_id.to_string()
                    && task.state == "pending"
                    && task.manual)
        );
        tx.commit().unwrap();

        let first_id = tasks[0].id;
        db.execute(
            "UPDATE cloud_tasks SET state='processing' WHERE id=?1",
            params![first_id.to_string()],
        )
        .unwrap();
        let tx = db.transaction().unwrap();
        let (again_id, again_tasks) =
            transcribe_range_in(&tx, 1_000, 935_000, EventSources::Both, 941_000).unwrap();
        assert_eq!(again_id, range_id);
        assert_eq!(
            again_tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
            tasks.iter().map(|task| task.id).collect::<Vec<_>>()
        );
        assert_eq!(again_tasks[0].state, "processing");
        tx.commit().unwrap();
    }

    #[test]
    fn empty_manual_range_does_not_leave_a_false_queue_entry() {
        let mut db = db();
        let tx = db.transaction().unwrap();
        assert_eq!(
            transcribe_range_in(&tx, 1_000, 6_000, EventSources::Both, 7_000).unwrap_err(),
            "no retained audio in selected range"
        );
        tx.rollback().unwrap();
        let ranges: i64 = db
            .query_row("SELECT count(*) FROM manual_ranges", [], |row| row.get(0))
            .unwrap();
        let tasks: i64 = db
            .query_row("SELECT count(*) FROM cloud_tasks", [], |row| row.get(0))
            .unwrap();
        assert_eq!((ranges, tasks), (0, 0));
    }

    #[test]
    fn repeating_failed_manual_range_preserves_explicit_retry_identity() {
        let mut db = db();
        media(&db, "saved", 1_000, 6_000);
        let tx = db.transaction().unwrap();
        let (range_id, initial) =
            transcribe_range_in(&tx, 1_000, 6_000, EventSources::Microphone, 7_000).unwrap();
        tx.commit().unwrap();
        let task_id = initial[0].id;
        let remote_id = Uuid::new_v4().to_string();
        db.execute(
            "UPDATE cloud_tasks SET state='failed',error_code='provider_missing_submitted_task',
             submit_attempts=1,remote_id=?2 WHERE id=?1",
            params![task_id.to_string(), remote_id],
        )
        .unwrap();

        let tx = db.transaction().unwrap();
        let (again_id, tasks) =
            transcribe_range_in(&tx, 1_000, 6_000, EventSources::Microphone, 7_001).unwrap();
        assert_eq!(again_id, range_id);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task_id);
        assert_eq!(tasks[0].state, "failed");
        assert_eq!(
            tasks[0].error_code.as_deref(),
            Some("provider_missing_submitted_task")
        );
        tx.commit().unwrap();
        let persisted_remote: String = db
            .query_row(
                "SELECT remote_id FROM cloud_tasks WHERE id=?1",
                params![task_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_remote, remote_id);

        retry_task(&db, task_id, 7_002).unwrap();
        let retried: (String, String) = db
            .query_row(
                "SELECT state,remote_id FROM cloud_tasks WHERE id=?1",
                params![task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(retried.0, "submitted");
        assert_ne!(retried.1, remote_id);
    }

    #[test]
    fn manual_event_blocks_overlapping_late_model_proposal() {
        let mut db = db();
        let now = 10_000_000;
        coarse(&db, "speech", now - 100_000, now - 50_000, "我们开会");
        media(&db, "saved", now - 110_000, now - 40_000);
        refresh_candidates(&mut db, now, true).unwrap();
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        let task = take_task(&mut db, CloudProvider::Llm, now)
            .unwrap()
            .unwrap();
        let human = create_event(
            &mut db,
            now - 105_000,
            now - 45_000,
            "人工会议",
            EventSources::Both,
            now,
        )
        .unwrap();
        let fit = serde_json::json!({"events":[{"start_ms":now-100_000,"end_ms":now-50_000,
            "title":"模型猜测","sources":"microphone","transcribe":true,"merge_with_event_id":null}]});
        assert_eq!(publish_fit(&mut db, &task, fit, now).unwrap(), 0);
        let items = timeline(&db, now - 120_000, now).unwrap();
        assert_eq!(items.iter().filter(|i| i.id == Some(human)).count(), 1);
        assert!(items.iter().all(|i| i.title.as_deref() != Some("模型猜测")));
    }

    #[test]
    fn extended_event_accepts_old_result_and_only_queues_new_coverage() {
        let mut db = db();
        let now = 10_000_000;
        media(&db, "a", now - 100_000, now - 90_000);
        media(&db, "b", now - 80_000, now - 70_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let id = create_event(
            &mut db,
            now - 100_000,
            now - 90_000,
            "会话",
            EventSources::Both,
            now,
        )
        .unwrap();
        let old = take_task(&mut db, CloudProvider::Asr, now)
            .unwrap()
            .unwrap();
        edit_event(
            &mut db,
            id,
            1,
            now - 100_000,
            now - 70_000,
            "会话",
            None,
            now + 1,
        )
        .unwrap();
        let rows = publish_asr(
            &mut db,
            &old,
            &[crate::cloud::AsrUtterance {
                words: None,
                start_ms: 0,
                end_ms: 500,
                text: "旧结果".to_owned(),
                speaker: Some("1".to_owned()),
            }],
            now + 2,
        )
        .unwrap();
        assert_eq!(rows, 1);
        let count: i64 = db
            .query_row("SELECT count(*) FROM precise_segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let next = take_task(&mut db, CloudProvider::Asr, now + 3)
            .unwrap()
            .unwrap();
        assert_eq!((next.start, next.end), (now - 80_000, now - 70_000));
        assert_eq!(
            next.target_version,
            asr_version(3, now - 100_000, now - 70_000)
        );
    }

    #[test]
    fn queue_pause_does_not_cancel_other_provider() {
        let mut db = db();
        let now = 10_000_000;
        coarse(&db, "speech", now - 100_000, now - 90_000, "一起讨论");
        media(&db, "saved", now - 100_000, now - 90_000);
        refresh_candidates(&mut db, now, true).unwrap();
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let _ = create_event(
            &mut db,
            now - 100_000,
            now - 90_000,
            "手动会议",
            EventSources::Both,
            now,
        )
        .unwrap();
        set_paused(&db, CloudProvider::Llm, true).unwrap();
        assert!(
            take_task(&mut db, CloudProvider::Llm, now)
                .unwrap()
                .is_none()
        );
        assert!(
            take_task(&mut db, CloudProvider::Asr, now)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn claimed_candidate_returns_to_pending_when_paused_or_recovered() {
        let mut db = db();
        let now = 10_000_000;
        coarse(&db, "speech", now - 100_000, now - 90_000, "继续讨论");
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        refresh_candidates(&mut db, now, true).unwrap();
        let claimed = take_task(&mut db, CloudProvider::Llm, now)
            .unwrap()
            .unwrap();
        let candidate_id = claimed.target_id.clone();
        let candidate_state = |db: &Connection| -> String {
            db.query_row(
                "SELECT state FROM candidate_windows WHERE id=?1",
                params![candidate_id],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(candidate_state(&db), "processing");
        set_paused(&db, CloudProvider::Llm, true).unwrap();
        defer_paused(&mut db, &claimed, now + 1).unwrap();
        assert_eq!(candidate_state(&db), "pending");
        set_paused(&db, CloudProvider::Llm, false).unwrap();
        let claimed = take_task(&mut db, CloudProvider::Llm, now + 1_001)
            .unwrap()
            .unwrap();
        assert_eq!(candidate_state(&db), "processing");
        set_paused(&db, CloudProvider::Llm, true).unwrap();
        recover_tasks(&mut db, now + 1_002).unwrap();
        assert_eq!(candidate_state(&db), "pending");
        assert!(
            take_task(&mut db, CloudProvider::Llm, now + 1_002)
                .unwrap()
                .is_none()
        );
        assert!(task_current(&db, &claimed).unwrap());
    }

    #[test]
    fn late_coarse_text_updates_same_automatic_event_instead_of_duplicating_it() {
        let mut db = db();
        let now = 10_000_000;
        coarse(&db, "first", now - 120_000, now - 110_000, "开会开始");
        media(&db, "saved", now - 120_000, now - 100_000);
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        assert_eq!(refresh_candidates(&mut db, now, true).unwrap(), 1);
        let task = take_task(&mut db, CloudProvider::Llm, now)
            .unwrap()
            .unwrap();
        let fit = serde_json::json!({"events":[{"start_ms":now-120_000,"end_ms":now-110_000,
            "title":"讨论","sources":"microphone","transcribe":true,"merge_with_event_id":null}]});
        assert_eq!(publish_fit(&mut db, &task, fit, now).unwrap(), 1);
        let id: String = db
            .query_row("SELECT id FROM named_events", [], |r| r.get(0))
            .unwrap();
        coarse(&db, "later", now - 109_000, now - 100_000, "继续讨论");
        assert_eq!(refresh_candidates(&mut db, now + 1, true).unwrap(), 1);
        let updated = take_task(&mut db, CloudProvider::Llm, now + 1)
            .unwrap()
            .unwrap();
        assert_eq!(updated.target_id, task.target_id);
        let fit = serde_json::json!({"events":[{"start_ms":now-120_000,"end_ms":now-100_000,
            "title":"讨论","sources":"microphone","transcribe":true,"merge_with_event_id":null}]});
        assert_eq!(publish_fit(&mut db, &updated, fit, now + 2).unwrap(), 1);
        let rows: Vec<(String, i64)> = db
            .prepare("SELECT id,revision FROM named_events WHERE deleted_ms IS NULL")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![(id, 2)]);
    }

    #[test]
    fn fit_rejects_bad_ranges_atomically_instead_of_silent_partial_success() {
        for (ranges, expected_slots) in [
            (vec![(20_000, 30_000), (1_000, 10_000)], vec![0, 1]),
            (vec![(10_000, 30_000), (1_000, 10_000)], vec![0, 1]),
            (vec![(20_000, 30_000), (1_000, 25_000)], vec![]),
            (vec![(20_000, 30_001), (1_000, 10_000)], vec![]),
            (vec![(20_000, 30_000), (0, 10_000)], vec![]),
        ] {
            let mut db = db();
            coarse(&db, "speech-a", 1_000, 10_000, "第一个活动");
            coarse(&db, "speech-b", 20_000, 30_000, "第二个活动");
            media(&db, "saved", 1_000, 30_000);
            set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
            refresh_candidates(&mut db, 100_000, true).unwrap();
            let task = take_task(&mut db, CloudProvider::Llm, 100_000)
                .unwrap()
                .unwrap();
            let events: Vec<_> = ranges
                .iter()
                .map(|(start, end)| {
                    serde_json::json!({
                        "start_ms":start,"end_ms":end,"title":"讨论","sources":"microphone"
                    })
                })
                .collect();
            let result = publish_fit(
                &mut db,
                &task,
                serde_json::json!({"events":events}),
                100_001,
            );
            if expected_slots.is_empty() {
                assert!(result.unwrap_err().starts_with("invalid_fit_boundary_"));
            } else {
                assert_eq!(result.unwrap(), expected_slots.len());
            }
            let slots: Vec<i64> = db
                .prepare("SELECT slot FROM event_candidate_links ORDER BY slot")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(slots, expected_slots);
            assert_eq!(
                db.query_row::<String, _, _>("SELECT state FROM candidate_windows", [], |r| r
                    .get(0))
                    .unwrap(),
                if expected_slots.is_empty() {
                    "processing"
                } else {
                    "complete"
                }
            );
            assert_eq!(
                db.query_row::<i64, _, _>("SELECT count(*) FROM coarse_segments", [], |r| r.get(0))
                    .unwrap(),
                2
            );
        }
    }

    #[test]
    fn manual_event_without_saved_audio_remains_visible_and_reports_unavailable() {
        let mut db = db();
        let id = create_event(
            &mut db,
            10_000,
            20_000,
            "人工补记",
            EventSources::Both,
            30_000,
        )
        .unwrap();
        let items = timeline(&db, 0, 30_000).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, Some(id));
        assert_eq!(items[0].processing_state, "audio_unavailable");
    }

    #[test]
    fn invalid_fit_output_retries_are_bounded_and_keep_task_identity() {
        for code in [
            "provider_invalid_json_content",
            "provider_invalid_json_content_syntax",
            "invalid_fit_schema",
            "invalid_fit_boundary_range",
            "invalid_fit_boundary_overlap",
            "provider_invalid_json_content_data",
            "provider_incomplete_output",
        ] {
            let mut db = db();
            coarse(&db, "speech", 1000, 6000, "可整理的连续讨论");
            set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
            refresh_candidates(&mut db, 100_000, true).unwrap();
            let mut identity = None;
            for attempt in 1..=3 {
                let t = take_task(&mut db, CloudProvider::Llm, attempt * 1_000_000)
                    .unwrap()
                    .unwrap();
                assert!(identity.is_none_or(|id| id == t.id));
                identity = Some(t.id);
                task_failed(&db, &t, code, attempt * 1_000_000 + 1).unwrap();
                let (state, attempts): (String, i64) = db
                    .query_row(
                        "SELECT state,attempts FROM cloud_tasks WHERE id=?1",
                        [t.id.to_string()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(state, if attempt < 3 { "pending" } else { "failed" });
                assert_eq!(attempts, attempt);
                assert_eq!(
                    db.query_row(
                        "SELECT state FROM candidate_windows WHERE id=?1",
                        [&t.target_id],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    state
                );
            }
            assert!(
                take_task(&mut db, CloudProvider::Llm, 10_000_000)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM named_events", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn fit_input_intersects_crossing_observations_without_retiming_evidence() {
        let mut db = db();
        coarse(&db, "initial", 2000, 3000, "当前讨论");
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        refresh_candidates(&mut db, 100_000, true).unwrap();
        let task = take_task(&mut db, CloudProvider::Llm, 100_000)
            .unwrap()
            .unwrap();
        coarse(&db, "crossing", 1000, 4000, "跨越前后窗口的原始句子");
        let sent: serde_json::Value =
            serde_json::from_str(&llm_input(&db, &task).unwrap()).unwrap();
        let lines = sent["observations"].as_array().unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line["start_ms"] == 2000
            && line["end_ms"] == 3000
            && line["source"] == "microphone"));
        assert_eq!(
            db.query_row(
                "SELECT started_ms,ended_ms FROM coarse_segments WHERE text='跨越前后窗口的原始句子'",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            )
            .unwrap(),
            (1000, 4000)
        );
        let invalid = serde_json::json!({"events":[{"start_ms":1000,"end_ms":4000,"title":"越界","summary":null,"sources":"microphone","merge_with_event_id":null}]});
        assert_eq!(
            publish_fit(&mut db, &task, invalid, 100_001).unwrap_err(),
            "invalid_fit_boundary_range"
        );
    }

    #[test]
    fn two_hour_meeting_across_compute_windows_remains_one_linear_event() {
        let mut db = db();
        let minute = 60_000;
        let now = 180 * minute;
        for index in 0..24 {
            let start = index * 5 * minute;
            coarse(
                &db,
                &format!("speech-{index}"),
                start,
                start + minute,
                "会议中的连续讨论",
            );
        }
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        assert_eq!(refresh_candidates(&mut db, now, true).unwrap(), 8);
        let mut event_id: Option<Uuid> = None;
        for index in 0..8 {
            let task = take_task_fair(&mut db, CloudProvider::Llm, now, index)
                .unwrap()
                .unwrap();
            let input = llm_input(&db, &task).unwrap();
            if let Some(id) = event_id {
                assert!(input.contains(&id.to_string()));
                assert!(input.contains(&format!("会议摘要{}", index - 1)));
            }
            let fit = serde_json::json!({"events":[{"start_ms":task.start,"end_ms":task.end,
                "title":"产品周会","summary":[{"replaces":if index==0 { vec![] } else { vec![0] },"text":format!("会议摘要{}",index),"evidence_indices":[0]}],"sources":"microphone","transcribe":true,
                "merge_with_event_id":event_id.map(|id:Uuid|id.to_string())}]});
            assert_eq!(publish_fit(&mut db, &task, fit, now).unwrap(), 1);
            event_id = Some(
                Uuid::parse_str(
                    &db.query_row::<String, _, _>(
                        "SELECT id FROM named_events WHERE deleted_ms IS NULL",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
        }
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            event_detail(&db, event_id.unwrap(), 0, 10)
                .unwrap()
                .summary
                .as_deref(),
            Some("会议摘要7")
        );
        let (_start, end): (i64, i64) = db
            .query_row("SELECT started_ms,ended_ms FROM named_events", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(end, 116 * minute);
    }

    #[test]
    fn title_edit_keeps_paid_asr_and_manual_source_selection() {
        let mut db = db();
        let now = 10_000_000;
        media(&db, "mic", now - 100_000, now - 90_000);
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
            frames,bytes,media_path,retention_decision,analysis_state,speech_active)
            VALUES ('sys','session','system',?1,?2,0,480000,100,'media/system/example.opus',
            'retain','complete',0)",
            params![now - 100_000, now - 90_000],
        )
        .unwrap();
        let id = create_event(
            &mut db,
            now - 100_000,
            now - 90_000,
            "第一版",
            EventSources::Microphone,
            now,
        )
        .unwrap();
        let (source, version): (String, String) = db
            .query_row(
                "SELECT source,target_version FROM cloud_tasks
            WHERE target_kind='event'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "microphone");
        edit_event(
            &mut db,
            id,
            1,
            now - 100_000,
            now - 90_000,
            "改名",
            None,
            now + 1,
        )
        .unwrap();
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM cloud_tasks WHERE target_kind='event'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let current: String = db
            .query_row(
                "SELECT target_version FROM cloud_tasks WHERE target_kind='event'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(current, version);
        let item = timeline(&db, now - 110_000, now)
            .unwrap()
            .into_iter()
            .find(|i| i.id == Some(id))
            .unwrap();
        assert_eq!(item.title.as_deref(), Some("改名"));
        assert_eq!(item.revision, Some(2));
        assert_eq!(item.sources, EventSources::Microphone);
    }

    #[test]
    fn overlapping_sources_keep_the_other_sources_unorganized_audio_visible() {
        let mut db = db();
        media(&db, "mic", 0, 30_000);
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
            frames,bytes,media_path,retention_decision,analysis_state,speech_active)
            VALUES ('sys','session','system',0,30000,0,480000,100,'media/system/example.opus',
            'retain','complete',0)",
            [],
        )
        .unwrap();
        let mic = create_event(
            &mut db,
            10_000,
            20_000,
            "麦克风对话",
            EventSources::Microphone,
            40_000,
        )
        .unwrap();
        let sys = create_event(
            &mut db,
            15_000,
            25_000,
            "电脑声音",
            EventSources::System,
            40_001,
        )
        .unwrap();
        let items = timeline(&db, 0, 30_000).unwrap();
        assert!(items.iter().any(|i| i.id == Some(mic)));
        assert!(items.iter().any(|i| i.id == Some(sys)));
        assert!(items.iter().any(|i| i.id.is_none()
            && i.sources == EventSources::System
            && i.started_ms <= 10_000
            && i.ended_ms >= 15_000));
        assert!(items.iter().any(|i| i.id.is_none()
            && i.sources == EventSources::Microphone
            && i.started_ms <= 20_000
            && i.ended_ms >= 25_000));
    }

    #[test]
    fn split_merge_and_cancel_change_only_the_virtual_event_axis() {
        let mut db = db();
        let now = 10_000_000;
        media(&db, "saved", now - 100_000, now - 80_000);
        coarse(&db, "speech", now - 100_000, now - 80_000, "整段会议");
        let first = create_event(
            &mut db,
            now - 100_000,
            now - 80_000,
            "整场",
            EventSources::Microphone,
            now,
        )
        .unwrap();
        let (left, right) =
            split_event(&mut db, first, 1, now - 90_000, "前半", "后半", now + 1).unwrap();
        assert_eq!(
            timeline(&db, now - 110_000, now)
                .unwrap()
                .iter()
                .filter(|i| i.id.is_some())
                .count(),
            2
        );
        let merged = merge_events(&mut db, left, 1, right, 1, "合并后", now + 2).unwrap();
        assert_eq!(
            timeline(&db, now - 110_000, now)
                .unwrap()
                .iter()
                .filter(|i| i.id.is_some())
                .count(),
            1
        );
        delete_event(&mut db, merged, 1, now + 3).unwrap();
        let items = timeline(&db, now - 110_000, now).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].id.is_none());
        let media_count: i64 = db
            .query_row("SELECT count(*) FROM audio_blocks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(media_count, 1);
        refresh_candidates(&mut db, now + 4, true).unwrap();
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        let task = take_task(&mut db, CloudProvider::Llm, now + 4)
            .unwrap()
            .unwrap();
        let fit = serde_json::json!({"events":[{"start_ms":now-100_000,"end_ms":now-80_000,
            "title":"机器重建","sources":"microphone","transcribe":true,"merge_with_event_id":null}]});
        assert_eq!(publish_fit(&mut db, &task, fit, now + 5).unwrap(), 0);
    }

    #[test]
    fn uncertain_paid_request_queries_same_remote_id_after_restart_and_manual_retry_rotates_it() {
        let mut db = db();
        let now = 10_000_000;
        media(&db, "saved", now - 100_000, now - 90_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        create_event(
            &mut db,
            now - 100_000,
            now - 90_000,
            "测试",
            EventSources::Microphone,
            now,
        )
        .unwrap();
        let first = take_task(&mut db, CloudProvider::Asr, now)
            .unwrap()
            .unwrap();
        mark_submit_intent(&db, &first, now + 1).unwrap();
        task_failed(&db, &first, "provider_transport", now + 2).unwrap();
        recover_tasks(&mut db, now + 3).unwrap();
        let retry = take_task(&mut db, CloudProvider::Asr, now + 100_000)
            .unwrap()
            .unwrap();
        assert_eq!(retry.remote_id, first.remote_id);
        assert_eq!(retry.submit_attempts, 1);
        task_failed(
            &db,
            &retry,
            "provider_missing_submitted_task",
            now + 100_001,
        )
        .unwrap();
        retry_task(&db, first.id, now + 100_002).unwrap();
        let manual = take_task(&mut db, CloudProvider::Asr, now + 100_003)
            .unwrap()
            .unwrap();
        assert_ne!(manual.remote_id, first.remote_id);
        assert_eq!(manual.submit_attempts, 0);
    }

    #[test]
    fn empty_precise_result_is_visible_and_explicitly_retryable() {
        let mut db = db();
        let now = 10_000_000;
        media(&db, "saved", now - 100_000, now - 90_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let id = create_event(
            &mut db,
            now - 100_000,
            now - 90_000,
            "测试",
            EventSources::Microphone,
            now,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, now)
            .unwrap()
            .unwrap();
        publish_asr(&mut db, &task, &[], now + 1).unwrap();
        let item = timeline(&db, now - 110_000, now)
            .unwrap()
            .into_iter()
            .find(|i| i.id == Some(id))
            .unwrap();
        assert_eq!(item.processing_state, "no_transcript");
        assert_eq!(
            task_views(&db, CloudProvider::Asr, 10).unwrap()[0].state,
            "no_transcript"
        );
        retry_task(&db, task.id, now + 2).unwrap();
        assert!(
            take_task(&mut db, CloudProvider::Asr, now + 3)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn account_block_survives_restart_and_explicit_resume_does_not_replay_failed_work() {
        let (mut db, task) = summary_fixture();
        task_failed(&db, &task, "provider_http_402", 100_001).unwrap();
        assert_eq!(
            queue_states(&db).unwrap()[0].blocked_reason.as_deref(),
            Some("provider_http_402")
        );
        init(&db).unwrap();
        assert!(
            configured(&db).unwrap_or(false) == false
                || model(&db, CloudProvider::Llm).unwrap().is_some()
        );
        assert!(
            take_task(&mut db, CloudProvider::Llm, 200_000)
                .unwrap()
                .is_none()
        );
        set_paused(&db, CloudProvider::Llm, true).unwrap();
        set_paused(&db, CloudProvider::Llm, false).unwrap();
        assert!(queue_states(&db).unwrap()[0].blocked_reason.is_none());
        assert_eq!(
            db.query_row::<String, _, _>(
                "SELECT state FROM cloud_tasks WHERE id=?1",
                [task.id.to_string()],
                |r| r.get(0)
            )
            .unwrap(),
            "failed"
        );
        assert!(
            take_task(&mut db, CloudProvider::Llm, 200_001)
                .unwrap()
                .is_none()
        );
        retry_task(&db, task.id, 200_002).unwrap();
        let retried = take_task(&mut db, CloudProvider::Llm, 200_003)
            .unwrap()
            .unwrap();
        assert_eq!(retried.id, task.id);
        assert_eq!(retried.attempts, 0);
    }

    #[test]
    fn replayed_account_failure_cannot_reblock_after_explicit_recovery() {
        for configure in [false, true] {
            let (db, task) = summary_fixture();
            task_failed(&db, &task, "provider_http_402", 100_001).unwrap();
            if configure {
                set_configured_with_credential(
                    &db,
                    CloudProvider::Llm,
                    "deepseek-flash",
                    &format!("deepseek:{}", Uuid::new_v4()),
                )
                .unwrap();
            } else {
                set_paused(&db, CloudProvider::Llm, false).unwrap();
            }
            // The request's account failure was committed under its outbound
            // gate. A later common failure handler must be an idempotent replay.
            task_failed(&db, &task, "provider_http_402", 100_002).unwrap();
            assert!(queue_states(&db).unwrap()[0].blocked_reason.is_none());
            assert_eq!(
                db.query_row::<i64, _, _>(
                    "SELECT retry_failures FROM cloud_tasks WHERE id=?1",
                    [task.id.to_string()],
                    |r| r.get(0)
                )
                .unwrap(),
                1
            );
        }
    }

    #[test]
    fn persistent_network_failures_stop_at_budget_and_manual_retry_keeps_paid_identity() {
        let (db, mut task) = summary_fixture();
        for attempt in 0..8 {
            db.execute(
                "UPDATE cloud_tasks SET state='processing' WHERE id=?1",
                [task.id.to_string()],
            )
            .unwrap();
            task.attempts = attempt;
            task_failed(&db, &task, "provider_read_timeout", 100_001 + attempt).unwrap();
            let state = db
                .query_row::<String, _, _>(
                    "SELECT state FROM cloud_tasks WHERE id=?1",
                    [task.id.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(state, if attempt < 7 { "pending" } else { "failed" });
        }
        retry_task(&db, task.id, 200_000).unwrap();
        assert_eq!(
            db.query_row::<i64, _, _>(
                "SELECT attempts FROM cloud_tasks WHERE id=?1",
                [task.id.to_string()],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn stale_account_failure_cannot_unblock_the_provider_or_retry_a_replaced_target() {
        let (db, task) = summary_fixture();
        task_failed(&db, &task, "provider_http_402", 100_001).unwrap();
        db.execute(
            "UPDATE candidate_windows SET input_hash='newer-version' WHERE id=?1",
            [&task.target_id],
        )
        .unwrap();
        assert!(retry_task(&db, task.id, 200_000).is_err());
        assert_eq!(
            queue_states(&db).unwrap()[0].blocked_reason.as_deref(),
            Some("provider_http_402")
        );
        set_configured_with_credential(
            &db,
            CloudProvider::Llm,
            "deepseek-flash",
            "new-test-reference",
        )
        .unwrap();
        assert!(queue_states(&db).unwrap()[0].blocked_reason.is_none());
    }

    #[test]
    fn credential_wait_does_not_spend_the_provider_retry_budget() {
        let (db, mut task) = summary_fixture();
        for attempt in 0..10 {
            db.execute(
                "UPDATE cloud_tasks SET state='processing' WHERE id=?1",
                [task.id.to_string()],
            )
            .unwrap();
            task.attempts = attempt;
            task_failed(&db, &task, "credential_missing", 100_001 + attempt).unwrap();
        }
        assert_eq!(
            db.query_row::<i64, _, _>(
                "SELECT retry_failures FROM cloud_tasks WHERE id=?1",
                [task.id.to_string()],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
        db.execute(
            "UPDATE cloud_tasks SET state='processing' WHERE id=?1",
            [task.id.to_string()],
        )
        .unwrap();
        task_failed(&db, &task, "provider_read_timeout", 200_000).unwrap();
        assert_eq!(
            db.query_row::<String, _, _>(
                "SELECT state FROM cloud_tasks WHERE id=?1",
                [task.id.to_string()],
                |r| r.get(0)
            )
            .unwrap(),
            "pending"
        );
    }

    #[test]
    fn twenty_millisecond_missing_tail_is_real_missing_audio() {
        let db = db();
        media(&db, "tail", 1000, 4980);
        assert!(utterance_has_retained_audio(&db, "microphone", 1000, 4980, false).unwrap());
        assert!(!utterance_has_retained_audio(&db, "microphone", 1000, 5000, false).unwrap());
        assert_eq!(
            voice_manifest(&db, "microphone", 1000, 5000).unwrap_err(),
            "speaker audio unavailable"
        );
    }

    #[test]
    fn local_failure_counts_use_failed_subset_and_preserve_source_and_overlap() {
        let db = db();
        for (id, source, state, attempts, retention, start, end) in [
            (
                "failed-mic",
                "microphone",
                "failed",
                1,
                "retain",
                1_000,
                3_000,
            ),
            (
                "exhausted-sys",
                "system",
                "pending",
                3,
                "retain",
                2_000,
                4_000,
            ),
            (
                "retryable",
                "microphone",
                "pending",
                2,
                "retain",
                2_000,
                4_000,
            ),
            (
                "cleared",
                "microphone",
                "failed",
                3,
                "all_data",
                2_000,
                4_000,
            ),
            ("touching", "system", "failed", 1, "retain", 4_000, 5_000),
        ] {
            media(&db, id, start, end);
            db.execute("UPDATE audio_blocks SET source=?2,analysis_state=?3,analysis_attempts=?4,retention_decision=?5 WHERE id=?1",
                params![id,source,state,attempts,retention]).unwrap();
        }
        db.execute("INSERT INTO coarse_jobs(id,block_id,session_id,source,start_sample,end_sample,started_ms,ended_ms,state)
            VALUES('coarse-failed','block','s','system',0,100,1500,3500,'failed')", []).unwrap();
        assert_eq!(
            asr::local_analysis_failures(&db, 2_500, 4_000, 1).unwrap(),
            1
        );
        assert_eq!(
            asr::local_analysis_failures(&db, 2_500, 4_000, 2).unwrap(),
            2
        );
        assert_eq!(
            asr::local_analysis_failures(&db, 2_500, 4_000, 3).unwrap(),
            3
        );
        assert_eq!(
            asr::local_analysis_failures(&db, 2_500, 4_000, 0).unwrap(),
            0
        );
        assert_eq!(
            asr::local_analysis_failures(&db, 5_000, 6_000, 3).unwrap(),
            0
        );
    }

    #[test]
    fn calendar_days_include_cross_day_material_and_notes_but_not_gaps_or_cleared_ranges() {
        let db = db();
        let day = 86_400_000;
        let bounds = [0, day, day * 2, day * 3, day * 4];
        media(&db, "cross", day - 1_000, day + 1_000);
        db.execute(
            "INSERT INTO moments(id,kind,anchor_ms,created_ms) VALUES(?1,'mark',?2,?2)",
            params![Uuid::new_v4().to_string(), day * 2 + 1],
        )
        .unwrap();
        db.execute("INSERT INTO manual_ranges(id,started_ms,ended_ms,source_mask,revision) VALUES(?1,?2,?3,1,1)", params![Uuid::new_v4().to_string(), day * 3 + 10, day * 3 + 20]).unwrap();
        db.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES ('calendar-gap','microphone',?1,?2,'manual_pause')",params![day*3,day*4]).unwrap();
        assert_eq!(
            calendar_days(&db, &bounds, day * 4).unwrap(),
            vec![0, day, day * 2]
        );
        assert_eq!(calendar_days(&db, &bounds, day - 1).unwrap(), vec![0]);
        db.execute("DELETE FROM audio_blocks", []).unwrap();
        assert_eq!(calendar_days(&db, &bounds, day * 4).unwrap(), vec![day * 2]);
        assert!(calendar_days(&db, &[0, 0], day).is_err());
        assert!(calendar_days(&db, &[i64::MIN, i64::MAX], day).is_err());
        assert!(calendar_days(&db, &[0; 33], day).is_err());
        // 23/25-hour local days are valid; exact midnight belongs only to the next day.
        assert!(calendar_days(&db, &[0, 82_800_000, 172_800_000], day * 4).is_ok());
    }

    #[test]
    fn calendar_adjacent_seeks_all_history_and_preserves_half_open_and_point_boundaries() {
        let db = db();
        let day = 86_400_000;
        let old = -day * 365 * 200;
        media(&db, "ancient", old, old + 1_000);
        media(&db, "cross", day - 1_000, day * 2);
        db.execute(
            "INSERT INTO moments(id,kind,anchor_ms,created_ms) VALUES('midnight','mark',?1,?1)",
            params![day * 3],
        )
        .unwrap();
        db.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES('gap','microphone',?1,?2,'manual_pause')", params![day * 2, day * 3]).unwrap();
        db.execute(
            "INSERT INTO manual_ranges VALUES('cleared',?1,?2,1,1)",
            params![day * 2, day * 3],
        )
        .unwrap();
        media(&db, "future", day * 5, day * 6);
        media(&db, "future-today", day * 4 + 10, day * 4 + 20);
        assert_eq!(
            calendar_adjacent(&db, 0, day, day * 4).unwrap(),
            (Some(day), Some(old + 999))
        );
        assert_eq!(
            calendar_adjacent(&db, day, day * 2, day * 4).unwrap(),
            (Some(day * 3), Some(day - 1))
        );
        assert_eq!(
            calendar_adjacent(&db, day * 2, day * 3, day * 4).unwrap(),
            (Some(day * 3), Some(day * 2 - 1))
        );
        assert_eq!(
            calendar_adjacent(&db, day * 3, day * 4, day * 4).unwrap(),
            (None, Some(day * 2 - 1))
        );
        assert_eq!(
            calendar_adjacent(&db, old, old + day, day * 4).unwrap(),
            (Some(day - 1_000), None)
        );
        assert_eq!(
            calendar_days(&db, &[day * 4, day * 5, day * 6], day * 4 + 1).unwrap(),
            Vec::<i64>::new()
        );
        // Both DST day lengths use supplied boundaries, without UTC-day math.
        for length in [82_800_000, 90_000_000] {
            assert!(calendar_adjacent(&db, 0, length, day * 4).is_ok());
        }
        for (start, end) in [(0, 0), (1, 0), (0, 1), (i64::MIN, i64::MAX)] {
            assert_eq!(
                calendar_adjacent(&db, start, end, day * 4).unwrap_err(),
                "invalid calendar boundaries"
            );
        }
    }

    #[test]
    fn calendar_projections_share_named_coarse_and_visible_manual_range_rules() {
        let db = db();
        let day = 86_400_000;
        coarse(&db, "coarse", day + 5, day + 10, "synthetic");
        db.execute("INSERT INTO named_events(id,started_ms,ended_ms,title,source_mask,revision,origin,boundary_locked,title_locked,asr_required,deleted_ms,created_ms) VALUES('event',?1,?2,'synthetic',1,1,'manual',1,1,1,NULL,0)", params![day * 3, day * 3 + 10]).unwrap();
        db.execute(
            "INSERT INTO manual_ranges VALUES('precise-only',?1,?2,1,1)",
            params![day * 5, day * 6],
        )
        .unwrap();
        db.execute("INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,source,state,created_ms,updated_ms) VALUES('task','asr','range','precise-only','1','test','1',?1,?2,'microphone','complete',0,0)", params![day * 5, day * 6]).unwrap();
        db.execute("INSERT INTO precise_segments VALUES('precise','task','microphone',?1,?2,'synthetic','scope',NULL)", params![day * 5 + 10, day * 5 + 20]).unwrap();
        let bounds = (0..=7).map(|index| index * day).collect::<Vec<_>>();
        assert_eq!(
            calendar_days(&db, &bounds, day * 7).unwrap(),
            vec![day, day * 3, day * 5]
        );
        assert_eq!(
            calendar_adjacent(&db, day * 2, day * 3, day * 7).unwrap(),
            (Some(day * 3), Some(day + 9))
        );
        assert_eq!(
            calendar_adjacent(&db, day * 4, day * 5, day * 7).unwrap(),
            (Some(day * 5), Some(day * 3 + 9))
        );
        // A stale precise task no longer makes an otherwise cleared range visible.
        db.execute("UPDATE manual_ranges SET revision=2", [])
            .unwrap();
        db.execute("UPDATE named_events SET deleted_ms=1", [])
            .unwrap();
        assert_eq!(calendar_days(&db, &bounds, day * 7).unwrap(), vec![day]);
        assert_eq!(
            calendar_adjacent(&db, day * 2, day * 3, day * 7).unwrap(),
            (None, Some(day + 9))
        );
    }

    #[test]
    fn recording_projection_requires_active_source_and_current_confirmed_tail() {
        let db = db();
        media(&db, "live", 1_000, 6_000);
        db.execute(
            "INSERT INTO recording_sessions VALUES('session','microphone',1000,6000,NULL,'open')",
            [],
        )
        .unwrap();
        let mut items = timeline(&db, 0, 10_000).unwrap();
        mark_recording_items(&db, &mut items, &[AudioSource::Microphone], 7_000).unwrap();
        assert_eq!(items[0].recording_sources, vec![AudioSource::Microphone]);
        for (active, now) in [
            (vec![], 7_000),
            (vec![AudioSource::System], 7_000),
            (vec![AudioSource::Microphone], 40_000),
        ] {
            let mut items = timeline(&db, 0, 10_000).unwrap();
            mark_recording_items(&db, &mut items, &active, now).unwrap();
            assert!(items[0].recording_sources.is_empty());
        }
        db.execute("UPDATE recording_sessions SET state='complete'", [])
            .unwrap();
        let mut items = timeline(&db, 0, 10_000).unwrap();
        mark_recording_items(&db, &mut items, &[AudioSource::Microphone], 7_000).unwrap();
        assert!(items[0].recording_sources.is_empty());
    }

    #[test]
    fn timeline_preview_is_unicode_bounded_and_source_scoped() {
        let db = db();
        coarse(&db, "microphone", 1_000, 2_000, &"你好🙂".repeat(100));
        db.execute("INSERT INTO coarse_segments VALUES(?1,'block','session','system',500,900,'电脑文字','test')", params![Uuid::new_v4().to_string()]).unwrap();
        let mic = timeline_preview(&db, 0, 3_000, 1, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(mic.text.chars().count(), 161);
        assert!(mic.text.ends_with('…'));
        assert_eq!(mic.source, AudioSource::Microphone);
        let system = timeline_preview(&db, 0, 3_000, 2, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(system.text, "电脑文字");
        assert_eq!(system.source, AudioSource::System);
        assert!(
            timeline_preview(&db, 2_000, 3_000, 1, None, None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn day_timeline_is_a_summary_and_detail_pages_transcript_in_time_order() {
        let mut db = db();
        let now = 10_000_000;
        media(&db, "saved", now - 100_000, now - 80_000);
        coarse(&db, "first", now - 99_000, now - 98_000, "第一句");
        coarse(&db, "second", now - 91_000, now - 90_000, "第二句");
        let id = create_event(
            &mut db,
            now - 100_000,
            now - 80_000,
            "测试",
            EventSources::Microphone,
            now,
        )
        .unwrap();
        let summary = timeline(&db, now - 110_000, now).unwrap();
        assert!(
            summary
                .iter()
                .all(|item| item.coarse_segments.is_empty() && item.precise_segments.is_empty())
        );
        let preview = summary
            .iter()
            .find(|item| item.id == Some(id))
            .unwrap()
            .preview
            .as_ref()
            .unwrap();
        assert_eq!(preview.text, "第一句");
        assert!(!preview.is_precise);
        // A day slice must not pick a transcript outside its displayed bounds.
        let sliced = timeline(&db, now - 95_000, now).unwrap();
        assert_eq!(
            sliced
                .iter()
                .find(|item| item.id == Some(id))
                .unwrap()
                .preview
                .as_ref()
                .unwrap()
                .text,
            "第二句"
        );
        let first = event_detail(&db, id, 0, 1).unwrap();
        let second = event_detail(&db, id, 1, 1).unwrap();
        assert_eq!(first.coarse_segments[0].text, "第一句");
        assert_eq!(second.coarse_segments[0].text, "第二句");
        assert!(event_detail(&db, id, 0, 201).is_err());
    }

    #[test]
    fn boundary_revert_resumes_the_original_asr_task() {
        let mut db = db();
        media(&db, "audio", 1_000, 11_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let id = create_event(
            &mut db,
            1_000,
            6_000,
            "会话",
            EventSources::Microphone,
            12_000,
        )
        .unwrap();
        let old = take_task(&mut db, CloudProvider::Asr, 12_000)
            .unwrap()
            .unwrap();
        edit_event(&mut db, id, 1, 1_000, 11_000, "会话", None, 12_001).unwrap();
        mark_submit_intent(&db, &old, 12_001).unwrap();
        edit_event(&mut db, id, 2, 1_000, 6_000, "会话", None, 12_002).unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 72_002)
            .unwrap()
            .unwrap();
        assert_eq!((task.start, task.end), (1_000, 6_000));
        assert!(task_current(&db, &task).unwrap());
    }

    #[test]
    fn automatic_fit_cannot_add_a_source_without_evidence() {
        let mut db = db();
        coarse(&db, "speech", 1_000, 6_000, "只有麦克风说话");
        set_configured(&db, CloudProvider::Llm, "deepseek-flash").unwrap();
        refresh_candidates(&mut db, 100_000, true).unwrap();
        let task = take_task(&mut db, CloudProvider::Llm, 100_000)
            .unwrap()
            .unwrap();
        let fit = serde_json::json!({"events":[{"start_ms":1_000,"end_ms":6_000,
            "title":"讨论","sources":"both","transcribe":true,"merge_with_event_id":null}]});
        assert_eq!(publish_fit(&mut db, &task, fit, 100_001).unwrap(), 0);
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM named_events", [], |r| r.get(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn discarded_silence_does_not_create_an_unorganized_audio_block() {
        let db = db();
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
            frames,bytes,media_path,retention_decision,analysis_state,speech_active)
            VALUES ('quiet','session','microphone',1000,6000,0,240000,0,NULL,
              'discard_silence','complete',0)",
            [],
        )
        .unwrap();
        assert!(timeline(&db, 0, 10_000).unwrap().is_empty());
    }

    #[test]
    fn new_moments_before_audio_seals_do_not_merge_or_recolor_existing_material() {
        let db = db();
        media(&db, "mic", 1_000, 9_000);
        media(&db, "sys", 3_000, 6_000);
        db.execute("UPDATE audio_blocks SET source='system' WHERE id='sys'", [])
            .unwrap();
        db.execute(
            "INSERT INTO recording_sessions VALUES('live','microphone',8000,9000,NULL,'open')",
            [],
        )
        .unwrap();
        let projection = || {
            let mut items = timeline(&db, 0, 20_000).unwrap();
            mark_recording_items(&db, &mut items, &[AudioSource::Microphone], 10_000).unwrap();
            items
        };
        let before = serde_json::to_value(projection()).unwrap();
        // Includes an early point, two points beyond the sealed tail and a
        // point already covered by audio. Only real material-free gaps group.
        for anchor in [500, 9_500, 9_600, 4_000] {
            db.execute(
                "INSERT INTO moments(id,kind,anchor_ms,created_ms) VALUES(?1,'mark',?2,?2)",
                params![Uuid::new_v4().to_string(), anchor],
            )
            .unwrap();
        }
        let after = projection();
        let audio: Vec<_> = after
            .iter()
            .filter(|item| item.processing_state != "moment_only")
            .collect();
        assert_eq!(serde_json::to_value(audio).unwrap(), before);
        let points: Vec<_> = after
            .iter()
            .filter(|item| item.processing_state == "moment_only")
            .collect();
        assert_eq!(points.len(), 2);
        assert!(points.iter().all(|item| item.recording_sources.is_empty()));
        assert!(
            points
                .iter()
                .any(|item| item.started_ms == 9_500 && item.ended_ms == 9_601)
        );
        // Once the next block arrives, its real source absorbs the tail marks.
        media(&db, "next", 9_000, 10_000);
        let next = projection();
        assert_eq!(
            next.iter()
                .filter(|item| item.processing_state == "moment_only")
                .count(),
            1
        );
        assert!(next.iter().any(|item| item.started_ms == 6_000
            && item.ended_ms == 10_000
            && item.sources == EventSources::Microphone
            && item.recording_sources == vec![AudioSource::Microphone]));
    }

    #[test]
    fn retained_audio_without_recognized_speech_is_not_a_moment_only_block() {
        let db = db();
        media(&db, "saved", 1_000, 6_000);
        let items = timeline(&db, 0, 10_000).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].processing_state, "raw_audio");
        assert_eq!(items[0].sources, EventSources::Microphone);
    }

    #[test]
    fn another_sources_distant_audio_does_not_create_empty_parallel_blocks() {
        let mut db = db();
        media(&db, "mic", 0, 30_000);
        for (id, start, end) in [("sys-before", 0, 5_000), ("sys-after", 25_000, 30_000)] {
            db.execute(
                "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
                frames,bytes,media_path,retention_decision,analysis_state,speech_active)
                VALUES (?1,'session','system',?2,?3,0,240000,100,'media/system/example.opus',
                'retain','complete',0)",
                params![id, start, end],
            )
            .unwrap();
        }
        let event = create_event(
            &mut db,
            10_000,
            20_000,
            "麦克风会话",
            EventSources::Microphone,
            40_000,
        )
        .unwrap();
        let items = timeline(&db, 0, 30_000).unwrap();
        assert!(items.iter().any(|item| item.id == Some(event)));
        assert!(
            !items
                .iter()
                .any(|item| item.processing_state == "moment_only")
        );
        assert!(!items.iter().any(|item| {
            item.id.is_none()
                && item.sources == EventSources::System
                && item.started_ms == 10_000
                && item.ended_ms == 20_000
        }));
    }

    #[test]
    fn repeated_manual_range_reuses_its_task_and_transcript() {
        let mut db = db();
        media(&db, "saved", 1_000, 6_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let first =
            transcribe_range(&mut db, 1_000, 6_000, EventSources::Microphone, 7_000).unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 7_001)
            .unwrap()
            .unwrap();
        publish_asr(
            &mut db,
            &task,
            &[crate::cloud::AsrUtterance {
                words: None,
                start_ms: 1_000,
                end_ms: 2_000,
                text: "一句话".to_owned(),
                speaker: Some("1".to_owned()),
            }],
            7_002,
        )
        .unwrap();
        let summary = timeline(&db, 0, 10_000).unwrap();
        let preview = summary
            .iter()
            .find(|item| item.manual_range_id == Some(first))
            .unwrap()
            .preview
            .as_ref()
            .unwrap();
        assert_eq!(preview.text, "一句话");
        assert!(preview.is_precise);
        assert!(
            timeline_preview(&db, 0, 10_000, 2, None, Some(&first.to_string()))
                .unwrap()
                .is_none()
        );
        assert!(
            timeline_preview(&db, 0, 10_000, 1, None, Some(&Uuid::new_v4().to_string()))
                .unwrap()
                .is_none()
        );
        db.execute(
            "UPDATE cloud_tasks SET processor_version='volc-standard-v3-wav-old'
             WHERE id=?1",
            params![task.id.to_string()],
        )
        .unwrap();
        let second =
            transcribe_range(&mut db, 1_000, 6_000, EventSources::Microphone, 7_003).unwrap();
        assert_eq!(first, second);
        let task_count: i64 = db
            .query_row(
                "SELECT count(*) FROM cloud_tasks WHERE target_kind='range' AND target_id=?1",
                params![first.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(task_count, 1);
        assert_eq!(
            range_detail(&db, 1_000, 6_000, EventSources::Microphone, 0, 100)
                .unwrap()
                .precise_segments
                .len(),
            1
        );
    }

    #[test]
    fn mixed_empty_windows_and_known_gaps_never_show_full_transcription() {
        let mut db = db();
        media(&db, "first", 1_000, 6_000);
        media(&db, "second", 931_000, 936_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let id = create_event(
            &mut db,
            1_000,
            936_000,
            "长会话",
            EventSources::Microphone,
            200_000,
        )
        .unwrap();
        let first = take_task(&mut db, CloudProvider::Asr, 200_001)
            .unwrap()
            .unwrap();
        publish_asr(
            &mut db,
            &first,
            &[crate::cloud::AsrUtterance {
                words: None,
                start_ms: 1_000,
                end_ms: 2_000,
                text: "第一句".to_owned(),
                speaker: Some("1".to_owned()),
            }],
            200_002,
        )
        .unwrap();
        let second = take_task(&mut db, CloudProvider::Asr, 200_003)
            .unwrap()
            .unwrap();
        task_failed(&db, &second, "provider_invalid_utterance_text", 200_004).unwrap();
        let partial = event_detail(&db, id, 0, 100).unwrap();
        assert_eq!(partial.processing_state, "partial_asr_failed");
        assert_eq!(partial.precise_segments.len(), 1);
        assert_eq!(partial.precise_segments[0].text, "第一句");
        assert_eq!(
            timeline(&db, 0, 1_000_000)
                .unwrap()
                .iter()
                .find(|item| item.id == Some(id))
                .unwrap()
                .processing_state,
            "partial_asr_failed"
        );
        publish_asr(&mut db, &second, &[], 200_004).unwrap();
        assert_eq!(
            event_detail(&db, id, 0, 100).unwrap().processing_state,
            "partial_transcript"
        );
        let progress = event_detail(&db, id, 0, 1).unwrap().transcription.unwrap();
        assert_eq!(
            (progress.total, progress.complete, progress.empty),
            (2, 2, 1)
        );
        assert!(!progress.boundary_clipped);
        assert_eq!(progress.empty_ranges.len(), 1);
        assert_eq!(
            (
                progress.empty_ranges[0].started_ms,
                progress.empty_ranges[0].ended_ms
            ),
            (931_000, 936_000)
        );
        assert_eq!(progress.empty_ranges[0].source, AudioSource::Microphone);
        let list = timeline(&db, 0, 1_000_000).unwrap();
        let progress = list
            .iter()
            .find(|i| i.id == Some(id))
            .unwrap()
            .transcription
            .as_ref()
            .unwrap();
        assert_eq!(progress.empty, 1);
        assert!(progress.empty_ranges.is_empty());
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES ('gap','microphone',6000,931000,'source_failure')",
            [],
        )
        .unwrap();
        assert_eq!(
            event_detail(&db, id, 0, 100).unwrap().processing_state,
            "partial_transcript"
        );
    }

    #[test]
    fn provider_timing_stays_inside_request_despite_late_gap_metadata() {
        let mut db = db();
        media(&db, "saved", 1_000, 6_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        create_event(
            &mut db,
            1_000,
            6_000,
            "会话",
            EventSources::Microphone,
            7_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 7_001)
            .unwrap()
            .unwrap();
        let utterance = crate::cloud::AsrUtterance {
            words: None,
            start_ms: 4_000,
            end_ms: 6_000,
            text: "越界".to_owned(),
            speaker: None,
        };
        assert_eq!(
            publish_asr(&mut db, &task, &[utterance], 7_002).unwrap_err(),
            "provider_invalid_timing"
        );
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES ('gap','microphone',3000,5000,'source_failure')",
            [],
        )
        .unwrap();
        let utterance = crate::cloud::AsrUtterance {
            words: None,
            start_ms: 2_500,
            end_ms: 3_500,
            text: "缺口".to_owned(),
            speaker: None,
        };
        assert_eq!(publish_asr(&mut db, &task, &[utterance], 7_003).unwrap(), 1);
    }

    #[test]
    fn provider_utterance_cannot_bridge_discarded_audio_without_a_gap_row() {
        let mut db = db();
        media(&db, "first", 1_000, 2_000);
        media(&db, "last", 4_000, 5_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        create_event(
            &mut db,
            1_000,
            5_000,
            "讨论",
            EventSources::Microphone,
            6_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 6_001)
            .unwrap()
            .unwrap();
        assert_eq!(
            publish_asr(
                &mut db,
                &task,
                &[crate::cloud::AsrUtterance {
                    words: None,
                    start_ms: 500,
                    end_ms: 3_500,
                    text: "跨过空白".to_owned(),
                    speaker: None,
                }],
                6_002
            )
            .unwrap_err(),
            "provider_invalid_timing"
        );
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM precise_segments", [], |row| row
                .get(0))
                .unwrap(),
            0
        );
        assert_eq!(
            publish_asr(
                &mut db,
                &task,
                &[crate::cloud::AsrUtterance {
                    words: None,
                    start_ms: 100,
                    end_ms: 900,
                    text: "第一段".to_owned(),
                    speaker: None,
                }],
                6_003
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn provider_timing_allows_only_block_clock_rounding_gaps() {
        let db = db();
        media(&db, "first", 1_000, 6_000);
        media(&db, "next", 6_003, 11_003);
        assert!(utterance_has_retained_audio(&db, "microphone", 2_000, 10_000, false).unwrap());
        assert!(!utterance_has_retained_audio(&db, "microphone", 980, 10_000, false).unwrap());
        db.execute(
            "UPDATE audio_blocks SET started_ms=6030,ended_ms=11030 WHERE id='next'",
            [],
        )
        .unwrap();
        assert!(!utterance_has_retained_audio(&db, "microphone", 2_000, 10_000, false).unwrap());
        db.execute(
            "UPDATE audio_blocks SET started_ms=6003,ended_ms=11003 WHERE id='next'",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES ('real','microphone',6001,6002,'source_failure')",
            [],
        )
        .unwrap();
        assert!(!utterance_has_retained_audio(&db, "microphone", 2_000, 10_000, false).unwrap());
    }

    #[test]
    fn an_event_crossing_midnight_appears_on_both_days_with_one_identity() {
        let mut db = db();
        let day = 24 * 60 * 60 * 1000_i64;
        media(&db, "before", day - 30_000, day);
        media(&db, "after", day, day + 30_000);
        let id = create_event(
            &mut db,
            day - 30_000,
            day + 30_000,
            "跨日会议",
            EventSources::Microphone,
            day + 40_000,
        )
        .unwrap();
        let before = timeline(&db, 0, day).unwrap();
        let after = timeline(&db, day, 2 * day).unwrap();
        assert_eq!(before.iter().filter(|item| item.id == Some(id)).count(), 1);
        assert_eq!(after.iter().filter(|item| item.id == Some(id)).count(), 1);
        assert_eq!(
            (
                event_detail(&db, id, 0, 100).unwrap().started_ms,
                event_detail(&db, id, 0, 100).unwrap().ended_ms
            ),
            (day - 30_000, day + 30_000)
        );
    }

    #[test]
    fn failed_response_read_keeps_paid_request_id_until_queried() {
        let mut db = db();
        media(&db, "saved", 1_000, 6_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        create_event(
            &mut db,
            1_000,
            6_000,
            "会话",
            EventSources::Microphone,
            7_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 7_001)
            .unwrap()
            .unwrap();
        mark_submit_intent(&db, &task, 7_002).unwrap();
        task_failed(&db, &task, "provider_read_timeout", 7_003).unwrap();
        // A read timeout is transient, but must respect backoff and keep the
        // original paid request identity for query-before-resubmit recovery.
        assert!(
            take_task(&mut db, CloudProvider::Asr, 7_005)
                .unwrap()
                .is_none()
        );
        let retry = take_task(&mut db, CloudProvider::Asr, 43_004)
            .unwrap()
            .unwrap();
        assert_eq!(retry.remote_id, task.remote_id);
        assert!(retry.submit_attempts > 0);
    }

    #[test]
    fn missing_credential_after_paid_submit_still_queries_the_original_request() {
        let mut db = db();
        media(&db, "saved", 1_000, 6_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        create_event(
            &mut db,
            1_000,
            6_000,
            "会话",
            EventSources::Microphone,
            7_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 7_001)
            .unwrap()
            .unwrap();
        mark_submit_intent(&db, &task, 7_002).unwrap();
        task_failed(&db, &task, "credential_missing", 7_003).unwrap();
        set_configured_with_credential(&db, CloudProvider::Llm, "deepseek-flash", "test:other")
            .unwrap();
        assert!(
            take_task(&mut db, CloudProvider::Asr, 7_004)
                .unwrap()
                .is_none()
        );
        recover_tasks(&mut db, 7_004).unwrap();
        let retry = take_task(&mut db, CloudProvider::Asr, 7_005)
            .unwrap()
            .unwrap();
        assert_eq!(retry.remote_id, task.remote_id);
        assert!(retry.submit_attempts > 0);
        db.execute(
            "UPDATE cloud_tasks SET state='failed',error_code='credential_missing' WHERE id=?1",
            params![task.id.to_string()],
        )
        .unwrap();
        retry_task(&db, task.id, 37_004).unwrap();
        let manual = take_task(&mut db, CloudProvider::Asr, 37_005)
            .unwrap()
            .unwrap();
        assert_eq!(manual.remote_id, task.remote_id);
    }

    #[test]
    fn a_long_offline_period_keeps_the_remote_asr_query_recoverable() {
        let mut db = db();
        media(&db, "saved", 1_000, 6_000);
        set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let id = create_event(
            &mut db,
            1_000,
            6_000,
            "会话",
            EventSources::Microphone,
            7_000,
        )
        .unwrap();
        let task = take_task(&mut db, CloudProvider::Asr, 7_001)
            .unwrap()
            .unwrap();
        mark_submit_intent(&db, &task, 7_002).unwrap();
        recover_tasks(&mut db, 7_002 + 3 * 60 * 60 * 1000).unwrap();
        let recovered = take_task(&mut db, CloudProvider::Asr, 7_003 + 3 * 60 * 60 * 1000)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.remote_id, task.remote_id);
        assert!(recovered.submit_attempts > 0);
        publish_asr(
            &mut db,
            &recovered,
            &[crate::cloud::AsrUtterance {
                words: None,
                start_ms: 1_000,
                end_ms: 2_000,
                text: "已完成".to_owned(),
                speaker: Some("1".to_owned()),
            }],
            7_004 + 3 * 60 * 60 * 1000,
        )
        .unwrap();
        assert_eq!(
            event_detail(&db, id, 0, 100).unwrap().processing_state,
            "transcribed"
        );
    }
}

pub fn rebuild_understanding(
    tx: &Transaction<'_>,
    event_id: Uuid,
    now: i64,
) -> Result<usize, String> {
    rebuild::enqueue(tx, event_id, now)
}
