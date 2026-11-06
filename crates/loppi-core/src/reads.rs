//! Two short read snapshots, independent of the business writer. Cached page
//! projections belong to a connection's SQLite data_version, never to a second
//! revision owner. Dropping a lease releases the WAL snapshot and deadline.
use rusqlite::{Connection, OpenFlags};
use serde::{Serialize, de::DeserializeOwned};
use std::cell::RefCell;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

// GUI 一次进入日期页会并发发起 timeline/calendar_days/moments/people/cloud_status，
// 2 个只读连接会让排在后面的请求在 3 秒内拿不到租约，用户看到“记录读取繁忙”。
const READERS: usize = 8;
const CACHE_BYTES: usize = 4 * 1024 * 1024;

/// Preserve the failure layer. A SQLite interruption is never invalid user input.
pub fn error_code(error: &str) -> &'static str {
    match error {
        "invalid time range" => "invalid_time_range",
        "invalid calendar boundaries" => "invalid_calendar_range",
        "invalid transcript page" => "invalid_transcript_page",
        "event not found" => "event_not_found",
        "segment is no longer current" => "speaker_invalid",
        _ if error.contains("interrupted") => "query_timeout",
        _ if error.contains("database is locked") || error.contains("database table is locked") => {
            "db_locked"
        }
        _ if error.contains("capacity busy") => "read_busy",
        _ => "query_failed",
    }
}

