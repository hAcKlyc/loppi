//! Bounded historical media reconciliation. Directory IO and deletion never
//! hold Store; the existing media lifetime gate serializes cleanup/publication.
use crate::{Store, ingestion};
use rusqlite::params;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};
use uuid::Uuid;

pub fn init(db: &rusqlite::Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS inventory_state(id INTEGER PRIMARY KEY CHECK(id=1),generation INTEGER NOT NULL);
      INSERT OR IGNORE INTO inventory_state VALUES(1,0);
      CREATE TABLE IF NOT EXISTS physical_storage(category TEXT PRIMARY KEY,bytes INTEGER NOT NULL,observed_ms INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS orphan_seen(media_path TEXT PRIMARY KEY,generation INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS orphan_seen_generation ON orphan_seen(generation,media_path);
      CREATE TRIGGER IF NOT EXISTS orphan_inventory_insert AFTER INSERT ON orphan_media BEGIN INSERT OR IGNORE INTO orphan_seen VALUES(new.media_path,0); END;
      CREATE TRIGGER IF NOT EXISTS orphan_inventory_delete AFTER DELETE ON orphan_media BEGIN DELETE FROM orphan_seen WHERE media_path=old.media_path; END;").map_err(|e|e.to_string())?;
    crate::storage::migrate(
        db,
        "orphan_inventory_v1",
        "INSERT OR IGNORE INTO orphan_seen SELECT media_path,0 FROM orphan_media;",
    )
}

pub struct Scan {
    generation: i64,
    began: SystemTime,
    next_source: usize,
    entries: Option<fs::ReadDir>,
    totals: [u64; 4],
    pending: Option<ClaimCheck>,
}
struct ClaimCheck {
    path: PathBuf,
    after_session: String,
}

impl Scan {
    pub fn new(store: &Mutex<Store>) -> Result<Self, String> {
        let mut store = store.lock().map_err(|e| e.to_string())?;
        let tx = store.db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE inventory_state SET generation=generation+1 WHERE id=1",
            [],
        )
        .map_err(|e| e.to_string())?;
        let generation = tx
            .query_row(
                "SELECT generation FROM inventory_state WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Self {
            generation,
            began: SystemTime::now(),
            next_source: 0,
            entries: None,
            totals: [0; 4],
            pending: None,
        })
    }
    fn next(&mut self, directory: &Path) -> Result<Option<PathBuf>, String> {
        loop {
            if let Some(entries) = self.entries.as_mut() {
                if let Some(entry) = entries.next() {
                    return entry.map(|e| Some(e.path())).map_err(|e| e.to_string());
                }
                self.entries = None;
            }
            let Some(source) = ["microphone", "system"].get(self.next_source) else {
                return Ok(None);
            };
            self.next_source += 1;
            self.entries = match fs::read_dir(directory.join("media").join(source)) {
                Ok(entries) => Some(entries),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.to_string()),
            };
        }
    }
    /// At most 32 files/rows per turn. Caller may yield to recording/ingestion.
    pub fn step(
        &mut self,
        store: &Mutex<Store>,
        gate: &Mutex<()>,
        directory: &Path,
    ) -> Result<bool, String> {
        for _ in 0..32 {
            let mut candidate = match self.pending.take() {
                Some(candidate) => candidate,
                None => {
                    let Some(path) = self.next(directory)? else {
                        return self.finish(store);
                    };
                    ClaimCheck {
                        path,
                        after_session: String::new(),
                    }
                }
            };
            if !self.inspect(store, gate, directory, &mut candidate)? {
                self.pending = Some(candidate);
                return Ok(false);
            }
        }
        Ok(false)
    }

    /// At most 128 pending sessions are checked per turn. Unreadable old
    /// journals cannot hide later claims or hold the media gate indefinitely.
    fn inspect(
        &mut self,
        store: &Mutex<Store>,
        gate: &Mutex<()>,
        directory: &Path,
        candidate: &mut ClaimCheck,
    ) -> Result<bool, String> {
        let path = &candidate.path;
        let _gate = gate.lock().map_err(|e| e.to_string())?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(e) => return Err(e.to_string()),
        };
        if !metadata.is_file() {
            return Ok(true);
        }
        let Some(id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            return Ok(true);
        };
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !matches!(extension, "opus" | "pending" | "cleanup-staged") {
            return Ok(true);
        }
        let relative = path
            .strip_prefix(directory)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let media_relative = path
            .with_extension("opus")
            .strip_prefix(directory)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let (known, deleting) = {
            let locked = store.lock().map_err(|e| e.to_string())?;
            locked.db.query_row("SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE media_path=?1),EXISTS(SELECT 1 FROM media_deletion_jobs WHERE media_path=?1 AND completed_ms IS NULL)",[&media_relative],|r|Ok((r.get::<_,bool>(0)?,r.get::<_,bool>(1)?))).map_err(|e|e.to_string())?
        };
        // Resolve a replacement before classifying *either* media suffix,
        // independent of filesystem enumeration order.
        let marker = path.with_extension("cleanup-staged");
        if marker.try_exists().map_err(|e| e.to_string())? {
            crate::cleanup::recover_staged_file(directory, &marker, known)?;
            let pending_relative = path
                .with_extension("pending")
                .strip_prefix(directory)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            store
                .lock()
                .map_err(|e| e.to_string())?
                .db
                .execute(
                    "DELETE FROM orphan_media WHERE media_path=?1 OR (?3=0 AND media_path=?2)",
                    params![pending_relative, media_relative, known],
                )
                .map_err(|e| e.to_string())?;
            if !known || extension != "opus" {
                return Ok(true);
            }
        } else if extension == "cleanup-staged" {
            return Ok(true);
        }
        let mut claimed = false;
        if !known && !deleting {
            let sessions = ingestion::pending_sessions(
                &store.lock().map_err(|e| e.to_string())?.db,
                &candidate.after_session,
            )?;
            claimed = sessions.iter().any(|session| {
                let root = directory.join("ingestion").join(session.to_string());
                root.join(format!("{id}.prepared")).exists()
                    || root.join(format!("{id}.ready")).exists()
            });
            if !claimed && sessions.len() == 128 {
                candidate.after_session = sessions.last().unwrap().to_string();
                return Ok(false);
            }
        }
        // A session may start after the DB snapshot. Its newly created file
        // will be checked next sweep; never label active capture as orphan.
        let fresh = metadata.modified().map_err(|e| e.to_string())? >= self.began;
        let category = if known {
            0
        } else if deleting {
            1
        } else if claimed || fresh {
            2
        } else {
            3
        };
        self.totals[category] = self.totals[category].saturating_add(metadata.len());
        let mut locked = store.lock().map_err(|e| e.to_string())?;
        let tx = locked.db.transaction().map_err(|e| e.to_string())?;
        if category == 3 {
            tx.execute("INSERT INTO orphan_media(media_path,bytes,detected_ms) VALUES(?1,?2,?3) ON CONFLICT(media_path) DO UPDATE SET bytes=excluded.bytes,detected_ms=excluded.detected_ms",params![relative,metadata.len() as i64,crate::now_ms()?]).map_err(|e|e.to_string())?;
            tx.execute("INSERT INTO orphan_seen VALUES(?1,?2) ON CONFLICT(media_path) DO UPDATE SET generation=excluded.generation",params![relative,self.generation]).map_err(|e|e.to_string())?;
        } else {
            tx.execute("DELETE FROM orphan_media WHERE media_path=?1", [&relative])
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM orphan_seen WHERE media_path=?1", [&relative])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        let state = locked.state()?;
        if !state.storage_paused && state.audio_used_bytes >= state.audio_limit_bytes {
            locked.pause_storage()?;
        }
        Ok(true)
    }
    fn finish(&self, store: &Mutex<Store>) -> Result<bool, String> {
        let mut locked = store.lock().map_err(|e| e.to_string())?;
        let tx = locked.db.transaction().map_err(|e| e.to_string())?;
        let removed=tx.execute("DELETE FROM orphan_media WHERE media_path IN (SELECT media_path FROM orphan_seen WHERE generation<?1 ORDER BY generation,media_path LIMIT 32)",[self.generation]).map_err(|e|e.to_string())?;
        if removed == 0 {
            for (category, bytes) in [
                "live_audio",
                "pending_deletion",
                "capture_staging",
                "orphan_audio",
            ]
            .into_iter()
            .zip(self.totals)
            {
                tx.execute("INSERT INTO physical_storage VALUES(?1,?2,?3) ON CONFLICT(category) DO UPDATE SET bytes=excluded.bytes,observed_ms=excluded.observed_ms",params![category,bytes as i64,crate::now_ms()?]).map_err(|e|e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(removed == 0)
    }
}

#[cfg(test)]
pub fn scan_all(store: Store, directory: &Path) -> Store {
    let store = Mutex::new(store);
    let gate = Mutex::new(());
    let mut scan = Scan::new(&store).unwrap();
    while !scan.step(&store, &gate, directory).unwrap() {}
    store.into_inner().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconciliation_yields_prunes_missing_orphans_and_preserves_pause() {
        let directory = tempfile::tempdir().unwrap();
        let media = directory.path().join("media/system");
        fs::create_dir_all(&media).unwrap();
        for _ in 0..70 {
            fs::write(media.join(format!("{}.pending", Uuid::new_v4())), [7; 100]).unwrap();
        }
        let store =
            Mutex::new(Store::new(rusqlite::Connection::open_in_memory().unwrap()).unwrap());
        let gate = Mutex::new(());
        let mut scan = Scan::new(&store).unwrap();
        assert!(!scan.step(&store, &gate, directory.path()).unwrap());
        let counted: i64 = store
            .lock()
            .unwrap()
            .db
            .query_row("SELECT count(*) FROM orphan_media", [], |r| r.get(0))
            .unwrap();
        assert_eq!(counted, 32);
        while !scan.step(&store, &gate, directory.path()).unwrap() {}
        store.lock().unwrap().pause_storage().unwrap();
        fs::remove_dir_all(&media).unwrap();
        let mut next = Scan::new(&store).unwrap();
        while !next.step(&store, &gate, directory.path()).unwrap() {}
        let locked = store.lock().unwrap();
        assert_eq!(locked.state().unwrap().audio_used_bytes, 0);
        assert!(locked.state().unwrap().storage_paused);
    }
    #[test]
    fn staged_media_is_recovered_before_quota_in_every_enumeration_order() {
        for extension in ["pending", "opus", "cleanup-staged"] {
            let dir = tempfile::tempdir().unwrap();
            let media = dir.path().join("media/microphone");
            fs::create_dir_all(&media).unwrap();
            let id = Uuid::new_v4();
            for suffix in ["pending", "opus", "cleanup-staged"] {
                fs::write(media.join(format!("{id}.{suffix}")), [1; 100]).unwrap();
            }
            let store =
                Mutex::new(Store::new(rusqlite::Connection::open_in_memory().unwrap()).unwrap());
            store
                .lock()
                .unwrap()
                .db
                .execute(
                    "UPDATE capture_guard SET audio_limit_bytes=100 WHERE id=1",
                    [],
                )
                .unwrap();
            let gate = Mutex::new(());
            let mut scan = Scan::new(&store).unwrap();
            // Invoke the production classification path with an explicit order.
            let mut candidate = ClaimCheck {
                path: media.join(format!("{id}.{extension}")),
                after_session: String::new(),
            };
            assert!(
                scan.inspect(&store, &gate, dir.path(), &mut candidate)
                    .unwrap()
            );
            while !scan.step(&store, &gate, dir.path()).unwrap() {}
            let state = store.lock().unwrap().state().unwrap();
            assert_eq!(state.audio_used_bytes, 0, "{extension}");
            assert!(!state.storage_paused, "{extension}");
            assert_eq!(fs::read_dir(&media).unwrap().count(), 0);
        }
    }

    #[test]
    fn active_journal_and_pending_deletion_are_never_orphan_quota() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let session_id = Uuid::new_v4();
        store
            .start_recording_session(
                session_id,
                loppi_protocol::AudioSource::System,
                Uuid::new_v4(),
                crate::record::ClockAnchor::now().unwrap(),
            )
            .unwrap();
        let session = ingestion::session(&store.db, session_id).unwrap();
        let mut journal = ingestion::Journal::new(dir.path(), session.clone()).unwrap();
        let id = Uuid::new_v4();
        journal
            .persist(crate::record::RecordEvent::Block(
                crate::record::BlockRecord {
                    id,
                    source: session.source,
                    started_ms: session.wall_anchor_ms,
                    ended_ms: session.wall_anchor_ms + 1000,
                    frames: 48000,
                    sample_start: 0,
                    decision: "pending_vad_v2",
                    preparing: true,
                    media: None,
                },
            ))
            .unwrap();
        let media = dir.path().join("media/system");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join(format!("{id}.pending")), [1; 100]).unwrap();
        let deleted = Uuid::new_v4();
        fs::write(media.join(format!("{deleted}.opus")), [2; 100]).unwrap();
        store
            .db
            .execute(
                "INSERT INTO media_deletion_jobs(media_path,created_ms) VALUES(?1,0)",
                [format!("media/system/{deleted}.opus")],
            )
            .unwrap();
        store
            .db
            .execute(
                "UPDATE capture_guard SET audio_limit_bytes=100 WHERE id=1",
                [],
            )
            .unwrap();
        let store = scan_all(store, dir.path());
        assert_eq!(store.state().unwrap().audio_used_bytes, 0);
        assert!(!store.state().unwrap().storage_paused);
        assert!(media.join(format!("{id}.pending")).exists());
    }
}
