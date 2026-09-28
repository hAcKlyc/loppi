use hear_protocol::{CleanupMode, CleanupPreview, EventSources};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS cleanup_previews (
      token TEXT PRIMARY KEY,from_ms INTEGER NOT NULL,to_ms INTEGER NOT NULL,
      source_mask INTEGER NOT NULL,mode TEXT NOT NULL,signature TEXT NOT NULL,
      created_ms INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS clear_ranges (
      id TEXT PRIMARY KEY,from_ms INTEGER NOT NULL,to_ms INTEGER NOT NULL,
      source_mask INTEGER NOT NULL,mode TEXT NOT NULL,
      committed_ms INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS clear_ranges_time ON clear_ranges(from_ms,to_ms);
    CREATE TABLE IF NOT EXISTS media_deletion_jobs (
      media_path TEXT PRIMARY KEY,created_ms INTEGER NOT NULL,
      completed_ms INTEGER,error_code TEXT
    );",
    )
    .map_err(|error| error.to_string())
}

fn source_mask(sources: EventSources) -> i64 {
    match sources {
        EventSources::Microphone => 1,
        EventSources::System => 2,
        EventSources::Both => 3,
    }
}
fn mode_name(mode: CleanupMode) -> &'static str {
    match mode {
        CleanupMode::RawAudio => "raw_audio",
        CleanupMode::AllData => "all_data",
    }
}
fn parse_mode(value: &str) -> Result<CleanupMode, String> {
    match value {
        "raw_audio" => Ok(CleanupMode::RawAudio),
        "all_data" => Ok(CleanupMode::AllData),
        _ => Err("cleanup preview mode invalid".to_owned()),
    }
}

fn validate(start: i64, end: i64) -> Result<(), String> {
    if start < 0 || end <= start {
        Err("cleanup range invalid".to_owned())
    } else {
        Ok(())
    }
}

fn count(db: &Connection, sql: &str, start: i64, end: i64, mask: i64) -> Result<i64, String> {
    db.query_row(sql, params![start, end, mask], |r| r.get(0))
        .map_err(|error| error.to_string())
}