/// An allowlisted diagnostic cause, never SQL, provider bodies or private text.
pub fn failure_cause(error: &str) -> &'static str {
    match error {
        _ if error.contains("no such table") || error.contains("no such column") => "db_schema",
        _ if error.contains("constraint failed") => "db_constraint",
        _ if error.contains("disk I/O error") => "db_io",
        _ if error.contains("malformed") => "db_corrupt",
        _ => error_code(error),
    }
}
struct Cached {
    version: i64,
    key: String,
    json: Vec<u8>,
}
struct Reader {
    db: Connection,
    cache: RefCell<Option<Cached>>,
}
pub struct Pool {
    readers: Vec<Mutex<Reader>>,
}
pub struct Snapshot<'a> {
    reader: MutexGuard<'a, Reader>,
    version: i64,
    slot: usize,
}
impl Pool {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut readers = Vec::with_capacity(READERS);
        for _ in 0..READERS {
            let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| e.to_string())?;
            db.busy_timeout(Duration::from_millis(250))
                .map_err(|e| e.to_string())?;
            db.execute_batch("PRAGMA query_only=ON; PRAGMA cache_size=-2048;")
                .map_err(|e| e.to_string())?;
            readers.push(Mutex::new(Reader {
                db,
                cache: RefCell::new(None),
            }));
        }
        Ok(Self { readers })
    }
    pub fn shrink(&self) {
        for slot in &self.readers {
            if let Ok(reader) = slot.try_lock() {
                *reader.cache.borrow_mut() = None;
            }
        }
    }
    pub fn snapshot(&self) -> Result<Snapshot<'_>, String> {
        self.snapshot_for(Duration::from_secs(3))
    }
    fn snapshot_for(&self, timeout: Duration) -> Result<Snapshot<'_>, String> {
        let deadline = Instant::now() + timeout;
        loop {
            for (slot, entry) in self.readers.iter().enumerate() {
                if let Ok(reader) = entry.try_lock() {
                    reader
                        .db
                        .progress_handler(1000, Some(move || Instant::now() >= deadline))
                        .map_err(|e| e.to_string())?;
                    let mut lease = Snapshot {
                        reader,
                        version: 0,
                        slot,
                    };
                    lease
                        .execute_batch(
                            "BEGIN DEFERRED; SELECT revision FROM service_state LIMIT 1;",
                        )
                        .map_err(|e| e.to_string())?;
                    lease.version = lease
                        .query_row("PRAGMA data_version", [], |r| r.get(0))
                        .map_err(|e| e.to_string())?;
                    if lease
                        .reader
                        .cache
                        .borrow()
                        .as_ref()
                        .is_some_and(|c| c.version != lease.version)
                    {
                        *lease.reader.cache.borrow_mut() = None;
                    }
                    return Ok(lease);
                }
            }
            if Instant::now() >= deadline {
                return Err("read capacity busy".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Snapshot<'_> {
    pub fn version_key(&self, run: uuid::Uuid) -> String {
        format!("{run}:{}:{}", self.slot, self.version)
    }

    pub fn cached<T: Serialize + DeserializeOwned>(
        &self,
        key: impl Serialize,
        build: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let key = serde_json::to_string(&key).map_err(|e| e.to_string())?;
        if let Some(cached) = self.reader.cache.borrow().as_ref().filter(|c| c.key == key) {
            return serde_json::from_slice(&cached.json).map_err(|e| e.to_string());
        }
        let result = build(self)?;
        let json = serde_json::to_vec(&result).map_err(|e| e.to_string())?;
        *self.reader.cache.borrow_mut() = (json.len() <= CACHE_BYTES).then_some(Cached {
            version: self.version,
            key,
            json,
        });
        Ok(result)
    }
}
impl Deref for Snapshot<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.reader.db
    }
}
impl Drop for Snapshot<'_> {
    fn drop(&mut self) {
        let _ = self.reader.db.progress_handler(0, None::<fn() -> bool>);
        let _ = self.reader.db.execute_batch("ROLLBACK");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_identity_tracks_writes_connections_and_core_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE service_state(revision); INSERT INTO service_state VALUES(1);").unwrap();
        let pool = Pool::open(&path).unwrap();
        let run = uuid::Uuid::new_v4();
        let first = pool.snapshot().unwrap();
        let before = first.version_key(run);
        assert_ne!(before, pool.snapshot().unwrap().version_key(run));
        assert_ne!(before, first.version_key(uuid::Uuid::new_v4()));
        writer
            .execute("UPDATE service_state SET revision=2", [])
            .unwrap();
        assert_eq!(before, first.version_key(run));
        drop(first);
        let after = pool.snapshot().unwrap().version_key(run);
        assert_ne!(before, after);
        assert_eq!(after, pool.snapshot().unwrap().version_key(run));
    }

    #[test]
    fn query_failures_keep_their_layer() {
        for (error, code) in [
            ("interrupted", "query_timeout"),
            ("database is locked", "db_locked"),
            ("read capacity busy", "read_busy"),
            ("invalid time range", "invalid_time_range"),
            ("event not found", "event_not_found"),
            ("invalid transcript page", "invalid_transcript_page"),
            ("no such table: private", "query_failed"),
        ] {
            assert_eq!(error_code(error), code);
        }
        assert_eq!(failure_cause("no such column: private"), "db_schema");
    }
    #[test]
    fn snapshots_cache_only_their_version_and_do_not_hold_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;CREATE TABLE service_state(revision);INSERT INTO service_state VALUES(1);").unwrap();
        let pool = Pool::open(&path).unwrap();
        let read = |db: &Connection| {
            db.query_row("SELECT revision FROM service_state", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(|e| e.to_string())
        };
        let snapshot = pool.snapshot().unwrap();
        assert_eq!(snapshot.cached("state", read).unwrap(), 1);
        db.execute("UPDATE service_state SET revision=2", [])
            .unwrap();
        assert_eq!(
            snapshot
                .cached::<i64>("state", |_| panic!("cached snapshot"))
                .unwrap(),
            1_i64
        );
        assert_eq!(read(&snapshot).unwrap(), 1);
        drop(snapshot);
        assert_eq!(pool.snapshot().unwrap().cached("state", read).unwrap(), 2);
        let held: Vec<_> = (0..READERS).map(|_| pool.snapshot().unwrap()).collect();
        assert!(pool.snapshot_for(Duration::from_millis(10)).is_err());
        drop(held);
        let limited = pool.snapshot_for(Duration::from_millis(10)).unwrap();
        assert!(limited.query_row("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT sum(x) FROM n",[],|r|r.get::<_,i64>(0)).is_err());
        drop(limited);
        assert_eq!(read(&pool.snapshot().unwrap()).unwrap(), 2);
    }
}
