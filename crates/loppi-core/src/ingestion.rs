//! Core-owned recovery journal. Capture helpers own immutable media and journal
//! files; only core validates generations, cleanup and commits business facts.
use crate::{cleanup, media, record, source_name};
use loppi_protocol::AudioSource;
use loppi_protocol::platform::OpenOptions;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

const MAX_ENTRY_BYTES: u64 = 16_384;
pub const MAX_SESSION_ENTRIES: usize = 64;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: Uuid,
    pub run_id: Uuid,
    pub source: AudioSource,
    pub wall_anchor_ms: i64,
    pub monotonic_anchor_ns: i64,
    pub generation: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    version: u32,
    pub id: Uuid,
    pub session: Session,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub kind: Kind,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    Block {
        sample_start: u64,
        frames: usize,
        digital_silence: bool,
        bytes: Option<u64>,
        sha256: Option<String>,
    },
    Gap {
        reason: String,
    },
}

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS ingestion_state(id INTEGER PRIMARY KEY CHECK(id=1),generation INTEGER NOT NULL);
      INSERT OR IGNORE INTO ingestion_state VALUES(1,0);
      CREATE TABLE IF NOT EXISTS session_generation(session_id TEXT PRIMARY KEY,generation INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS ingestion_pending_sessions(session_id TEXT PRIMARY KEY);
      CREATE TABLE IF NOT EXISTS ingestion_receipts(id TEXT PRIMARY KEY,session_id TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS ingestion_clears(clear_id TEXT PRIMARY KEY,generation INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS ingestion_clears_generation ON ingestion_clears(generation,clear_id);
      CREATE TRIGGER IF NOT EXISTS clear_ingestion_generation AFTER INSERT ON clear_ranges BEGIN
        INSERT INTO ingestion_clears VALUES(new.id,(SELECT generation FROM ingestion_state WHERE id=1));
        UPDATE ingestion_state SET generation=generation+1 WHERE id=1;
      END;").map_err(|e|e.to_string())?;
    crate::storage::migrate(db, "session_tail_gaps_v1", "
      ALTER TABLE media_gaps ADD COLUMN tail_session_id TEXT;
      CREATE INDEX media_gaps_tail_session ON media_gaps(tail_session_id) WHERE tail_session_id IS NOT NULL;
      UPDATE media_gaps SET tail_session_id=id WHERE reason IN ('source_failure','interrupted_tail')
        AND EXISTS(SELECT 1 FROM recording_sessions WHERE id=media_gaps.id);")?;
    crate::storage::migrate(
        db,
        "ingested_gap_owner_v1",
        "
      ALTER TABLE media_gaps ADD COLUMN source_session_id TEXT;
      CREATE INDEX media_gaps_source_session ON media_gaps(source_session_id,started_ms)
        WHERE source_session_id IS NOT NULL;
      CREATE INDEX audio_blocks_session_pending ON audio_blocks(session_id,sample_start)
        WHERE analysis_state='pending' AND analysis_attempts<3 AND media_path IS NOT NULL;",
    )?;
    // Installed versions wrote stop intent before the final block arrived.
    // Associate only unambiguous, physically evidenced overlap; no new gaps.
    crate::storage::migrate(db, "intent_gap_confirmed_audio_v1", "
      UPDATE media_gaps AS g SET source_session_id=(
        SELECT min(b.session_id) FROM audio_blocks b JOIN recording_sessions s ON s.id=b.session_id
        WHERE b.source=g.source AND s.source=g.source AND b.started_ms<=g.started_ms AND b.ended_ms>g.started_ms)
      WHERE source_session_id IS NULL
        AND reason IN ('manual_pause','schedule_pause','storage_limit','source_disabled')
        AND (SELECT count(DISTINCT b.session_id) FROM audio_blocks b JOIN recording_sessions s ON s.id=b.session_id
          WHERE b.source=g.source AND s.source=g.source AND b.started_ms<=g.started_ms AND b.ended_ms>g.started_ms)=1;
      UPDATE media_gaps AS g SET started_ms=max(started_ms,min(coalesce(ended_ms,
        (SELECT last_confirmed_ms FROM recording_sessions WHERE id=g.source_session_id)),
        (SELECT last_confirmed_ms FROM recording_sessions WHERE id=g.source_session_id)))
      WHERE reason IN ('manual_pause','schedule_pause','storage_limit','source_disabled')
        AND EXISTS(SELECT 1 FROM recording_sessions s WHERE s.id=g.source_session_id AND s.source=g.source);
      DELETE FROM media_gaps WHERE reason IN ('manual_pause','schedule_pause','storage_limit','source_disabled')
        AND ended_ms<=started_ms;
    ")
}

/// Lifecycle gaps belong to the session, including after cleanup splits them.
/// The DB owner applies the same source/generation tombstones as late journals.
pub fn record_tail_gap(db: &Connection, id: &str, reason: &str) -> Result<(), String> {
    let row = db
        .query_row(
            "SELECT s.source,s.last_confirmed_ms,coalesce(g.generation,0)
      FROM recording_sessions s LEFT JOIN session_generation g ON g.session_id=s.id WHERE s.id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((source, start, generation)) = row else {
        return Ok(());
    };
    let mask = if source == "microphone" { 1 } else { 2 };
    let cuts = cleared_ranges(db, generation, mask, start, i64::MAX, true)?;
    let mut ranges = vec![(start, i64::MAX)];
    for (from, to) in cuts {
        ranges = ranges
            .into_iter()
            .flat_map(|(a, b)| {
                [(a, from.min(b)), (to.max(a), b)]
                    .into_iter()
                    .filter(|(a, b)| a < b)
            })
            .collect();
    }
    db.execute("DELETE FROM media_gaps WHERE tail_session_id=?1", [id])
        .map_err(|e| e.to_string())?;
    for (index, (from, to)) in ranges.into_iter().enumerate() {
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason,tail_session_id)
          VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                if index == 0 {
                    id.to_owned()
                } else {
                    Uuid::new_v4().to_string()
                },
                source,
                from,
                if to == i64::MAX { None } else { Some(to) },
                reason,
                id
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn session(db: &Connection, id: Uuid) -> Result<Session, String> {
    db.query_row(
        "SELECT c.run_id,s.source,c.wall_anchor_ms,c.monotonic_anchor_ns,g.generation
      FROM session_clock c JOIN recording_sessions s ON s.id=c.session_id
      JOIN session_generation g ON g.session_id=s.id WHERE s.id=?1",
        [id.to_string()],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        },
    )
    .map_err(|e| e.to_string())
    .and_then(|(run, source, wall, mono, generation)| {
        Ok(Session {
            id,
            run_id: Uuid::parse_str(&run).map_err(|e| e.to_string())?,
            source: match source.as_str() {
                "microphone" => AudioSource::Microphone,
                "system" => AudioSource::System,
                _ => return Err("invalid session source".into()),
            },
            wall_anchor_ms: wall,
            monotonic_anchor_ns: mono,
            generation,
        })
    })
}

pub struct Journal {
    root: PathBuf,
    session: Session,
}

fn sync_dir(path: &Path) -> Result<(), String> {
    loppi_protocol::platform::sync_directory(path).map_err(|e| e.to_string())
}

impl Journal {
    pub fn new(directory: &Path, session: Session) -> Result<Self, String> {
        let root = directory.join("ingestion").join(session.id.to_string());
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        sync_dir(&directory.join("ingestion"))?;
        sync_dir(directory)?;
        Ok(Self { root, session })
    }

    pub fn persist(&mut self, event: record::RecordEvent) -> Result<(), String> {
        let (entry, prepared) = match event {
            record::RecordEvent::Block(block) => {
                if block.source != self.session.source {
                    return Err("ingestion source mismatch".into());
                }
                let (bytes, sha256) = if let Some(media) = &block.media {
                    if media.samples != block.frames as u64
                        || media.path.file_stem().and_then(|s| s.to_str())
                            != Some(block.id.to_string().as_str())
                    {
                        return Err("sealed media differs from manifest".into());
                    }
                    (Some(media.bytes), Some(hash_file(&media.path)?))
                } else {
                    (None, None)
                };
                (
                    Entry {
                        version: 1,
                        id: block.id,
                        session: self.session.clone(),
                        started_ms: block.started_ms,
                        ended_ms: block.ended_ms,
                        kind: Kind::Block {
                            sample_start: block.sample_start,
                            frames: block.frames,
                            digital_silence: block.decision == "discard_digital_silence_v1",
                            bytes,
                            sha256,
                        },
                    },
                    block.preparing,
                )
            }
            record::RecordEvent::Gap(gap) => {
                if gap.source != self.session.source {
                    return Err("ingestion source mismatch".into());
                }
                (
                    Entry {
                        version: 1,
                        id: Uuid::new_v4(),
                        session: self.session.clone(),
                        started_ms: gap.started_ms,
                        ended_ms: gap.ended_ms,
                        kind: Kind::Gap {
                            reason: gap.reason.into(),
                        },
                    },
                    false,
                )
            }
        };
        entry.validate()?;
        // The core's directory snapshot must be a publication prefix, not a
        // live read_dir view that can omit A while seeing later B/C. This lock
        // covers only the bounded journal namespace, never DB/model work. It
        // is on the directory inode (no extra journal file) and dies with its
        // owning descriptor/process. Capture callbacks never acquire it.
        let publication =
            loppi_protocol::platform::open_directory(&self.root).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match loppi_protocol::platform::try_lock_exclusive(&publication) {
                Ok(()) => break,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(format!("journal publication lock: {e}")),
            }
        }
        let name = entry.id.to_string();
        // Count logical entries, not publication files. Finishing a prepared
        // block must remain possible at capacity; otherwise the last admitted
        // block could never become durable/recoverable.
        let mut ids = std::collections::HashSet::new();
        for file in fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let file = file.map_err(|e| e.to_string())?;
            if file.file_name() == ".publication.lock" {
                continue;
            }
            if let Some(id) = file.path().file_stem().and_then(|n| n.to_str()) {
                ids.insert(id.to_owned());
            }
            if ids.len() >= MAX_SESSION_ENTRIES
                && !self.root.join(format!("{name}.prepared")).exists()
            {
                return Err("ingestion backlog exceeded limit".into());
            }
        }
        let temporary = self.root.join(format!("{name}.writing"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        loppi_protocol::platform::rename(
            &temporary,
            self.root.join(format!(
                "{name}.{}",
                if prepared { "prepared" } else { "ready" }
            )),
        )
        .map_err(|e| e.to_string())?;
        sync_dir(&self.root)?;
        if !prepared {
            match fs::remove_file(self.root.join(format!("{name}.prepared"))) {
                Ok(()) => sync_dir(&self.root)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 10_000_000 {
        return Err("ingestion media too large".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

impl Entry {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.started_ms < 0
            || self.ended_ms < self.started_ms
            || self.session.generation < 0
        {
            return Err("invalid ingestion metadata".into());
        }
        match &self.kind {
            Kind::Block {
                sample_start,
                frames,
                bytes,
                sha256,
                ..
            } => {
                if !(1..=record::RATE * 5).contains(frames)
                    || *sample_start > i64::MAX as u64 - *frames as u64
                    || self.ended_ms - self.started_ms
                        != (*frames as i64 * 1000 / record::RATE as i64)
                    || bytes.is_some_and(|n| n > 10_000_000)
                    || sha256
                        .as_ref()
                        .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    return Err("invalid ingestion block".into());
                }
            }
            Kind::Gap { reason }
                if self.ended_ms <= self.started_ms
                    || !matches!(reason.as_str(), "source_start_gap" | "callback_gap") =>
            {
                return Err("invalid ingestion gap".into());
            }
            _ => {}
        }
        Ok(())
    }

    pub fn media_path(&self) -> PathBuf {
        PathBuf::from("media")
            .join(source_name(self.session.source))
            .join(format!("{}.opus", self.id))
    }
}

pub fn load(path: &Path) -> Result<Entry, String> {
    if fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("journal is a symlink".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ENTRY_BYTES {
        return Err("journal metadata too large".into());
    }
    let entry: Entry = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    entry.validate()?;
    if path.extension().is_some_and(|x| x == "ready")
        && matches!(&entry.kind, Kind::Block { digital_silence: false, bytes, sha256, .. }
            if bytes.is_none() || sha256.is_none())
    {
        return Err("final journal lacks media integrity".into());
    }
    if path.file_stem().and_then(|p| p.to_str()) != Some(entry.id.to_string().as_str())
        || path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|p| p.to_str())
            != Some(entry.session.id.to_string().as_str())
    {
        return Err("journal identity mismatch".into());
    }
    Ok(entry)
}

/// A bounded session journal makes restart independent of the historical media directory.
pub fn pending(
    directory: &Path,
    session_id: Uuid,
    include_prepared: bool,
) -> std::io::Result<Vec<PathBuf>> {
    let root = directory.join("ingestion").join(session_id.to_string());
    let snapshot = match loppi_protocol::platform::open_directory(&root) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    // Yield if a source is publishing; do not wait on its filesystem sync.
    // Only enumerate names under this guard, then release before file reads,
    // decoding, commits and acknowledgement. Each session has one publisher.
    loppi_protocol::platform::try_lock_shared(&snapshot)?;
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|v| v == "ready" || (include_prepared && v == "prepared"))
        {
            paths.push(path);
            if paths.len() >= MAX_SESSION_ENTRIES * 2 {
                break;
            }
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn acknowledged(db: &Connection, entry: &Entry) -> Result<bool, String> {
    let session = session(db, entry.session.id)?;
    if session.run_id != entry.session.run_id
        || session.source != entry.session.source
        || session.wall_anchor_ms != entry.session.wall_anchor_ms
        || session.monotonic_anchor_ns != entry.session.monotonic_anchor_ns
        || session.generation != entry.session.generation
    {
        return Err("stale ingestion session".into());
    }
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ingestion_receipts WHERE id=?1)",
        [entry.id.to_string()],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Caller owns cleanup_gate across planning, media work and commit. Existing
/// cleanup encoders and staging markers keep partial tails crash recoverable.
pub fn surviving_samples(db: &Connection, entry: &Entry) -> Result<Vec<(usize, usize)>, String> {
    let Kind::Block { frames, .. } = entry.kind else {
        return Ok(vec![]);
    };
    let cuts = cleared_intervals(db, entry, false)?;
    let mut ranges = vec![(0, frames)];
    for (from, to) in cuts {
        let from = ((from.saturating_sub(entry.started_ms).max(0) as u64).saturating_mul(48))
            .min(frames as u64) as usize;
        let to = ((to.saturating_sub(entry.started_ms).max(0) as u64).saturating_mul(48))
            .min(frames as u64) as usize;
        ranges = ranges
            .into_iter()
            .flat_map(|(a, b)| {
                [(a, from.min(b)), (to.max(a), b)]
                    .into_iter()
                    .filter(|(a, b)| a < b)
            })
            .collect();
    }
    Ok(ranges)
}

fn cleared_intervals(
    db: &Connection,
    entry: &Entry,
    only_all_data: bool,
) -> Result<Vec<(i64, i64)>, String> {
    let mask = if entry.session.source == AudioSource::Microphone {
        1
    } else {
        2
    };
    cleared_ranges(
        db,
        entry.session.generation,
        mask,
        entry.started_ms,
        entry.ended_ms,
        only_all_data,
    )
}

fn cleared_ranges(
    db: &Connection,
    generation: i64,
    mask: i64,
    start: i64,
    end: i64,
    only_all_data: bool,
) -> Result<Vec<(i64, i64)>, String> {
    let mut stmt = db
        .prepare(
            "SELECT c.from_ms,min(c.to_ms,c.committed_ms) FROM clear_ranges c
      JOIN ingestion_clears g ON g.clear_id=c.id WHERE g.generation>=?1 AND c.source_mask&?2!=0
      AND c.from_ms<?4 AND min(c.to_ms,c.committed_ms)>?3 AND c.from_ms<min(c.to_ms,c.committed_ms) AND (?5=0 OR c.mode='all_data') ORDER BY c.from_ms",
        )
        .map_err(|e| e.to_string())?;
    let cuts = stmt
        .query_map(params![generation, mask, start, end, only_all_data], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    cuts.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn insert_gap(db: &Connection, entry: &Entry, reason: &str) -> Result<(), String> {
    let mut ranges = vec![(entry.started_ms, entry.ended_ms)];
    for (from, to) in cleared_intervals(db, entry, true)? {
        ranges = ranges
            .into_iter()
            .flat_map(|(a, b)| {
                [(a, from.min(b)), (to.max(a), b)]
                    .into_iter()
                    .filter(|(a, b)| a < b)
            })
            .collect();
    }
    for (i, (from, to)) in ranges.into_iter().enumerate() {
        db.execute(
            "INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason,source_session_id) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                format!("{}:{i}", entry.id),
                source_name(entry.session.source),
                from,
                to,
                reason,
                entry.session.id.to_string()
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub struct Prepared {
    pub retained: Vec<cleanup::ReplacementAudio>,
    pub original: PathBuf,
    pub replaced: bool,
}

#[derive(Debug)]
pub enum PrepareError {
    Invalid(String),
    Retry(&'static str),
}

pub fn prepare(
    directory: &Path,
    entry: &Entry,
    ranges: &[(usize, usize)],
) -> Result<Prepared, PrepareError> {
    let original = entry.media_path();
    let Kind::Block {
        sample_start,
        frames,
        digital_silence,
        bytes,
        sha256,
    } = &entry.kind
    else {
        return Ok(Prepared {
            retained: vec![],
            original,
            replaced: false,
        });
    };
    let full = directory.join(&original);
    let (size, pcm) = if *digital_silence {
        (0, vec![])
    } else {
        let metadata = fs::symlink_metadata(&full).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PrepareError::Invalid("sealed media missing".into())
            } else {
                PrepareError::Retry("ingestion_prepare_metadata")
            }
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 10_000_000 {
            return Err(PrepareError::Invalid("invalid ingestion media file".into()));
        }
        let mut data = Vec::with_capacity(metadata.len() as usize);
        File::open(&full)
            .map_err(|_| PrepareError::Retry("ingestion_prepare_open"))?
            .take(10_000_001)
            .read_to_end(&mut data)
            .map_err(|_| PrepareError::Retry("ingestion_prepare_read"))?;
        if data.len() > 10_000_000
            || bytes.is_some_and(|n| n != data.len() as u64)
            || sha256
                .as_ref()
                .is_some_and(|hash| *hash != format!("{:x}", Sha256::digest(&data)))
        {
            return Err(PrepareError::Invalid(
                "ingestion media integrity mismatch".into(),
            ));
        }
        let pcm = media::decode_reader(std::io::Cursor::new(data), *frames)
            .map_err(PrepareError::Invalid)?;
        (metadata.len(), pcm)
    };
    let row = cleanup::OriginalAudio {
        id: entry.id.to_string(),
        session_id: entry.session.id.to_string(),
        source: source_name(entry.session.source).into(),
        start: entry.started_ms,
        end: entry.ended_ms,
        sample_start: *sample_start as i64,
        frames: *frames,
        bytes: size as i64,
        path: original.clone(),
        speech_active: None,
    };
    let replaced = ranges != [(0, *frames)];
    let mut retained = Vec::new();
    for &(from, to) in ranges {
        if replaced && !digital_silence {
            match cleanup::encode_part(directory, &row, &pcm, from, to) {
                Ok(part) => retained.push(part),
                Err(_) => {
                    discard_prepared(
                        directory,
                        &Prepared {
                            retained,
                            original,
                            replaced,
                        },
                    );
                    return Err(PrepareError::Retry("ingestion_prepare_encode"));
                }
            }
        } else {
            retained.push(cleanup::ReplacementAudio {
                id: if replaced { Uuid::new_v4() } else { entry.id },
                session_id: row.session_id.clone(),
                source: row.source.clone(),
                start: row.start + from as i64 / 48,
                end: if to == *frames {
                    row.end
                } else {
                    row.start + to as i64 / 48
                },
                sample_start: row.sample_start + from as i64,
                frames: to - from,
                bytes: size,
                path: original.clone(),
                speech_active: None,
            });
        }
    }
    Ok(Prepared {
        retained,
        original,
        replaced,
    })
}

pub fn discard_prepared(directory: &Path, prepared: &Prepared) {
    if prepared.replaced {
        for row in &prepared.retained {
            if row.path == prepared.original {
                continue;
            }
            let _ = fs::remove_file(directory.join(&row.path));
            let _ = fs::remove_file(directory.join(&row.path).with_extension("cleanup-staged"));
        }
    }
}

fn settle_intent_gaps(db: &Connection, entry: &Entry) -> Result<(), String> {
    let session = entry.session.id.to_string();
    // An old unowned stop gap may predate an upgrade's pending final journal.
    // Bind it only once that session's actual block covers its start.
    db.execute(
        "UPDATE media_gaps AS g SET source_session_id=?1
      WHERE source_session_id IS NULL AND source=?2 AND started_ms>=?3 AND started_ms<?4
        AND reason IN ('manual_pause','schedule_pause','storage_limit','source_disabled')
        AND (SELECT count(DISTINCT b.session_id) FROM audio_blocks b
          WHERE b.source=g.source AND b.started_ms<=g.started_ms AND b.ended_ms>g.started_ms)=1
        AND EXISTS(SELECT 1 FROM audio_blocks b WHERE b.session_id=?1
          AND b.started_ms<=g.started_ms AND b.ended_ms>g.started_ms)",
        params![
            session,
            source_name(entry.session.source),
            entry.started_ms,
            entry.ended_ms
        ],
    )
    .map_err(|e| e.to_string())?;
    db.execute(
        "UPDATE media_gaps SET started_ms=max(started_ms,min(coalesce(ended_ms,
        (SELECT last_confirmed_ms FROM recording_sessions WHERE id=?1)),
        (SELECT last_confirmed_ms FROM recording_sessions WHERE id=?1)))
      WHERE source_session_id=?1 AND source=(SELECT source FROM recording_sessions WHERE id=?1)
        AND reason IN ('manual_pause','schedule_pause','storage_limit','source_disabled')",
        [&session],
    )
    .map_err(|e| e.to_string())?;
    db.execute("DELETE FROM media_gaps WHERE source_session_id=?1
      AND reason IN ('manual_pause','schedule_pause','storage_limit','source_disabled') AND ended_ms<=started_ms", [&session])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn commit(db: &mut Connection, entry: &Entry, prepared: &Prepared) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    if acknowledged(&tx, entry)? {
        return Ok(());
    }
    match &entry.kind {
        Kind::Block {
            digital_silence, ..
        } => {
            for row in &prepared.retained {
                tx.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state)
                  VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![row.id.to_string(),row.session_id,row.source,row.start,row.end,row.sample_start,row.frames as i64,row.bytes as i64,
                    if *digital_silence{None}else{Some(loppi_protocol::platform::relative_key(&row.path))},
                    if *digital_silence{"discard_digital_silence_v1"}else{"pending_vad_v2"},if *digital_silence{"not_needed"}else{"pending"}]).map_err(|e|e.to_string())?;
            }
            if prepared.replaced && !digital_silence {
                tx.execute("INSERT OR IGNORE INTO media_deletion_jobs(media_path,created_ms) VALUES(?1,?2)",params![loppi_protocol::platform::relative_key(&prepared.original),crate::now_ms()?]).map_err(|e|e.to_string())?;
            }
            tx.execute(
                "DELETE FROM orphan_media WHERE media_path=?1",
                [loppi_protocol::platform::relative_key(&prepared.original)],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("UPDATE recording_sessions SET last_confirmed_ms=max(last_confirmed_ms,?2) WHERE id=?1",params![entry.session.id.to_string(),entry.ended_ms]).map_err(|e|e.to_string())?;
            settle_intent_gaps(&tx, entry)?;
            // A helper may finish publishing just before core observes its
            // death. Recovered audio shortens that session's unfinished tail.
            tx.execute(
                "DELETE FROM media_gaps WHERE tail_session_id=?1 AND ended_ms<=?2",
                params![entry.session.id.to_string(), entry.ended_ms],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE media_gaps SET started_ms=max(started_ms,?2) WHERE tail_session_id=?1",
                params![entry.session.id.to_string(), entry.ended_ms],
            )
            .map_err(|e| e.to_string())?;
        }
        Kind::Gap { reason } => {
            insert_gap(&tx, entry, reason)?;
        }
    }
    tx.execute(
        "INSERT INTO ingestion_receipts VALUES(?1,?2)",
        params![entry.id.to_string(), entry.session.id.to_string()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn acknowledge(directory: &Path, entry: &Entry) -> Result<(), String> {
    let root = directory
        .join("ingestion")
        .join(entry.session.id.to_string());
    for extension in ["ready", "prepared", "writing"] {
        match fs::remove_file(root.join(format!("{}.{}", entry.id, extension))) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    sync_dir(&root)
}

pub fn finish_markers(directory: &Path, prepared: &Prepared) {
    if prepared.replaced {
        for row in &prepared.retained {
            if row.path != prepared.original {
                let _ = fs::remove_file(directory.join(&row.path).with_extension("cleanup-staged"));
            }
        }
    }
}

pub fn pending_sessions(db: &Connection, after: &str) -> Result<Vec<Uuid>, String> {
    let mut stmt = db
        .prepare("SELECT session_id FROM ingestion_pending_sessions WHERE session_id>?1 ORDER BY session_id LIMIT 128")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([after], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| Uuid::parse_str(&r.map_err(|e| e.to_string())?).map_err(|e| e.to_string()))
        .collect()
}

pub fn forget_receipt(db: &Connection, entry: &Entry) -> Result<(), String> {
    db.execute(
        "DELETE FROM ingestion_receipts WHERE id=?1",
        [entry.id.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn quarantine(directory: &Path, path: &Path) -> Result<(), String> {
    let session = path
        .parent()
        .and_then(Path::file_name)
        .ok_or("missing journal session")?;
    let target = directory.join("ingestion-quarantine").join(session);
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    sync_dir(&directory.join("ingestion-quarantine"))?;
    sync_dir(directory)?;
    loppi_protocol::platform::rename(
        path,
        target.join(path.file_name().ok_or("missing journal name")?),
    )
    .map_err(|e| e.to_string())?;
    sync_dir(&target)?;
    sync_dir(path.parent().unwrap())
}

pub fn reject(db: &mut Connection, entry: &Entry) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    if !acknowledged(&tx, entry)? {
        insert_gap(&tx, entry, "interrupted_seal")?;
        tx.execute(
            "INSERT INTO ingestion_receipts VALUES(?1,?2)",
            params![entry.id.to_string(), entry.session.id.to_string()],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn finish_journal(directory: &Path, session: Uuid) -> Result<bool, String> {
    let root = directory.join("ingestion").join(session.to_string());
    match fs::read_dir(&root) {
        Ok(entries) => {
            for entry in entries {
                let path = entry.map_err(|e| e.to_string())?.path();
                #[cfg(windows)]
                if path.file_name().is_some_and(|n| n == ".publication.lock") {
                    continue;
                }
                if path.extension().is_some_and(|e| e == "writing") {
                    quarantine(directory, &path)?;
                } else {
                    return Ok(false);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    #[cfg(windows)]
    match fs::remove_file(root.join(".publication.lock")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    match fs::remove_dir(&root) {
        Ok(()) => sync_dir(&directory.join("ingestion"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(true)
}

pub fn forget_session(db: &Connection, session: Uuid) -> Result<(), String> {
    db.execute(
        "DELETE FROM ingestion_receipts WHERE session_id=?1",
        [session.to_string()],
    )
    .map_err(|e| e.to_string())?;
    db.execute(
        "DELETE FROM ingestion_pending_sessions WHERE session_id=?1",
        [session.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Store,
        record::{BlockRecord, RecordEvent},
    };
    use std::sync::Mutex;

    #[test]
    fn pending_distinguishes_busy_publication_from_io_failure_and_retries() {
        let dir = tempfile::tempdir().unwrap();
        let session = Uuid::new_v4();
        assert!(pending(dir.path(), session, false).unwrap().is_empty());
        let root = dir.path().join("ingestion").join(session.to_string());
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("one.ready"), b"{}").unwrap();
        let guard = loppi_protocol::platform::open_directory(&root).unwrap();
        fs2::FileExt::lock_exclusive(&guard).unwrap();
        assert_eq!(
            pending(dir.path(), session, false).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(guard);
        assert_eq!(
            pending(dir.path(), session, false).unwrap(),
            vec![root.join("one.ready")]
        );
        fs::remove_dir_all(&root).unwrap();
        fs::write(&root, b"not a directory").unwrap();
        assert_eq!(
            pending(dir.path(), session, false).unwrap_err().kind(),
            std::io::ErrorKind::NotADirectory
        );
    }

    fn setup() -> (tempfile::TempDir, Mutex<Store>, Session) {
        let dir = tempfile::tempdir().unwrap();
        let mut store =
            Store::new(Connection::open(dir.path().join("loppi.sqlite")).unwrap()).unwrap();
        let id = Uuid::new_v4();
        store
            .start_recording_session(
                id,
                AudioSource::Microphone,
                Uuid::new_v4(),
                record::ClockAnchor::now().unwrap(),
            )
            .unwrap();
        let session = session(&store.db, id).unwrap();
        (dir, Mutex::new(store), session)
    }
    fn block(
        session: &Session,
        id: Uuid,
        offset: i64,
        preparing: bool,
        media: Option<media::SealedAudio>,
    ) -> BlockRecord {
        BlockRecord {
            id,
            source: session.source,
            started_ms: session.wall_anchor_ms + offset,
            ended_ms: session.wall_anchor_ms + offset + 1000,
            frames: 48000,
            sample_start: offset as u64 * 48,
            decision: "pending_vad_v2",
            preparing,
            media,
        }
    }
    fn publish(dir: &Path, session: &Session, offset: i64, finalize: bool) -> Uuid {
        let mut journal = Journal::new(dir, session.clone()).unwrap();
        let id = Uuid::new_v4();
        journal
            .persist(RecordEvent::Block(block(session, id, offset, true, None)))
            .unwrap();
        let mut opus = media::OpusBlock::create(&dir.join("media/microphone"), id).unwrap();
        opus.push(&vec![0.1; 48000]).unwrap();
        let sealed = opus.seal().unwrap();
        if finalize {
            journal
                .persist(RecordEvent::Block(block(
                    session,
                    id,
                    offset,
                    false,
                    Some(sealed),
                )))
                .unwrap();
        }
        id
    }
    fn clear(store: &Mutex<Store>, session: &Session, from: i64, to: i64, mask: i64, mode: &str) {
        store
            .lock()
            .unwrap()
            .db
            .execute(
                "INSERT INTO clear_ranges VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    Uuid::new_v4().to_string(),
                    session.wall_anchor_ms + from,
                    session.wall_anchor_ms + to,
                    mask,
                    mode,
                    crate::now_ms().unwrap().max(session.wall_anchor_ms + to)
                ],
            )
            .unwrap();
    }
    fn count(store: &Mutex<Store>) -> i64 {
        store
            .lock()
            .unwrap()
            .db
            .query_row("SELECT count(*) FROM audio_blocks", [], |r| r.get(0))
            .unwrap()
    }

    fn import_one(dir: &Path, store: &Mutex<Store>, session: &Session, id: Uuid) {
        let entry = load(&dir.join(format!("ingestion/{}/{id}.ready", session.id))).unwrap();
        let ranges = surviving_samples(&store.lock().unwrap().db, &entry).unwrap();
        let prepared = prepare(dir, &entry, &ranges).unwrap();
        commit(&mut store.lock().unwrap().db, &entry, &prepared).unwrap();
    }

    #[test]
    fn confirmed_final_audio_shortens_only_its_session_intent_gaps() {
        for reason in [
            "manual_pause",
            "schedule_pause",
            "storage_limit",
            "source_disabled",
        ] {
            for closed in [false, true] {
                let (dir, store, session) = setup();
                let id = publish(dir.path(), &session, 0, true);
                let wall = session.wall_anchor_ms;
                {
                    let mut s = store.lock().unwrap();
                    s.db.execute("UPDATE service_state SET enabled_intent=1 WHERE id=1", [])
                        .unwrap();
                    let tx = s.db.transaction().unwrap();
                    crate::open_intent_gap(&tx, "microphone", reason, wall + 500).unwrap();
                    crate::open_intent_gap(&tx, "system", reason, wall + 500).unwrap();
                    tx.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason,source_session_id) VALUES('callback','microphone',?1,?2,'callback_gap',?3),('other','microphone',?1,NULL,?4,'another-session')",params![wall+600,wall+700,session.id.to_string(),reason]).unwrap();
                    if closed {
                        crate::close_intent_gaps(&tx, reason, wall + 750).unwrap();
                    }
                    tx.commit().unwrap();
                    s.end_recording_session(session.id, true).unwrap();
                }
                import_one(dir.path(), &store, &session, id);
                let db = &store.lock().unwrap().db;
                let own:Option<i64> = db.query_row("SELECT started_ms FROM media_gaps WHERE reason=?1 AND source_session_id=?2",params![reason,session.id.to_string()],|r|r.get(0)).optional().unwrap();
                assert_eq!(own, if closed { None } else { Some(wall + 1000) });
                let sibling: i64 = db
                    .query_row(
                        "SELECT started_ms FROM media_gaps WHERE source='system' AND reason=?1",
                        [reason],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(sibling, wall + 500);
                let other: i64 = db
                    .query_row(
                        "SELECT started_ms FROM media_gaps WHERE id='other'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(other, wall + 600);
                let callback: (i64, i64) = db
                    .query_row(
                        "SELECT started_ms,ended_ms FROM media_gaps WHERE id='callback'",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(callback, (wall + 600, wall + 700));
            }
        }
    }

    #[test]
    fn intent_gap_correction_survives_cleanup_split_and_late_upgrade_journal() {
        for unowned in [false, true] {
            let (dir, store, session) = setup();
            publish(dir.path(), &session, 0, true);
            let wall = session.wall_anchor_ms;
            {
                let mut s = store.lock().unwrap();
                s.db.execute("UPDATE service_state SET enabled_intent=1 WHERE id=1", [])
                    .unwrap();
                let tx = s.db.transaction().unwrap();
                crate::open_intent_gap(&tx, "microphone", "manual_pause", wall + 100).unwrap();
                if unowned {
                    tx.execute("UPDATE media_gaps SET source_session_id=NULL", [])
                        .unwrap();
                }
                tx.commit().unwrap();
                s.end_recording_session(session.id, true).unwrap();
                execute_clear(&mut s, dir.path(), wall + 250, wall + 750, wall + 2000);
            }
            crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
            let s = store.lock().unwrap();
            let spans: Vec<(i64, Option<i64>)> = s
                .db
                .prepare("SELECT started_ms,ended_ms FROM media_gaps WHERE reason='manual_pause'")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(spans, vec![(wall + 1000, None)]);
            assert_eq!(count_blocks(&s.db), 2);
        }
    }

    #[test]
    fn intent_gap_open_after_confirmed_tail_and_old_gap_migration_are_idempotent() {
        let (dir, store, session) = setup();
        let id = publish(dir.path(), &session, 0, true);
        import_one(dir.path(), &store, &session, id);
        let wall = session.wall_anchor_ms;
        let mut s = store.lock().unwrap();
        s.db.execute("UPDATE service_state SET enabled_intent=1 WHERE id=1", [])
            .unwrap();
        let tx = s.db.transaction().unwrap();
        crate::open_intent_gap(&tx, "microphone", "manual_pause", wall + 500).unwrap();
        let start: i64 = tx
            .query_row(
                "SELECT started_ms FROM media_gaps WHERE reason='manual_pause'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(start, wall + 1000);
        tx.execute(
            "UPDATE media_gaps SET started_ms=?1,source_session_id=NULL",
            [wall + 500],
        )
        .unwrap();
        tx.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason) VALUES('unknown','microphone',?1,NULL,'schedule_pause'),('real','microphone',?2,?3,'callback_gap')",params![wall+2000,wall+200,wall+300]).unwrap();
        tx.execute(
            "DELETE FROM core_migrations WHERE name='intent_gap_confirmed_audio_v1'",
            [],
        )
        .unwrap();
        tx.commit().unwrap();
        init(&s.db).unwrap();
        let migrated: (i64, String) =
            s.db.query_row(
                "SELECT started_ms,source_session_id FROM media_gaps WHERE reason='manual_pause'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(migrated, (wall + 1000, session.id.to_string()));
        let unknown: (i64, Option<String>) =
            s.db.query_row(
                "SELECT started_ms,source_session_id FROM media_gaps WHERE id='unknown'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(unknown, (wall + 2000, None));
        let before: i64 =
            s.db.query_row("SELECT total_changes()", [], |r| r.get(0))
                .unwrap();
        init(&s.db).unwrap();
        let after: i64 =
            s.db.query_row("SELECT total_changes()", [], |r| r.get(0))
                .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn delayed_journal_tail_is_not_vad_eof_after_close_or_crash() {
        for crashed in [false, true] {
            let (dir, store, session) = setup();
            let a = publish(dir.path(), &session, 0, true);
            let b = publish(dir.path(), &session, 1000, true);
            if crashed {
                store.lock().unwrap().recover(dir.path()).unwrap();
            } else {
                store
                    .lock()
                    .unwrap()
                    .end_recording_session(session.id, true)
                    .unwrap();
            }
            import_one(dir.path(), &store, &session, a);
            assert!(
                store
                    .lock()
                    .unwrap()
                    .take_pending_block()
                    .unwrap()
                    .is_none()
            );
            import_one(dir.path(), &store, &session, b);
            let mut s = store.lock().unwrap();
            let first = s.take_pending_block().unwrap().unwrap();
            assert_eq!(first.id, a.to_string());
            assert!(!first.flush);
            s.publish_analysis(
                &first,
                crate::analysis::VadResult {
                    segments: vec![],
                    speech_active: false,
                    speech_pending: false,
                    error: None,
                },
            )
            .unwrap();
            let retained:bool=s.db.query_row("SELECT media_path IS NOT NULL AND retention_decision='pending_vad_v2' FROM audio_blocks WHERE id=?1",[a.to_string()],|r|r.get(0)).unwrap();
            assert!(retained); // Unconfirmed onset still has its original audio.
            let next = crate::PendingBlock {
                id: b.to_string(),
                started_ms: session.wall_anchor_ms + 1000,
                ended_ms: session.wall_anchor_ms + 2000,
                session_id: session.id.to_string(),
                source: "microphone".into(),
                sample_start: 48_000,
                frames: 48_000,
                media_path: format!("media/microphone/{b}.opus"),
                flush: true,
            };
            assert_eq!(s.replay_context(&next).unwrap()[0].id, a.to_string());
            assert!(s.take_pending_block().unwrap().is_none());
            drop(s);
            crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
            let mut s = store.lock().unwrap();
            let last = s.take_pending_block().unwrap().unwrap();
            assert_eq!(last.id, b.to_string());
            assert!(last.flush);
            s.publish_analysis(
                &last,
                crate::analysis::VadResult {
                    segments: vec![crate::analysis::SpeechRegion {
                        start_sample: 15_900,
                        end_sample: 24_000,
                    }],
                    speech_active: true,
                    speech_pending: false,
                    error: None,
                },
            )
            .unwrap();
            let retained:bool=s.db.query_row("SELECT media_path IS NOT NULL AND retention_decision='retain_speech_v2' FROM audio_blocks WHERE id=?1",[a.to_string()],|r|r.get(0)).unwrap();
            assert!(retained);
        }
    }

    #[test]
    fn active_journal_waits_for_boundary_proof_and_preserves_pending_order() {
        let (dir, store, mut session) = setup();
        session.wall_anchor_ms -= 60_000;
        store
            .lock()
            .unwrap()
            .db
            .execute(
                "UPDATE session_clock SET wall_anchor_ms=?2 WHERE session_id=?1",
                params![session.id.to_string(), session.wall_anchor_ms],
            )
            .unwrap();
        let a = publish(dir.path(), &session, 0, true);
        let b = publish(dir.path(), &session, 1000, true);
        let c = publish(dir.path(), &session, 2000, true);
        import_one(dir.path(), &store, &session, a);
        assert!(
            store
                .lock()
                .unwrap()
                .take_pending_block()
                .unwrap()
                .is_none()
        ); // >8s is not EOF.
        // Even a later eligible row cannot jump over an unresolved prefix.
        import_one(dir.path(), &store, &session, c);
        store.lock().unwrap().db.execute("INSERT INTO media_gaps(id,source,started_ms,ended_ms,reason,source_session_id) VALUES('known','microphone',?1,?2,'callback_gap',?3)",params![session.wall_anchor_ms+3000,session.wall_anchor_ms+4000,session.id.to_string()]).unwrap();
        assert!(
            store
                .lock()
                .unwrap()
                .take_pending_block()
                .unwrap()
                .is_none()
        );
        import_one(dir.path(), &store, &session, b);
        let first = store.lock().unwrap().take_pending_block().unwrap().unwrap();
        assert_eq!(first.id, a.to_string());
        assert!(!first.flush);
        store
            .lock()
            .unwrap()
            .db
            .execute(
                "UPDATE audio_blocks SET analysis_state='complete' WHERE id IN (?1,?2)",
                params![a.to_string(), b.to_string()],
            )
            .unwrap();
        let last = store.lock().unwrap().take_pending_block().unwrap().unwrap();
        assert_eq!(last.id, c.to_string());
        assert!(last.flush); // Same-session real gap proves end.
    }

    #[test]
    fn journal_snapshot_yields_to_publisher_without_holding_store_or_waiting() {
        let (dir, _store, session) = setup();
        let id = publish(dir.path(), &session, 0, true);
        let guard = loppi_protocol::platform::open_directory(
            dir.path().join(format!("ingestion/{}", session.id)),
        )
        .unwrap();
        fs2::FileExt::lock_exclusive(&guard).unwrap();
        let started = Instant::now();
        assert!(pending(dir.path(), session.id, false).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(guard);
        assert_eq!(
            pending(dir.path(), session.id, false).unwrap(),
            vec![
                dir.path()
                    .join(format!("ingestion/{}/{id}.ready", session.id))
            ]
        );
    }

    #[test]
    fn journal_tail_uses_digital_silence_and_selected_cleanup_as_boundaries() {
        for boundary in ["digital", "cleanup", "other_source", "future_clear"] {
            let (dir, store, mut session) = setup();
            session.wall_anchor_ms -= 60_000;
            store
                .lock()
                .unwrap()
                .db
                .execute(
                    "UPDATE session_clock SET wall_anchor_ms=?2 WHERE session_id=?1",
                    params![session.id.to_string(), session.wall_anchor_ms],
                )
                .unwrap();
            let a = publish(dir.path(), &session, 0, true);
            import_one(dir.path(), &store, &session, a);
            if boundary == "digital" {
                let mut b = block(&session, Uuid::new_v4(), 1000, false, None);
                b.decision = "discard_digital_silence_v1";
                Journal::new(dir.path(), session.clone())
                    .unwrap()
                    .persist(RecordEvent::Block(b))
                    .unwrap();
                crate::ingest_session_locked(&store, dir.path(), session.id, false).unwrap();
            } else if boundary == "future_clear" {
                store
                    .lock()
                    .unwrap()
                    .db
                    .execute(
                        "INSERT INTO clear_ranges VALUES('future',?1,?2,1,'raw_audio',?1)",
                        params![session.wall_anchor_ms, session.wall_anchor_ms + 2000],
                    )
                    .unwrap();
            } else {
                clear(
                    &store,
                    &session,
                    1000,
                    2000,
                    if boundary == "cleanup" { 1 } else { 2 },
                    "raw_audio",
                );
            }
            let claimed = store.lock().unwrap().take_pending_block().unwrap();
            if matches!(boundary, "digital" | "cleanup") {
                assert!(claimed.unwrap().flush);
            } else {
                assert!(claimed.is_none());
            }
        }
    }

    #[test]
    fn publication_cannot_change_a_live_journal_snapshot() {
        let (dir, _store, session) = setup();
        let a = publish(dir.path(), &session, 0, true);
        let snapshot = loppi_protocol::platform::open_directory(
            dir.path().join(format!("ingestion/{}", session.id)),
        )
        .unwrap();
        fs2::FileExt::lock_shared(&snapshot).unwrap();
        let directory = dir.path().to_owned();
        let source = session.clone();
        let (done, completion) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let b = publish(&directory, &source, 1000, true);
            done.send(b).unwrap();
        });
        assert!(completion.recv_timeout(Duration::from_millis(30)).is_err());
        assert_eq!(
            pending(dir.path(), session.id, false).unwrap(),
            vec![
                dir.path()
                    .join(format!("ingestion/{}/{a}.ready", session.id))
            ]
        );
        drop(snapshot);
        let b = completion.recv_timeout(Duration::from_secs(2)).unwrap();
        writer.join().unwrap();
        let paths = pending(dir.path(), session.id, false).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(
            paths
                .iter()
                .any(|p| p.file_stem().unwrap() == b.to_string().as_str())
        );
    }

    #[test]
    fn sealed_file_survives_crash_before_ready_manifest_and_ack_is_idempotent() {
        let (dir, store, session) = setup();
        publish(dir.path(), &session, 0, false);
        crate::ingest_session_locked(&store, dir.path(), session.id, false).unwrap();
        assert_eq!(count(&store), 0); // A live writer's prepared file is never consumed.
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        assert_eq!(count(&store), 1);
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        assert_eq!(count(&store), 1);
    }

    #[test]
    fn late_audio_obeys_partial_cleanup_and_keeps_unselected_samples() {
        let (dir, store, session) = setup();
        publish(dir.path(), &session, 0, true);
        clear(&store, &session, 250, 750, 1, "all_data");
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        let locked = store.lock().unwrap();
        let rows = locked
            .db
            .prepare(
                "SELECT sample_start,frames,media_path FROM audio_blocks ORDER BY sample_start",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)? as usize,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows.iter().map(|r| (r.0, r.1)).collect::<Vec<_>>(),
            vec![(0, 12000), (36000, 12000)]
        );
        for (_, frames, path) in rows {
            assert_eq!(
                media::decode_block(&dir.path().join(path), frames)
                    .unwrap()
                    .len(),
                frames
            );
        }
        assert_eq!(
            locked
                .db
                .query_row("SELECT count(*) FROM media_deletion_jobs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn durable_receipt_prevents_reimport_after_cleanup_before_journal_ack() {
        let (dir, store, session) = setup();
        publish(dir.path(), &session, 0, true);
        let entry = load(&pending(dir.path(), session.id, false).unwrap()[0]).unwrap();
        let prepared = prepare(dir.path(), &entry, &[(0, 48000)]).unwrap();
        commit(&mut store.lock().unwrap().db, &entry, &prepared).unwrap();
        // Simulate committed cleanup and a core crash before journal acknowledgement.
        store
            .lock()
            .unwrap()
            .db
            .execute("DELETE FROM audio_blocks", [])
            .unwrap();
        fs::remove_file(dir.path().join(entry.media_path())).unwrap();
        clear(&store, &session, 0, 1000, 1, "all_data");
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        assert_eq!(count(&store), 0);
        assert!(pending(dir.path(), session.id, true).unwrap().is_empty());
    }

    #[test]
    fn cleanup_generations_and_sources_do_not_erase_later_capture() {
        let (dir, store, old) = setup();
        clear(&store, &old, 0, 1000, 2, "raw_audio");
        publish(dir.path(), &old, 0, true);
        crate::ingest_session_locked(&store, dir.path(), old.id, true).unwrap();
        assert_eq!(count(&store), 1);
        clear(&store, &old, 0, 1000, 1, "all_data");
        let id = Uuid::new_v4();
        let mut clock = record::ClockAnchor::now().unwrap();
        clock.wall_ms = old.wall_anchor_ms;
        store
            .lock()
            .unwrap()
            .start_recording_session(id, old.source, Uuid::new_v4(), clock)
            .unwrap();
        let new = session(&store.lock().unwrap().db, id).unwrap();
        publish(dir.path(), &new, 0, true);
        crate::ingest_session_locked(&store, dir.path(), id, true).unwrap();
        assert_eq!(count(&store), 2);
    }

    #[test]
    fn missing_media_is_quarantined_and_following_blocks_still_import() {
        let (dir, store, session) = setup();
        let bad = publish(dir.path(), &session, 0, true);
        fs::write(
            dir.path().join(format!("media/microphone/{bad}.opus")),
            b"broken",
        )
        .unwrap();
        publish(dir.path(), &session, 1000, true);
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        assert_eq!(count(&store), 1);
        let gaps: i64 = store
            .lock()
            .unwrap()
            .db
            .query_row(
                "SELECT count(*) FROM media_gaps WHERE reason='interrupted_seal'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gaps, 1);
        assert!(
            dir.path()
                .join(format!("ingestion-quarantine/{}/{bad}.ready", session.id))
                .exists()
        );
    }

    #[test]
    fn backlog_is_bounded_and_last_prepared_block_can_finalize() {
        let (dir, _store, session) = setup();
        let mut journal = Journal::new(dir.path(), session.clone()).unwrap();
        for i in 0..MAX_SESSION_ENTRIES - 1 {
            let mut b = block(&session, Uuid::new_v4(), i as i64 * 1000, false, None);
            b.decision = "discard_digital_silence_v1";
            journal.persist(RecordEvent::Block(b)).unwrap();
        }
        let id = publish(dir.path(), &session, 64000, true);
        assert!(journal.root.join(format!("{id}.ready")).exists());
        assert!(
            journal
                .persist(RecordEvent::Block(block(
                    &session,
                    Uuid::new_v4(),
                    65000,
                    true,
                    None
                )))
                .is_err()
        );
        assert_eq!(
            pending(dir.path(), session.id, true).unwrap().len(),
            MAX_SESSION_ENTRIES
        );
    }

    #[test]
    fn late_gap_keeps_both_tails_and_late_audio_shortens_failure_gap() {
        let (dir, store, session) = setup();
        let mut journal = Journal::new(dir.path(), session.clone()).unwrap();
        journal
            .persist(RecordEvent::Gap(record::RecordGap {
                source: session.source,
                started_ms: session.wall_anchor_ms,
                ended_ms: session.wall_anchor_ms + 1000,
                reason: "callback_gap",
            }))
            .unwrap();
        clear(&store, &session, 250, 750, 1, "all_data");
        crate::ingest_session_locked(&store, dir.path(), session.id, false).unwrap();
        let db = &store.lock().unwrap().db;
        let duration: i64 = db
            .query_row(
                "SELECT sum(ended_ms-started_ms) FROM media_gaps WHERE reason='callback_gap'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(duration, 500);
        drop(journal);
        // Separate fixture, so the intentional lock lifetime above is explicit.
        let (dir, store, session) = setup();
        publish(dir.path(), &session, 0, true);
        store
            .lock()
            .unwrap()
            .end_recording_session(session.id, false)
            .unwrap();
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        let start: i64 = store
            .lock()
            .unwrap()
            .db
            .query_row(
                "SELECT started_ms FROM media_gaps WHERE id=?1",
                [session.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(start, session.wall_anchor_ms + 1000);
    }
    fn execute_clear(store: &mut Store, dir: &Path, start: i64, end: i64, now: i64) {
        use loppi_protocol::{CleanupMode, EventSources};
        let preview = cleanup::preview(
            &store.db,
            start,
            end,
            EventSources::Microphone,
            CleanupMode::AllData,
            now,
        )
        .unwrap();
        let scope = cleanup::verified_scope(&store.db, preview.token, true, now).unwrap();
        let prepared =
            cleanup::prepare_media(dir, &scope, cleanup::originals(&store.db, &scope).unwrap())
                .unwrap();
        let tx = store.db.transaction().unwrap();
        cleanup::apply(&tx, &scope, &prepared, now).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn late_sealed_audio_corrects_sleep_and_cleanup_split_lifecycle_gaps() {
        for sleep in [false, true] {
            let (dir, store, session) = setup();
            publish(dir.path(), &session, 0, true);
            {
                let mut locked = store.lock().unwrap();
                locked.end_recording_session(session.id, sleep).unwrap();
                if sleep {
                    locked
                        .record_gap_after_session(session.id, "sleep_or_clock_gap")
                        .unwrap();
                }
                let wall = session.wall_anchor_ms;
                execute_clear(&mut locked, dir.path(), wall + 250, wall + 750, wall + 2000);
                let parts: i64 = locked
                    .db
                    .query_row(
                        "SELECT count(*) FROM media_gaps WHERE tail_session_id=?1",
                        [session.id.to_string()],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(parts, 2);
            }
            crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
            let locked = store.lock().unwrap();
            let gaps: Vec<(i64, Option<i64>)> = locked
                .db
                .prepare("SELECT started_ms,ended_ms FROM media_gaps WHERE tail_session_id=?1")
                .unwrap()
                .query_map([session.id.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(gaps, vec![(session.wall_anchor_ms + 1000, None)]);
            assert_eq!(count_blocks(&locked.db), 2);
        }
    }
    fn count_blocks(db: &Connection) -> i64 {
        db.query_row("SELECT count(*) FROM audio_blocks", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn lifecycle_recovery_failure_and_sleep_obey_committed_cleanup() {
        for kind in ["recover", "failure", "sleep"] {
            let (dir, store, session) = setup();
            let mut locked = store.lock().unwrap();
            let wall = session.wall_anchor_ms;
            execute_clear(&mut locked, dir.path(), wall - 1, wall + 900, wall + 1000);
            match kind {
                "recover" => locked.recover(dir.path()).unwrap(),
                "failure" => locked.end_recording_session(session.id, false).unwrap(),
                _ => {
                    locked.end_recording_session(session.id, true).unwrap();
                    locked
                        .record_gap_after_session(session.id, "sleep_or_clock_gap")
                        .unwrap();
                }
            }
            let overlap:i64=locked.db.query_row("SELECT count(*) FROM media_gaps WHERE source='microphone' AND started_ms<?2 AND coalesce(ended_ms,9223372036854775807)>?1",params![wall-1,wall+900],|r|r.get(0)).unwrap();
            assert_eq!(overlap, 0, "{kind}");
        }
    }

    #[test]
    fn failed_session_page_does_not_starve_later_durable_audio_or_inventory_claims() {
        let (dir, store, session) = setup();
        for i in 1..=128 {
            let id = Uuid::from_u128(i);
            let mut locked = store.lock().unwrap();
            locked
                .start_recording_session(
                    id,
                    AudioSource::Microphone,
                    Uuid::new_v4(),
                    record::ClockAnchor::now().unwrap(),
                )
                .unwrap();
            assert!(id.to_string() < session.id.to_string());
            let root = dir.path().join("ingestion");
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join(id.to_string()), b"not a directory").unwrap();
        }
        publish(dir.path(), &session, 0, true);
        let gate = Mutex::new(());
        let mut scan = crate::inventory::Scan::new(&store).unwrap();
        while !scan.step(&store, &gate, dir.path()).unwrap() {}
        assert_eq!(store.lock().unwrap().state().unwrap().audio_used_bytes, 0);
        let first = pending_sessions(&store.lock().unwrap().db, "").unwrap();
        assert_eq!(first.len(), 128);
        for id in &first {
            assert!(crate::ingest_session_locked(&store, dir.path(), *id, true).is_err());
        }
        let second = pending_sessions(
            &store.lock().unwrap().db,
            &first.last().unwrap().to_string(),
        )
        .unwrap();
        assert_eq!(second, vec![session.id]);
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        assert_eq!(count(&store), 1);
    }

    #[test]
    fn future_end_selection_does_not_erase_audio_captured_after_clear_commit() {
        let (dir, store, session) = setup();
        publish(dir.path(), &session, 0, true);
        // Selection reaches the end of today, but clearing is an operation at
        // this moment, not an instruction to delete future recordings.
        store
            .lock()
            .unwrap()
            .db
            .execute(
                "INSERT INTO clear_ranges VALUES(?1,?2,?3,1,'all_data',?4)",
                params![
                    Uuid::new_v4().to_string(),
                    session.wall_anchor_ms,
                    session.wall_anchor_ms + 86_400_000,
                    session.wall_anchor_ms + 500
                ],
            )
            .unwrap();
        crate::ingest_session_locked(&store, dir.path(), session.id, true).unwrap();
        let (start, frames): (i64, i64) = store
            .lock()
            .unwrap()
            .db
            .query_row("SELECT sample_start,frames FROM audio_blocks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((start, frames), (24000, 24000));
    }
}