fn selection_signature(db: &Connection, start: i64, end: i64, mask: i64) -> Result<String, String> {
    let mut hasher = Sha256::new();
    for (tag, sql) in [
        (
            "audio",
            "SELECT id||'|'||coalesce(media_path,'')||'|'||bytes||'|'||
          retention_decision||'|'||analysis_state FROM audio_blocks
          WHERE started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY id",
        ),
        (
            "coarse",
            "SELECT id||'|'||text FROM coarse_segments
          WHERE started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY id",
        ),
        (
            "precise",
            "SELECT id||'|'||text FROM precise_segments
          WHERE started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY id",
        ),
        (
            "correction",
            "SELECT id||'|'||text||'|'||state||'|'||revision FROM text_corrections
          WHERE started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY id",
        ),
        (
            "voice",
            "SELECT id||'|'||person_id||'|'||coalesce(revoked_ms,0) FROM voice_samples
          WHERE started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY id",
        ),
        (
            "observation",
            "SELECT segment_id||'|'||state||'|'||updated_ms FROM voice_observations
          WHERE started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY segment_id",
        ),
        (
            "binding",
            "SELECT id||'|'||person_id||'|'||state||'|'||updated_ms FROM speaker_bindings
          WHERE started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY id",
        ),
        (
            "event",
            "SELECT id||'|'||title||'|'||revision||'|'||coalesce(deleted_ms,0)
          FROM named_events WHERE started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0 ORDER BY id",
        ),
        (
            "task",
            "SELECT id||'|'||state||'|'||updated_ms FROM cloud_tasks
          WHERE started_ms<?2 AND ended_ms>?1 AND
          (source IS NULL OR (source='microphone' AND ?3&1!=0)
          OR (source='system' AND ?3&2!=0)) ORDER BY id",
        ),
        (
            "range",
            "SELECT id||'|'||revision||'|'||source_mask FROM manual_ranges
          WHERE started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0 ORDER BY id",
        ),
        (
            "moment",
            "SELECT id||'|'||kind||'|'||coalesce(text,'')||'|'||coalesce(media_path,'')||'|'||coalesce(ocr_text,'')
          FROM moments WHERE anchor_ms>=?1 AND anchor_ms<?2 AND ?3>=0 ORDER BY id",
        ),
		(
			"gap",
			"SELECT id||'|'||source||'|'||started_ms||'|'||coalesce(ended_ms,-1)||'|'||reason
			 FROM media_gaps WHERE started_ms<?2 AND coalesce(ended_ms,9223372036854775807)>?1
			 AND ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0)) ORDER BY id",
		),
    ] {
        let mut stmt = db.prepare(sql).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![start, end, mask], |r| r.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        for row in rows {
            hasher.update(tag.as_bytes());
            hasher.update([0]);
            hasher.update(row.map_err(|error| error.to_string())?.as_bytes());
            hasher.update([0]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn preview(
    db: &Connection,
    start: i64,
    end: i64,
    sources: EventSources,
    mode: CleanupMode,
    now: i64,
) -> Result<CleanupPreview, String> {
    validate(start, end)?;
    let mask = source_mask(sources);
    let audio_blocks = count(
        db,
        "SELECT count(*) FROM audio_blocks WHERE media_path IS NOT NULL
      AND started_ms<?2 AND ended_ms>?1 AND
      ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? as u64;
    let audio_bytes = count(
        db,
        "SELECT coalesce(sum(bytes),0) FROM audio_blocks WHERE media_path IS NOT NULL
      AND started_ms<?2 AND ended_ms>?1 AND
      ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? as u64;
    let coarse_segments = count(
        db,
        "SELECT count(*) FROM coarse_segments
      WHERE started_ms<?2 AND ended_ms>?1 AND
      ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? as u64;
    let precise_segments = count(
        db,
        "SELECT count(*) FROM precise_segments
      WHERE started_ms<?2 AND ended_ms>?1 AND
      ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? as u64;
    let voice_samples = count(
        db,
        "SELECT count(*) FROM voice_samples WHERE revoked_ms IS NULL
      AND started_ms<?2 AND ended_ms>?1 AND
      ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? as u64;
    let asr_tasks_awaiting_upload = count(
        db,
        "SELECT count(*) FROM cloud_tasks WHERE provider='asr' AND submit_attempts=0
         AND state IN ('pending','processing','failed') AND started_ms<?2 AND ended_ms>?1
         AND (source IS NULL OR (source='microphone' AND ?3&1!=0)
         OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? as u64;
    let voice_extractions_awaiting_audio = count(
        db,
        "SELECT count(*) FROM voice_observations WHERE state IN ('pending','processing')
         AND started_ms<?2 AND ended_ms>?1 AND
         ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? as u64;
    let event_mask = if mode == CleanupMode::AllData {
        3
    } else {
        mask
    };
    let affected_event_count = count(
        db,
        "SELECT count(*) FROM named_events WHERE deleted_ms IS NULL
      AND started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0",
        start,
        end,
        event_mask,
    )? as u64;
    let affected_events = {
        let mut stmt = db
            .prepare(
                "SELECT id FROM named_events WHERE deleted_ms IS NULL
          AND started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0
          ORDER BY started_ms LIMIT 100",
            )
            .map_err(|error| error.to_string())?;
        stmt.query_map(params![start, end, event_mask], |r| r.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .map(|row| {
                Uuid::parse_str(&row.map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let moment_count: u64 = db
        .query_row(
            "SELECT count(*) FROM moments WHERE anchor_ms>=?1 AND anchor_ms<?2",
            params![start, end],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())? as u64;
    let screenshot_count: u64 = db.query_row(
        "SELECT count(*) FROM moments WHERE kind='screenshot' AND anchor_ms>=?1 AND anchor_ms<?2",
        params![start, end], |row| row.get::<_, i64>(0),
    ).map_err(|error| error.to_string())? as u64;
    let partial = count(
        db,
        "SELECT count(*) FROM (
      SELECT source,started_ms,ended_ms FROM coarse_segments UNION ALL
      SELECT source,started_ms,ended_ms FROM precise_segments)
      WHERE ((started_ms<?1 AND ended_ms>?1) OR (started_ms<?2 AND ended_ms>?2))
      AND ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
        start,
        end,
        mask,
    )? > 0;
    let token = Uuid::new_v4();
    let signature = selection_signature(db, start, end, mask)?;
    db.execute(
        "DELETE FROM cleanup_previews WHERE created_ms<?1",
        params![now - 3_600_000],
    )
    .map_err(|error| error.to_string())?;
    db.execute(
        "INSERT INTO cleanup_previews VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            token.to_string(),
            start,
            end,
            mask,
            mode_name(mode),
            signature,
            now
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(CleanupPreview {
        token,
        mode,
        from_ms: start,
        to_ms: end,
        sources,
        audio_blocks,
        audio_bytes,
        coarse_segments,
        precise_segments,
        voice_samples,
        asr_tasks_awaiting_upload,
        voice_extractions_awaiting_audio,
        affected_event_count,
        affected_events,
        moment_count: if mode == CleanupMode::AllData {
            moment_count
        } else {
            0
        },
        screenshot_count: if mode == CleanupMode::AllData {
            screenshot_count
        } else {
            0
        },
        overlap_may_remove_whole_sentences: mode == CleanupMode::AllData && partial,
        voice_features_retained: mode == CleanupMode::RawAudio,
    })
}

#[derive(Clone)]
pub struct PreviewScope {
    pub token: Uuid,
    pub start: i64,
    pub end: i64,
    pub mask: i64,
    pub mode: CleanupMode,
    pub signature: String,
}

pub fn verified_scope(
    db: &Connection,
    token: Uuid,
    confirmed_all: bool,
    now: i64,
) -> Result<PreviewScope, String> {
    let row: Option<(i64, i64, i64, String, String, i64)> = db
        .query_row(
            "SELECT from_ms,to_ms,source_mask,mode,signature,created_ms
       FROM cleanup_previews WHERE token=?1",
            params![token.to_string()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (start, end, mask, mode, signature, created) =
        row.ok_or_else(|| "cleanup preview expired".to_owned())?;
    let mode = parse_mode(&mode)?;
    if now - created > 3_600_000
        || now < created
        || (mode == CleanupMode::AllData && !confirmed_all)
    {
        return Err("cleanup requires a fresh preview and extra all-data confirmation".to_owned());
    }
    if selection_signature(db, start, end, mask)? != signature {
        return Err("cleanup preview changed; review the current data again".to_owned());
    }
    Ok(PreviewScope {
        token,
        start,
        end,
        mask,
        mode,
        signature,
    })
}

#[derive(Clone)]
pub struct OriginalAudio {
    pub id: String,
    pub session_id: String,
    pub source: String,
    pub start: i64,
    pub end: i64,
    pub sample_start: i64,
    pub frames: usize,
    pub bytes: i64,
    pub path: PathBuf,
    pub speech_active: Option<i64>,
}

pub struct ReplacementAudio {
    pub id: Uuid,
    pub session_id: String,
    pub source: String,
    pub start: i64,
    pub end: i64,
    pub sample_start: i64,
    pub frames: usize,
    pub bytes: u64,
    pub path: PathBuf,
    pub speech_active: Option<i64>,
}

pub struct PreparedMedia {
    pub originals: Vec<OriginalAudio>,
    pub replacements: Vec<ReplacementAudio>,
}

fn source_selected(source: &str, mask: i64) -> bool {
    (source == "microphone" && mask & 1 != 0) || (source == "system" && mask & 2 != 0)
}

fn valid_media_path(path: &Path, source: &str) -> bool {
    let mut parts = path.components();
    parts.next().is_some_and(|part| part.as_os_str() == "media")
        && parts.next().is_some_and(|part| part.as_os_str() == source)
        && parts
            .next()
            .is_some_and(|part| part.as_os_str().to_string_lossy().ends_with(".opus"))
        && parts.next().is_none()
}

pub fn originals(db: &Connection, scope: &PreviewScope) -> Result<Vec<OriginalAudio>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id,session_id,source,started_ms,ended_ms,sample_start,
      frames,bytes,media_path,speech_active FROM audio_blocks WHERE media_path IS NOT NULL
      AND started_ms<?2 AND ended_ms>?1 ORDER BY started_ms,id",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![scope.start, scope.end], |r| {
            Ok(OriginalAudio {
                id: r.get(0)?,
                session_id: r.get(1)?,
                source: r.get(2)?,
                start: r.get(3)?,
                end: r.get(4)?,
                sample_start: r.get(5)?,
                frames: r.get::<_, i64>(6)? as usize,
                bytes: r.get(7)?,
                path: PathBuf::from(r.get::<_, String>(8)?),
                speech_active: r.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut selected = Vec::new();
    for row in rows {
        if !source_selected(&row.source, scope.mask) {
            continue;
        }
        if !valid_media_path(&row.path, &row.source)
            || row.frames == 0
            || row.frames > 48_000 * 11
            || row.end <= row.start
            || row.bytes < 0
        {
            return Err("audio manifest is invalid".to_owned());
        }
        selected.push(row);
    }
    Ok(selected)
}

fn encode_part(
    directory: &Path,
    row: &OriginalAudio,
    pcm: &[f32],
    from: usize,
    to: usize,
) -> Result<ReplacementAudio, String> {
    if from >= to || to > pcm.len() {
        return Err("audio trim range invalid".to_owned());
    }
    let id = Uuid::new_v4();
    let relative = PathBuf::from("media")
        .join(&row.source)
        .join(format!("{id}.opus"));
    let media_dir = directory.join("media").join(&row.source);
    fs::create_dir_all(&media_dir).map_err(|error| error.to_string())?;
    let marker = media_dir.join(format!("{id}.cleanup-staged"));
    let marker_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .map_err(|error| error.to_string())?;
    marker_file.sync_all().map_err(|error| error.to_string())?;
    fs::File::open(&media_dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|error| error.to_string())?;
    let sealed = (|| {
        let mut block = crate::media::OpusBlock::create(&media_dir, id)?;
        block.push(&pcm[from..to])?;
        block.seal()
    })();
    let sealed = match sealed {
        Ok(sealed) => sealed,
        Err(error) => {
            let _ = fs::remove_file(media_dir.join(format!("{id}.pending")));
            let _ = fs::remove_file(media_dir.join(format!("{id}.opus")));
            let _ = fs::remove_file(&marker);
            return Err(error);
        }
    };
    let start = row.start + (from as i64 / 48);
    let end = if to == row.frames {
        row.end
    } else {
        row.start + (to as i64 / 48)
    };
    Ok(ReplacementAudio {
        id,
        session_id: row.session_id.clone(),
        source: row.source.clone(),
        start,
        end,
        sample_start: row.sample_start + from as i64,
        frames: to - from,
        bytes: sealed.bytes,
        path: relative,
        speech_active: row.speech_active,
    })
}

pub fn prepare_media(
    directory: &Path,
    scope: &PreviewScope,
    originals: Vec<OriginalAudio>,
) -> Result<PreparedMedia, String> {
    let mut prepared = PreparedMedia {
        originals,
        replacements: Vec::new(),
    };
    let result = (|| {
        for row in &prepared.originals {
            let cut_start = scope.start.max(row.start).min(row.end);
            let cut_end = scope.end.max(row.start).min(row.end);
            if cut_start <= row.start && cut_end >= row.end {
                continue;
            }
            let full = directory.join(&row.path);
            if fs::symlink_metadata(&full)
                .map_err(|error| error.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("audio file is a symlink".to_owned());
            }
            let pcm = crate::media::decode_block(&full, row.frames)?;
            let before = ((cut_start - row.start).max(0) as usize * 48).min(row.frames);
            let after = ((cut_end - row.start).max(0) as usize * 48).min(row.frames);
            if before > 0 {
                prepared
                    .replacements
                    .push(encode_part(directory, row, &pcm, 0, before)?);
            }
            if after < row.frames {
                prepared
                    .replacements
                    .push(encode_part(directory, row, &pcm, after, row.frames)?);
            }
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        remove_prepared(directory, &prepared);
        return Err(error);
    }
    Ok(prepared)
}

pub fn remove_prepared(directory: &Path, prepared: &PreparedMedia) {
    for row in &prepared.replacements {
        let _ = fs::remove_file(directory.join(&row.path));
        let _ = fs::remove_file(staging_marker(directory, &row.path));
    }
}

fn staging_marker(directory: &Path, relative: &Path) -> PathBuf {
    let mut marker = directory.join(relative);
    marker.set_extension("cleanup-staged");
    marker
}

pub fn clear_staging_markers(directory: &Path, prepared: &PreparedMedia) {
    for row in &prepared.replacements {
        let _ = fs::remove_file(staging_marker(directory, &row.path));
    }
}

pub fn recover_staging(db: &Connection, directory: &Path) -> Result<(), String> {
    for source in ["microphone", "system"] {
        let media_dir = directory.join("media").join(source);
        let entries = match fs::read_dir(&media_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".cleanup-staged") else {
                continue;
            };
            if Uuid::parse_str(id).is_err() {
                continue;
            }
            let relative = format!("media/{source}/{id}.opus");
            let committed: i64 = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path=?1)",
                    params![relative],
                    |r| r.get(0),
                )
                .map_err(|error| error.to_string())?;
            match fs::remove_file(media_dir.join(format!("{id}.pending"))) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            if committed == 0 {
                match fs::remove_file(media_dir.join(format!("{id}.opus"))) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.to_string()),
                }
                db.execute(
                    "DELETE FROM orphan_media WHERE media_path=?1",
                    params![relative],
                )
                .map_err(|error| error.to_string())?;
            }
            fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn apply(
    tx: &Transaction<'_>,
    scope: &PreviewScope,
    prepared: &PreparedMedia,
    now: i64,
) -> Result<(u64, u64), String> {
    if selection_signature(tx, scope.start, scope.end, scope.mask)? != scope.signature {
        return Err("cleanup preview changed; review again".to_owned());
    }
    let clear_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO clear_ranges VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            clear_id,
            scope.start,
            scope.end,
            scope.mask,
            mode_name(scope.mode),
            now
        ],
    )
    .map_err(|error| error.to_string())?;
    for row in &prepared.originals {
        let updated = tx
            .execute(
                "UPDATE audio_blocks SET media_path=NULL,bytes=0,
          retention_decision=?3,analysis_state='cleared',
          speech_active=CASE WHEN ?4='all_data' THEN 0 ELSE speech_active END
          WHERE id=?1 AND media_path=?2",
                params![
                    row.id,
                    row.path.to_string_lossy(),
                    mode_name(scope.mode),
                    mode_name(scope.mode)
                ],
            )
            .map_err(|error| error.to_string())?;
        if updated != 1 {
            return Err("audio manifest changed during cleanup".to_owned());
        }
        tx.execute(
            "INSERT OR IGNORE INTO media_deletion_jobs VALUES (?1,?2,NULL,NULL)",
            params![row.path.to_string_lossy(), now],
        )
        .map_err(|error| error.to_string())?;
    }
    if scope.mode == CleanupMode::AllData {
        crate::moments::clear_all_data(tx, scope.start, scope.end, now)?;
        let gaps = {
            let mut statement = tx
                .prepare(
                    "SELECT id,source,started_ms,ended_ms,reason FROM media_gaps
			 WHERE started_ms<?2 AND coalesce(ended_ms,9223372036854775807)>?1
			 AND ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
                )
                .map_err(|error| error.to_string())?;
            statement
                .query_map(params![scope.start, scope.end, scope.mask], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        for (id, source, start, maybe_end, reason) in gaps {
            let end = maybe_end.unwrap_or(i64::MAX);
            tx.execute("DELETE FROM media_gaps WHERE id=?1", params![id])
                .map_err(|error| error.to_string())?;
            if start < scope.start {
                tx.execute(
                    "INSERT INTO media_gaps VALUES (?1,?2,?3,?4,?5)",
                    params![
                        Uuid::new_v4().to_string(),
                        source,
                        start,
                        scope.start,
                        reason
                    ],
                )
                .map_err(|error| error.to_string())?;
            }
            if end > scope.end {
                let tail_start = if maybe_end.is_none() {
                    scope.end.min(now)
                } else {
                    scope.end
                };
                tx.execute(
                    "INSERT INTO media_gaps VALUES (?1,?2,?3,?4,?5)",
                    params![
                        Uuid::new_v4().to_string(),
                        source,
                        tail_start,
                        maybe_end,
                        reason
                    ],
                )
                .map_err(|error| error.to_string())?;
            }
        }
        // A previous raw-audio clear leaves a speech-bearing manifest with no
        // file. Split its metadata at this selection, then retire the parent;
        // otherwise a later candidate scan could resurrect the erased voice.
        let mut stmt = tx
            .prepare(
                "SELECT id,session_id,source,started_ms,ended_ms,
          sample_start,frames,speech_active FROM audio_blocks
          WHERE media_path IS NULL AND retention_decision!='all_data'
          AND started_ms<?2 AND ended_ms>?1 AND
          ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![scope.start, scope.end, scope.mask], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(stmt);
        for (id, session, source, start, end, sample_start, frames, speech) in rows {
            if end <= start || frames < 0 {
                return Err("audio metadata interval invalid".into());
            }
            let cut_start = scope.start.max(start).min(end);
            let cut_end = scope.end.max(start).min(end);
            let cut_start_frame = (cut_start - start) * frames / (end - start);
            let cut_end_frame = (cut_end - start) * frames / (end - start);
            for (a, b, offset, length) in [
                (start, cut_start, 0, cut_start_frame),
                (cut_end, end, cut_end_frame, frames - cut_end_frame),
            ] {
                if b > a && length > 0 {
                    tx.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,
                      sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
                      VALUES (?1,?2,?3,?4,?5,?6,?7,0,NULL,'raw_audio','cleared',?8)",
                      params![Uuid::new_v4().to_string(),session,source,a,b,
                        sample_start+offset,length,speech])
                      .map_err(|error|error.to_string())?;
                }
            }
            tx.execute(
                "UPDATE audio_blocks SET retention_decision='all_data',
              analysis_state='cleared',speech_active=0 WHERE id=?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    for row in &prepared.replacements {
        tx.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,
          sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'retain','complete',?10)",
            params![
                row.id.to_string(),
                row.session_id,
                row.source,
                row.start,
                row.end,
                row.sample_start,
                row.frames as i64,
                row.bytes as i64,
                row.path.to_string_lossy(),
                row.speech_active
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    let source_filter = "(source IS NULL OR (source='microphone' AND ?3&1!=0)
      OR (source='system' AND ?3&2!=0))";
    if scope.mode == CleanupMode::RawAudio {
        tx.execute(
            &format!(
                "UPDATE cloud_tasks SET state='superseded',updated_ms=?4,
          error_code='audio_cleared' WHERE provider='asr' AND started_ms<?2
          AND ended_ms>?1 AND {source_filter}
          AND submit_attempts=0 AND state IN ('pending','processing','failed')"
            ),
            params![scope.start, scope.end, scope.mask, now],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "UPDATE voice_observations SET state='unavailable',error_code='audio_unavailable',
          updated_ms=?4 WHERE state IN ('pending','processing') AND started_ms<?2 AND ended_ms>?1
          AND ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
            params![scope.start, scope.end, scope.mask, now],
        )
        .map_err(|error| error.to_string())?;
    } else {
        let affected_speaker_targets = {
            let mut stmt = tx
                .prepare(
                    "SELECT DISTINCT t.target_kind,t.target_id,t.target_version
                 FROM precise_segments p JOIN cloud_tasks t ON t.id=p.task_id
                 WHERE p.started_ms<?2 AND p.ended_ms>?1 AND
                 ((p.source='microphone' AND ?3&1!=0)
                   OR (p.source='system' AND ?3&2!=0))",
                )
                .map_err(|error| error.to_string())?;
            stmt.query_map(params![scope.start, scope.end, scope.mask], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
        };
        let affected_people = {
            let mut stmt = tx
                .prepare(
                    "SELECT DISTINCT person_id FROM (
              SELECT person_id,source,started_ms,ended_ms FROM speaker_bindings
              UNION ALL SELECT person_id,source,started_ms,ended_ms FROM voice_samples)
              WHERE started_ms<?2 AND ended_ms>?1 AND
              ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
                )
                .map_err(|error| error.to_string())?;
            stmt.query_map(params![scope.start, scope.end, scope.mask], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
        };
        tx.execute(
            &format!(
                "UPDATE cloud_tasks SET state='superseded',updated_ms=?4,
          error_code='data_cleared'
          WHERE started_ms<?2 AND ended_ms>?1 AND {source_filter}
          AND state IN ('pending','processing','submitted','failed')"
            ),
            params![scope.start, scope.end, scope.mask, now],
        )
        .map_err(|error| error.to_string())?;
        for table in [
            "text_corrections",
            "speaker_bindings",
            "voice_samples",
            "voice_observations",
            "precise_segments",
            "coarse_segments",
        ] {
            let sql = format!(
                "DELETE FROM {table} WHERE started_ms<?2 AND ended_ms>?1
              AND ((source='microphone' AND ?3&1!=0)
                OR (source='system' AND ?3&2!=0))"
            );
            tx.execute(&sql, params![scope.start, scope.end, scope.mask])
                .map_err(|error| error.to_string())?;
        }
        for (kind, id, version) in affected_speaker_targets {
            crate::knowledge::rebuild_speaker_groups(tx, &kind, &id, &version, now)?;
        }
        tx.execute(
            "DELETE FROM speaker_groups WHERE NOT EXISTS (
               SELECT 1 FROM precise_segments p WHERE p.speaker_scope=speaker_groups.speaker_scope
                 AND p.speaker_label=speaker_groups.speaker_label)",
            [],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "DELETE FROM denied_matches WHERE NOT EXISTS (
          SELECT 1 FROM precise_segments p WHERE p.id=denied_matches.segment_id)",
            [],
        )
        .map_err(|error| error.to_string())?;
        for person_id in affected_people {
            let active_samples: i64 = tx
                .query_row(
                    "SELECT count(*) FROM voice_samples
              WHERE person_id=?1 AND revoked_ms IS NULL",
                    params![person_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if active_samples == 0 {
                tx.execute(
                    "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,
                  updated_ms=?2 WHERE person_id=?1 AND origin='automatic' AND state='active'",
                    params![person_id, now],
                )
                .map_err(|error| error.to_string())?;
            }
            let active_bindings: i64 = tx
                .query_row(
                    "SELECT count(*) FROM speaker_bindings
              WHERE person_id=?1 AND state IN ('active','pending_review')",
                    params![person_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if active_samples == 0 && active_bindings == 0 {
                tx.execute(
                    "DELETE FROM speaker_bindings WHERE person_id=?1",
                    params![person_id],
                )
                .map_err(|error| error.to_string())?;
                tx.execute(
                    "DELETE FROM denied_matches WHERE person_id=?1",
                    params![person_id],
                )
                .map_err(|error| error.to_string())?;
                tx.execute("DELETE FROM people WHERE id=?1", params![person_id])
                    .map_err(|error| error.to_string())?;
            }
        }
        tx.execute(
            "UPDATE named_events SET deleted_ms=?4 WHERE deleted_ms IS NULL
          AND started_ms>=?1 AND ended_ms<=?2 AND source_mask&?3=source_mask",
            params![scope.start, scope.end, scope.mask, now],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "DELETE FROM event_candidate_links WHERE event_id IN (
          SELECT id FROM named_events WHERE deleted_ms IS NOT NULL)",
            [],
        )
        .map_err(|error| error.to_string())?;
        if scope.start == 0 && scope.end == i64::MAX && scope.mask == 3 {
            tx.execute("DELETE FROM voice_samples", [])
                .map_err(|error| error.to_string())?;
            tx.execute("DELETE FROM speaker_bindings", [])
                .map_err(|error| error.to_string())?;
            tx.execute("DELETE FROM people", [])
                .map_err(|error| error.to_string())?;
        }
        tx.execute(
            "UPDATE identity_state SET generation=generation+1 WHERE id=1",
            [],
        )
        .map_err(|error| error.to_string())?;
        // Completed transcripts outside the selected interval retain their
        // original task version. The selected sentences are deleted above;
        // requeuing a whole event would upload material outside the user's
        // chosen range and could duplicate still-valid sentences.
        tx.execute(
            "UPDATE candidate_scan_state SET from_ms=min(from_ms,max(0,?1-1800000))
          WHERE id=1",
            params![scope.start],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.execute(
        "DELETE FROM cleanup_previews WHERE token=?1",
        params![scope.token.to_string()],
    )
    .map_err(|error| error.to_string())?;
    let pending: i64 = tx
        .query_row(
            "SELECT count(*) FROM media_deletion_jobs
      WHERE completed_ms IS NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok((prepared.originals.len() as u64, pending as u64))
}

pub fn drain_deletions(db: &Connection, directory: &Path, now: i64) -> Result<u64, String> {
    let mut completed = 0;
    let mut after = String::new();
    loop {
        let jobs = {
            let mut stmt = db
                .prepare(
                    "SELECT media_path FROM media_deletion_jobs
               WHERE completed_ms IS NULL AND media_path>?1 ORDER BY media_path LIMIT 1000",
                )
                .map_err(|error| error.to_string())?;
            stmt.query_map(params![after], |r| r.get::<_, String>(0))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        if jobs.is_empty() {
            break;
        }
        after = jobs.last().unwrap().clone();
        for job in jobs {
            let relative = PathBuf::from(&job);
            if !valid_media_path(&relative, "microphone")
                && !valid_media_path(&relative, "system")
                && !crate::moments::valid_screenshot_path(&relative)
            {
                return Err("invalid pending media deletion path".to_owned());
            }
            let path = directory.join(&relative);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    db.execute(
                        "UPDATE media_deletion_jobs SET error_code='unlink_failed'
                  WHERE media_path=?1",
                        params![job],
                    )
                    .map_err(|error| error.to_string())?;
                    continue;
                }
            }
            db.execute(
                "UPDATE media_deletion_jobs SET completed_ms=?2,error_code=NULL
          WHERE media_path=?1",
                params![job, now],
            )
            .map_err(|error| error.to_string())?;
            db.execute("DELETE FROM orphan_media WHERE media_path=?1", params![job])
                .map_err(|error| error.to_string())?;
            completed += 1;
        }
    }
    Ok(completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_data_clear_trims_gap_history_without_erasing_other_source() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        for (id, source) in [("mic", "microphone"), ("sys", "system")] {
            store
                .db
                .execute(
                    "INSERT INTO media_gaps VALUES (?1,?2,1000,5000,'source_failure')",
                    params![id, source],
                )
                .unwrap();
        }
        let preview = preview(
            &store.db,
            2000,
            4000,
            EventSources::Microphone,
            CleanupMode::AllData,
            10_000,
        )
        .unwrap();
        let scope = verified_scope(&store.db, preview.token, true, 10_000).unwrap();
        let prepared = prepare_media(
            directory.path(),
            &scope,
            originals(&store.db, &scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        apply(&tx, &scope, &prepared, 10_001).unwrap();
        tx.commit().unwrap();
        let gaps = crate::events::timeline_gaps(&store.db, 0, 6_000, 10_000).unwrap();
        assert!(
            gaps.iter()
                .any(|gap| gap.source == hear_protocol::AudioSource::Microphone
                    && gap.started_ms == 1_000
                    && gap.ended_ms == 2_000)
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.source == hear_protocol::AudioSource::Microphone
                    && gap.started_ms == 4_000
                    && gap.ended_ms == 5_000)
        );
        assert!(
            gaps.iter()
                .any(|gap| gap.source == hear_protocol::AudioSource::System
                    && gap.started_ms == 1_000
                    && gap.ended_ms == 5_000)
        );
        assert!(
            !gaps
                .iter()
                .any(|gap| gap.source == hear_protocol::AudioSource::Microphone
                    && gap.started_ms < 4_000
                    && gap.ended_ms > 2_000)
        );
    }

    #[test]
    fn crash_before_cleanup_commit_removes_only_staged_audio() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let original = audio(&store.db, directory.path(), "microphone");
        let preview = preview(
            &store.db,
            500,
            1500,
            EventSources::Microphone,
            CleanupMode::RawAudio,
            10000,
        )
        .unwrap();
        let scope = verified_scope(&store.db, preview.token, false, 10000).unwrap();
        let prepared = prepare_media(
            directory.path(),
            &scope,
            originals(&store.db, &scope).unwrap(),
        )
        .unwrap();
        assert_eq!(prepared.replacements.len(), 2);
        let staged: Vec<_> = prepared
            .replacements
            .iter()
            .map(|row| directory.path().join(&row.path))
            .collect();
        assert!(staged.iter().all(|path| path.exists()));
        let pending = directory
            .path()
            .join("media/microphone")
            .join(format!("{}.pending", prepared.replacements[0].id));
        fs::write(&pending, b"unfinished").unwrap();
        recover_staging(&store.db, directory.path()).unwrap();
        assert!(staged.iter().all(|path| !path.exists()));
        assert!(!pending.exists());
        assert!(
            directory
                .path()
                .join(format!("media/microphone/{original}.opus"))
                .exists()
        );
    }

    #[test]
    fn deletion_drain_clears_more_than_one_batch() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let media_dir = directory.path().join("media/microphone");
        fs::create_dir_all(&media_dir).unwrap();
        for _ in 0..1001 {
            let id = Uuid::new_v4();
            fs::write(media_dir.join(format!("{id}.opus")), b"x").unwrap();
            db.execute(
                "INSERT INTO media_deletion_jobs VALUES (?1,1,NULL,NULL)",
                params![format!("media/microphone/{id}.opus")],
            )
            .unwrap();
        }
        assert_eq!(drain_deletions(&db, directory.path(), 2).unwrap(), 1001);
        assert_eq!(fs::read_dir(media_dir).unwrap().count(), 0);
    }

    fn audio(db: &Connection, directory: &Path, source: &str) -> Uuid {
        let id = Uuid::new_v4();
        let media_dir = directory.join("media").join(source);
        let mut block = crate::media::OpusBlock::create(&media_dir, id).unwrap();
        let pcm = (0..96_000)
            .map(|sample| ((sample as f32) * 0.015).sin() * 0.1)
            .collect::<Vec<_>>();
        block.push(&pcm).unwrap();
        let sealed = block.seal().unwrap();
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,
          sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES (?1,'session',?2,0,2000,0,96000,?3,?4,'retain','complete',1)",
            params![
                id.to_string(),
                source,
                sealed.bytes as i64,
                format!("media/{source}/{id}.opus")
            ],
        )
        .unwrap();
        id
    }

    #[test]
    fn partial_all_data_clear_trims_only_selected_source_and_removes_selected_text() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let mic = audio(&store.db, directory.path(), "microphone");
        let sys = audio(&store.db, directory.path(), "system");
        store
            .db
            .execute(
                "INSERT INTO coarse_segments VALUES
          ('before','block','session','microphone',100,400,'before','test'),
          ('middle','block','session','microphone',700,900,'middle','test'),
          ('system','block','session','system',700,900,'other source','test')",
                [],
            )
            .unwrap();
        let preview = preview(
            &store.db,
            500,
            1500,
            EventSources::Microphone,
            CleanupMode::AllData,
            10000,
        )
        .unwrap();
        assert_eq!(preview.audio_blocks, 1);
        assert!(verified_scope(&store.db, preview.token, false, 10000).is_err());
        let scope = verified_scope(&store.db, preview.token, true, 10000).unwrap();
        let originals = originals(&store.db, &scope).unwrap();
        let prepared = prepare_media(directory.path(), &scope, originals).unwrap();
        assert_eq!(prepared.replacements.len(), 2);
        let tx = store.db.transaction().unwrap();
        apply(&tx, &scope, &prepared, 10001).unwrap();
        tx.commit().unwrap();
        drain_deletions(&store.db, directory.path(), 10002).unwrap();
        let mic_path: Option<String> = store
            .db
            .query_row(
                "SELECT media_path FROM audio_blocks WHERE id=?1",
                params![mic.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(mic_path.is_none());
        let sys_path: String = store
            .db
            .query_row(
                "SELECT media_path FROM audio_blocks WHERE id=?1",
                params![sys.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(directory.path().join(sys_path).is_file());
        for part in &prepared.replacements {
            assert_eq!(part.frames, 24_000);
            assert_eq!(
                crate::media::decode_block(&directory.path().join(&part.path), part.frames)
                    .unwrap()
                    .len(),
                24_000
            );
        }
        let text: Vec<String> = {
            let mut stmt = store
                .db
                .prepare("SELECT text FROM coarse_segments ORDER BY text")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(text, ["before", "other source"]);
        let old_file = directory
            .path()
            .join(format!("media/microphone/{mic}.opus"));
        assert!(!old_file.exists());
    }

    #[test]
    fn raw_audio_clear_preserves_text_and_marks_original_unplayable() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let mic = audio(&store.db, directory.path(), "microphone");
        store
            .db
            .execute(
                "INSERT INTO coarse_segments VALUES
          ('middle','block','session','microphone',700,900,'keep me','test')",
                [],
            )
            .unwrap();
        let preview = preview(
            &store.db,
            0,
            2000,
            EventSources::Microphone,
            CleanupMode::RawAudio,
            10000,
        )
        .unwrap();
        assert!(preview.voice_features_retained);
        let scope = verified_scope(&store.db, preview.token, false, 10000).unwrap();
        let prepared = prepare_media(
            directory.path(),
            &scope,
            originals(&store.db, &scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        apply(&tx, &scope, &prepared, 10001).unwrap();
        tx.commit().unwrap();
        let retained: i64 = store
            .db
            .query_row(
                "SELECT count(*) FROM coarse_segments WHERE text='keep me'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, 1);
        let (path, decision): (Option<String>, String) = store
            .db
            .query_row(
                "SELECT media_path,retention_decision FROM audio_blocks WHERE id=?1",
                params![mic.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(path.is_none());
        assert_eq!(decision, "raw_audio");
    }

    #[test]
    fn all_data_clear_rejects_an_in_flight_asr_result() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        audio(&store.db, directory.path(), "microphone");
        let range = Uuid::new_v4();
        store
            .db
            .execute(
                "INSERT INTO manual_ranges VALUES (?1,0,2000,1,1)",
                params![range.to_string()],
            )
            .unwrap();
        let tx = store.db.transaction().unwrap();
        crate::events::queue_asr(
            &tx,
            "range",
            &range.to_string(),
            "1",
            0,
            2000,
            1,
            true,
            1000,
        )
        .unwrap();
        tx.commit().unwrap();
        let task_id: String = store
            .db
            .query_row("SELECT id FROM cloud_tasks LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task = crate::events::CloudTask {
            id: Uuid::parse_str(&task_id).unwrap(),
            provider: hear_protocol::CloudProvider::Asr,
            target_kind: "range".into(),
            target_id: range.to_string(),
            target_version: "1".into(),
            model_id: "bigmodel".into(),
            start: 0,
            end: 2000,
            source: Some("microphone".into()),
            remote_id: Uuid::new_v4(),
            attempts: 1,
            submit_attempts: 1,
            updated_ms: 1000,
        };
        let preview = preview(
            &store.db,
            0,
            2000,
            EventSources::Microphone,
            CleanupMode::AllData,
            10000,
        )
        .unwrap();
        let scope = verified_scope(&store.db, preview.token, true, 10000).unwrap();
        let prepared = prepare_media(
            directory.path(),
            &scope,
            originals(&store.db, &scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        apply(&tx, &scope, &prepared, 10001).unwrap();
        tx.commit().unwrap();
        assert!(!crate::events::task_current(&store.db, &task).unwrap());
        let utterance = crate::cloud::AsrUtterance {
            start_ms: 500,
            end_ms: 1000,
            text: "should stay erased".into(),
            speaker: None,
        };
        assert_eq!(
            crate::events::publish_asr(&mut store.db, &task, &[utterance], 10002).unwrap(),
            0
        );
        let count: i64 = store
            .db
            .query_row("SELECT count(*) FROM precise_segments", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn raw_clear_keeps_an_already_submitted_non_audio_asr_result() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        audio(&store.db, directory.path(), "microphone");
        let range = Uuid::new_v4();
        store
            .db
            .execute(
                "INSERT INTO manual_ranges VALUES (?1,0,2000,1,1)",
                params![range.to_string()],
            )
            .unwrap();
        let tx = store.db.transaction().unwrap();
        crate::events::queue_asr(
            &tx,
            "range",
            &range.to_string(),
            "1",
            0,
            2000,
            1,
            true,
            1000,
        )
        .unwrap();
        tx.commit().unwrap();
        store
            .db
            .execute(
                "UPDATE cloud_tasks SET submit_attempts=1,state='submitted'",
                [],
            )
            .unwrap();
        let task_id: String = store
            .db
            .query_row("SELECT id FROM cloud_tasks LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task = crate::events::CloudTask {
            id: Uuid::parse_str(&task_id).unwrap(),
            provider: hear_protocol::CloudProvider::Asr,
            target_kind: "range".into(),
            target_id: range.to_string(),
            target_version: "1".into(),
            model_id: "bigmodel".into(),
            start: 0,
            end: 2000,
            source: Some("microphone".into()),
            remote_id: Uuid::new_v4(),
            attempts: 1,
            submit_attempts: 1,
            updated_ms: 1000,
        };
        let preview = preview(
            &store.db,
            0,
            2000,
            EventSources::Microphone,
            CleanupMode::RawAudio,
            10000,
        )
        .unwrap();
        let scope = verified_scope(&store.db, preview.token, false, 10000).unwrap();
        let prepared = prepare_media(
            directory.path(),
            &scope,
            originals(&store.db, &scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        apply(&tx, &scope, &prepared, 10001).unwrap();
        tx.commit().unwrap();
        assert!(crate::events::task_current(&store.db, &task).unwrap());
        let utterance = crate::cloud::AsrUtterance {
            start_ms: 500,
            end_ms: 1000,
            text: "already submitted".into(),
            speaker: None,
        };
        assert_eq!(
            crate::events::publish_asr(&mut store.db, &task, &[utterance], 10002).unwrap(),
            1
        );
        let text: String = store
            .db
            .query_row("SELECT text FROM precise_segments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(text, "already submitted");
    }

    #[test]
    fn raw_then_all_data_cannot_resurrect_speech_and_preserves_unselected_tails() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        audio(&store.db, directory.path(), "microphone");
        let raw = preview(
            &store.db,
            0,
            2000,
            EventSources::Microphone,
            CleanupMode::RawAudio,
            10_000,
        )
        .unwrap();
        let raw_scope = verified_scope(&store.db, raw.token, false, 10_000).unwrap();
        let raw_media = prepare_media(
            directory.path(),
            &raw_scope,
            originals(&store.db, &raw_scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        apply(&tx, &raw_scope, &raw_media, 10_001).unwrap();
        tx.commit().unwrap();
        let all = preview(
            &store.db,
            500,
            1500,
            EventSources::Microphone,
            CleanupMode::AllData,
            11_000,
        )
        .unwrap();
        let all_scope = verified_scope(&store.db, all.token, true, 11_000).unwrap();
        let all_media = prepare_media(
            directory.path(),
            &all_scope,
            originals(&store.db, &all_scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        apply(&tx, &all_scope, &all_media, 11_001).unwrap();
        tx.commit().unwrap();
        let intervals: Vec<(i64, i64)> = {
            let mut stmt = store
                .db
                .prepare(
                    "SELECT started_ms,ended_ms FROM audio_blocks
              WHERE speech_active=1 ORDER BY started_ms",
                )
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(intervals, [(0, 500), (1500, 2000)]);
        crate::events::refresh_candidates(&mut store.db, 12_000, true).unwrap();
        let candidate_spans: Vec<(i64, i64)> = {
            let mut stmt = store
                .db
                .prepare(
                    "SELECT started_ms,ended_ms FROM candidate_windows
              ORDER BY started_ms",
                )
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert!(candidate_spans.iter().all(|(a, b)| *b <= 500 || *a >= 1500));
    }
}
