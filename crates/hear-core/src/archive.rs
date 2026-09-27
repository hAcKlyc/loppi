use hear_protocol::EventSources;
use rusqlite::{Connection, Row, params};
use serde_json::{Value, json};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tar::{Builder, Header};
use uuid::Uuid;

fn source_mask(sources: EventSources) -> i64 {
    match sources {
        EventSources::Microphone => 1,
        EventSources::System => 2,
        EventSources::Both => 3,
    }
}

fn destination_has_job_marker(path: &Path, job_id: &str) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut archive = tar::Archive::new(file);
    let Ok(mut entries) = archive.entries() else {
        return false;
    };
    let Some(Ok(mut entry)) = entries.next() else {
        return false;
    };
    if entry.size() != 36 || entry.path().ok().as_deref() != Some(Path::new(".hear-export-id")) {
        return false;
    }
    let mut marker = String::new();
    entry.read_to_string(&mut marker).is_ok() && marker == job_id
}

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS export_jobs (
      id TEXT PRIMARY KEY,staging_path TEXT NOT NULL,created_ms INTEGER NOT NULL,
      destination_path TEXT,staging_dev INTEGER,
      state TEXT NOT NULL DEFAULT 'pending')",
    )
    .map_err(|error| error.to_string())
}

pub fn recover(db: &Connection) -> Result<(), String> {
    let mut stmt = db
        .prepare("SELECT id,staging_path,destination_path,staging_dev,state FROM export_jobs")
        .map_err(|error| error.to_string())?;
    let jobs = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for (id, path, destination, staging_dev, state) in jobs {
        if state == "published" {
            // The directory rename and the success marker were durable. The
            // final user file is committed, even if job GC was interrupted.
            db.execute("DELETE FROM export_jobs WHERE id=?1", params![id])
                .map_err(|error| error.to_string())?;
            continue;
        }
        let path = PathBuf::from(path);
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !path.is_absolute() || !name.starts_with(".hear-export-") || !name.ends_with(".pending")
        {
            db.execute("DELETE FROM export_jobs WHERE id=?1", params![id])
                .map_err(|error| error.to_string())?;
            continue;
        }
        let parent = path.parent().ok_or("export parent missing")?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // An unplugged destination can make the whole parent vanish.
                // Keep the job so it can be retried when the volume returns.
                let Ok(parent_meta) = fs::metadata(parent) else {
                    continue;
                };
                if !parent_meta.is_dir() {
                    continue;
                }
                // Rename may have published the file before a crash or an
                // I/O error. The first TAR entry identifies this job, even
                // after a remount changes the device number. Inode numbers
                // can be reused and are unsafe for deleting user files.
                let mut removed_published = false;
                if let Some(destination) = destination {
                    let destination = PathBuf::from(destination);
                    if destination.is_absolute() && destination.parent() == Some(parent) {
                        match fs::symlink_metadata(&destination) {
                            Ok(meta)
                                if meta.file_type().is_file()
                                    && destination_has_job_marker(&destination, &id) =>
                            {
                                if fs::remove_file(&destination).is_err() {
                                    continue;
                                }
                                removed_published = true;
                            }
                            Ok(_) if staging_dev != Some(parent_meta.dev() as i64) => continue,
                            Ok(_) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(_) => continue,
                        }
                    }
                }
                if !removed_published
                    && staging_dev.is_some_and(|dev| dev != parent_meta.dev() as i64)
                {
                    // The mountpoint may currently name a different volume.
                    // Without our marked destination, keep the job for retry.
                    continue;
                }
            }
            // A disconnected or read-only external destination must not stop
            // recording on login. Keep the job and retry on next launch.
            Err(_) => continue,
        }
        if File::open(parent)
            .and_then(|directory| directory.sync_all())
            .is_err()
        {
            continue;
        }
        db.execute("DELETE FROM export_jobs WHERE id=?1", params![id])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn rename_exclusive(from: &Path, to: &Path) -> Result<(), String> {
    let from = CString::new(from.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
    let rc = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

pub fn cleanup_spool(directory: &Path) -> Result<(), String> {
    let spool = directory.join("export_tmp");
    let metadata = match fs::symlink_metadata(&spool) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err("export spool is not a private directory".into());
    }
    for entry in fs::read_dir(&spool).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("export-") && name.ends_with(".jsonl") {
            fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn archive_bytes(builder: &mut Builder<File>, name: &str, content: &[u8]) -> Result<(), String> {
    let mut header = Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    builder
        .append_data(&mut header, name, content)
        .map_err(|error| error.to_string())
}

fn archive_rows<F>(
    builder: &mut Builder<File>,
    db: &Connection,
    directory: &Path,
    name: &str,
    sql: &str,
    start: i64,
    end: i64,
    mask: i64,
    mut map: F,
) -> Result<u64, String>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<Value>,
{
    let temporary = directory.join(format!("export-{}.jsonl", Uuid::new_v4()));
    let result = (|| {
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let mut writer = BufWriter::new(file);
        let mut count = 0;
        let mut cursor_time = i64::MIN;
        let mut cursor_id = String::new();
        let time_column = if name == "events.jsonl" { 1 } else { 2 };
        loop {
            // Release the SQLite statement before filesystem writes. A long
            // history export must not pin a WAL snapshot for its whole run.
            let page = {
                let mut stmt = db
                    .prepare(&format!(
                        "SELECT * FROM ({sql}) result
                      WHERE EXISTS(SELECT 1 FROM temp.export_allow allowed
                        WHERE allowed.kind=?4 AND allowed.id=result.id)
                        AND (started_ms>?5 OR (started_ms=?5 AND id>?6))
                      ORDER BY started_ms,id LIMIT ?7"
                    ))
                    .map_err(|error| error.to_string())?;
                stmt.query_map(
                    params![start, end, mask, name, cursor_time, cursor_id, 256i64],
                    |row| {
                        Ok((
                            row.get::<_, i64>(time_column)?,
                            row.get::<_, String>(0)?,
                            map(row)?,
                        ))
                    },
                )
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
            };
            let page_len = page.len();
            for (time, id, row) in page {
                cursor_time = time;
                cursor_id = id;
                serde_json::to_writer(&mut writer, &row).map_err(|error| error.to_string())?;
                writer.write_all(b"\n").map_err(|error| error.to_string())?;
                count += 1;
            }
            if page_len < 256 {
                break;
            }
        }
        writer.flush().map_err(|error| error.to_string())?;
        drop(writer);
        builder
            .append_path_with_name(&temporary, name)
            .map_err(|error| error.to_string())?;
        Ok(count)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn wav_mono_48k(samples: &[f32]) -> Result<Vec<u8>, String> {
    let data_bytes = u32::try_from(
        samples
            .len()
            .checked_mul(2)
            .ok_or("audio export too large")?,
    )
    .map_err(|_| "audio export too large".to_owned())?;
    let mut output = Vec::with_capacity(44 + data_bytes as usize);
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&(36u32 + data_bytes).to_le_bytes());
    output.extend_from_slice(b"WAVEfmt ");
    output.extend_from_slice(&16u32.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&48_000u32.to_le_bytes());
    output.extend_from_slice(&96_000u32.to_le_bytes());
    output.extend_from_slice(&2u16.to_le_bytes());
    output.extend_from_slice(&16u16.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        output.extend_from_slice(&value.to_le_bytes());
    }
    Ok(output)
}

pub fn export(
    db: &Connection,
    directory: &Path,
    start: i64,
    end: i64,
    sources: EventSources,
    include_audio: bool,
    include_screenshots: bool,
    destination: &Path,
    cleanup_gate: &Mutex<()>,
) -> Result<(u64, u64), String> {
    if start < 0 || end <= start {
        return Err("export range invalid".into());
    }
    if !destination.is_absolute() || destination.extension().is_none_or(|value| value != "tar") {
        return Err("choose an absolute .tar destination".into());
    }
    if include_screenshots {
        return Err("screenshots are not ready for export".into());
    }
    let mask = source_mask(sources);
    fs::create_dir_all(directory.join("export_tmp")).map_err(|error| error.to_string())?;
    let spool = directory.join("export_tmp");
    // Startup recovery owns stale spool cleanup. Concurrent exports use
    // unique temporary names and must not delete each other's files.
    fs::set_permissions(&spool, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let cleared_before: i64 = db
        .query_row("SELECT coalesce(max(rowid),0) FROM clear_ranges", [], |r| {
            r.get(0)
        })
        .map_err(|error| error.to_string())?;
    db.execute_batch(
        "DROP TABLE IF EXISTS temp.export_allow;
      CREATE TEMP TABLE export_allow(kind TEXT NOT NULL,id TEXT NOT NULL,
        PRIMARY KEY(kind,id)) WITHOUT ROWID; BEGIN",
    )
    .map_err(|error| error.to_string())?;
    let selected = (|| {
        for (kind, table, source_sql) in [
            ("events.jsonl", "named_events", "source_mask&?3!=0"),
            (
                "coarse.jsonl",
                "coarse_segments",
                "((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
            ),
            (
                "precise.jsonl",
                "precise_segments",
                "((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
            ),
            (
                "audio",
                "audio_blocks",
                "((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))",
            ),
        ] {
            db.execute(
                &format!(
                    "INSERT INTO temp.export_allow(kind,id)
              SELECT ?4,id FROM {table} WHERE started_ms<?2 AND ended_ms>?1 AND {source_sql}"
                ),
                params![start, end, mask, kind],
            )
            .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = selected {
        let _ = db.execute_batch("ROLLBACK");
        return Err(error);
    }
    db.execute_batch("COMMIT")
        .map_err(|error| error.to_string())?;
    let job_id = Uuid::new_v4();
    let staging = destination
        .parent()
        .ok_or("export parent missing")?
        .join(format!(".hear-export-{job_id}.pending"));
    db.execute(
        "INSERT INTO export_jobs(id,staging_path,created_ms,destination_path)
        VALUES(?1,?2,cast(strftime('%s','now') as integer)*1000,?3)",
        params![
            job_id.to_string(),
            staging.to_string_lossy().to_string(),
            destination.to_string_lossy().to_string()
        ],
    )
    .map_err(|error| error.to_string())?;
    let file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o000)
        .open(&staging)
    {
        Ok(file) => file,
        Err(error) => {
            let _ = db.execute(
                "DELETE FROM export_jobs WHERE id=?1",
                params![job_id.to_string()],
            );
            return Err(error.to_string());
        }
    };
    if let Err(error) = fs::set_permissions(&staging, fs::Permissions::from_mode(0o000)) {
        let _ = fs::remove_file(&staging);
        let _ = db.execute(
            "DELETE FROM export_jobs WHERE id=?1",
            params![job_id.to_string()],
        );
        return Err(error.to_string());
    }
    let identity = file.metadata().map_err(|error| error.to_string())?;
    if let Err(error) = db.execute(
        "UPDATE export_jobs SET staging_dev=?2 WHERE id=?1",
        params![job_id.to_string(), identity.dev() as i64],
    ) {
        let _ = fs::remove_file(&staging);
        let _ = db.execute(
            "DELETE FROM export_jobs WHERE id=?1",
            params![job_id.to_string()],
        );
        return Err(error.to_string());
    }
    let mut builder = Builder::new(file);
    let result = (|| {
        archive_bytes(
            &mut builder,
            ".hear-export-id",
            job_id.to_string().as_bytes(),
        )?;
        let events = archive_rows(
            &mut builder,
            db,
            &spool,
            "events.jsonl",
            "SELECT e.id,e.started_ms,e.ended_ms,e.title,e.source_mask,e.revision
           FROM named_events e WHERE e.deleted_ms IS NULL AND e.started_ms<?2
           AND e.ended_ms>?1 AND e.source_mask&?3!=0 AND EXISTS(
             SELECT 1 FROM audio_blocks b WHERE b.media_path IS NOT NULL
               AND b.started_ms<?2 AND b.ended_ms>?1
               AND b.started_ms<e.ended_ms AND b.ended_ms>e.started_ms
               AND ((b.source='microphone' AND e.source_mask&?3&1!=0)
                 OR (b.source='system' AND e.source_mask&?3&2!=0))
             UNION ALL SELECT 1 FROM coarse_segments c WHERE c.started_ms<?2 AND c.ended_ms>?1
               AND c.started_ms<e.ended_ms AND c.ended_ms>e.started_ms
               AND ((c.source='microphone' AND e.source_mask&?3&1!=0)
                 OR (c.source='system' AND e.source_mask&?3&2!=0))
             UNION ALL SELECT 1 FROM precise_segments p JOIN cloud_tasks t ON t.id=p.task_id
               WHERE t.state='complete' AND p.started_ms<?2 AND p.ended_ms>?1
               AND p.started_ms<e.ended_ms AND p.ended_ms>e.started_ms
               AND ((p.source='microphone' AND e.source_mask&?3&1!=0)
                 OR (p.source='system' AND e.source_mask&?3&2!=0)))
           ORDER BY e.started_ms,e.id",
            start,
            end,
            mask,
            |row| {
                Ok(
                    json!({"id":row.get::<_,String>(0)?,"started_ms":row.get::<_,i64>(1)?,
              "ended_ms":row.get::<_,i64>(2)?,"title":row.get::<_,String>(3)?,
              "source_mask":row.get::<_,i64>(4)?,"revision":row.get::<_,i64>(5)?}),
                )
            },
        )?;
        let coarse = archive_rows(
            &mut builder,
            db,
            &spool,
            "coarse.jsonl",
            "SELECT c.id,c.source,c.started_ms,c.ended_ms,c.text FROM coarse_segments c
           WHERE c.started_ms<?2 AND c.ended_ms>?1 AND
           ((c.source='microphone' AND ?3&1!=0) OR (c.source='system' AND ?3&2!=0))
           ORDER BY c.started_ms,c.id",
            start,
            end,
            mask,
            |row| {
                Ok(
                    json!({"id":row.get::<_,String>(0)?,"source":row.get::<_,String>(1)?,
              "started_ms":row.get::<_,i64>(2)?,"ended_ms":row.get::<_,i64>(3)?,
              "text":row.get::<_,String>(4)?}),
                )
            },
        )?;
        let precise = archive_rows(
            &mut builder,
            db,
            &spool,
            "precise.jsonl",
            "SELECT p.id,p.source,p.started_ms,p.ended_ms,p.text,
             (SELECT c.text FROM text_corrections c WHERE c.current_segment_id=p.id
                AND c.state='active' ORDER BY c.updated_ms DESC LIMIT 1),
             p.speaker_label,b.person_id,person.display_name,b.origin,t.target_kind,t.target_id
           FROM precise_segments p JOIN cloud_tasks t ON t.id=p.task_id
           LEFT JOIN speaker_bindings b ON b.current_segment_id=p.id AND b.state='active'
           LEFT JOIN people person ON person.id=b.person_id
           LEFT JOIN named_events e ON t.target_kind='event' AND e.id=t.target_id
           LEFT JOIN manual_ranges r ON t.target_kind='range' AND r.id=t.target_id
           WHERE t.state='complete' AND p.started_ms<?2 AND p.ended_ms>?1
             AND ((p.source='microphone' AND ?3&1!=0) OR (p.source='system' AND ?3&2!=0))
             AND ((e.deleted_ms IS NULL AND e.id IS NOT NULL AND
               t.target_version=printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms))
               OR (r.id IS NOT NULL AND t.target_version=cast(r.revision AS TEXT)))
           ORDER BY p.started_ms,p.id",
            start,
            end,
            mask,
            |row| {
                let correction: Option<String> = row.get(5)?;
                let original: String = row.get(4)?;
                Ok(
                    json!({"id":row.get::<_,String>(0)?,"source":row.get::<_,String>(1)?,
              "started_ms":row.get::<_,i64>(2)?,"ended_ms":row.get::<_,i64>(3)?,
              "text":correction.as_ref().unwrap_or(&original),"corrected":correction.is_some(),
              "speaker":row.get::<_,Option<String>>(6)?,"person_id":row.get::<_,Option<String>>(7)?,
              "person_name":row.get::<_,Option<String>>(8)?,
              "person_origin":row.get::<_,Option<String>>(9)?,
              "target_kind":row.get::<_,String>(10)?,"target_id":row.get::<_,String>(11)?}),
                )
            },
        )?;
        let mut audio_files = 0;
        if include_audio {
            let mut cursor_time = i64::MIN;
            let mut cursor_id = String::new();
            loop {
                let rows = {
                    let mut stmt = db
                        .prepare(
                            "SELECT id,source,started_ms,ended_ms,frames,media_path
              FROM audio_blocks WHERE media_path IS NOT NULL AND started_ms<?2 AND ended_ms>?1
              AND ((source='microphone' AND ?3&1!=0) OR (source='system' AND ?3&2!=0))
              AND EXISTS(SELECT 1 FROM temp.export_allow allowed WHERE allowed.kind='audio'
                AND allowed.id=audio_blocks.id)
              AND (started_ms>?4 OR (started_ms=?4 AND id>?5))
              ORDER BY started_ms,id LIMIT 128",
                        )
                        .map_err(|error| error.to_string())?;
                    stmt.query_map(params![start, end, mask, cursor_time, cursor_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?
                };
                let page_len = rows.len();
                for row in rows {
                    let (id, source, a, b, frames, path) = row;
                    cursor_time = a;
                    cursor_id = id.clone();
                    let _media_guard = cleanup_gate.lock().map_err(|error| error.to_string())?;
                    let still_current: i64 = db
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE id=?1 AND media_path=?2
                     AND started_ms=?3 AND ended_ms=?4 AND frames=?5)",
                            params![id, path, a, b, frames],
                            |r| r.get(0),
                        )
                        .map_err(|error| error.to_string())?;
                    if still_current == 0 {
                        continue;
                    }
                    let expected = PathBuf::from("media")
                        .join(&source)
                        .join(format!("{id}.opus"));
                    if Path::new(&path) != expected
                        || !matches!(source.as_str(), "microphone" | "system")
                    {
                        return Err("audio export manifest invalid".into());
                    }
                    let file = directory.join(&path);
                    if fs::symlink_metadata(&file)
                        .map_err(|error| error.to_string())?
                        .file_type()
                        .is_symlink()
                    {
                        return Err("audio export source is a symlink".into());
                    }
                    let pcm = crate::media::decode_block(&file, frames as usize)?;
                    let from = ((start.max(a) - a) as usize * 48).min(pcm.len());
                    let to = ((end.min(b) - a) as usize * 48).min(pcm.len());
                    if to <= from {
                        continue;
                    }
                    let wav = wav_mono_48k(&pcm[from..to])?;
                    archive_bytes(&mut builder, &format!("audio/{source}/{a}-{id}.wav"), &wav)?;
                    audio_files += 1;
                }
                if page_len < 128 {
                    break;
                }
            }
        }
        let manifest = json!({"format":"hear-export-0.1","from_ms":start,"to_ms":end,
            "sources":sources,"events":events,"coarse_segments":coarse,
            "precise_segments":precise,"audio_files":audio_files,
            "screenshots_included":false});
        archive_bytes(
            &mut builder,
            "manifest.json",
            &serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
        )?;
        builder.finish().map_err(|error| error.to_string())?;
        builder
            .into_inner()
            .map_err(|error| error.to_string())?
            .sync_all()
            .map_err(|error| error.to_string())?;
        let _publish_gate = cleanup_gate.lock().map_err(|error| error.to_string())?;
        let cleared_during: i64 = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM clear_ranges WHERE rowid>?1 AND from_ms<?3
             AND to_ms>?2 AND source_mask&?4!=0)",
                params![cleared_before, start, end, mask],
                |r| r.get(0),
            )
            .map_err(|error| error.to_string())?;
        if cleared_during != 0 {
            return Err("cleanup changed this range during export; retry".into());
        }
        // Make final permissions durable while the file is still hidden. A
        // crash immediately after rename must never expose a 000 final TAR.
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        File::open(&staging)
            .and_then(|file| file.sync_all())
            .map_err(|error| error.to_string())?;
        rename_exclusive(&staging, destination)?;
        // Persist the directory rename before forgetting the recovery job.
        let parent = destination.parent().ok_or("export parent missing")?;
        if let Err(error) = File::open(parent).and_then(|directory| directory.sync_all()) {
            // The job still remembers the destination and TAR marker if
            // immediate rollback fails; startup retries when writable.
            let _ = fs::remove_file(destination);
            let _ = File::open(parent).and_then(|directory| directory.sync_all());
            return Err(error.to_string());
        }
        if let Err(error) = db.execute(
            "UPDATE export_jobs SET state='published' WHERE id=?1",
            params![job_id.to_string()],
        ) {
            let _ = fs::remove_file(destination);
            let _ = File::open(parent).and_then(|directory| directory.sync_all());
            return Err(error.to_string());
        }
        Ok((audio_files, coarse + precise))
    })();
    let removed_staging = result.is_err() && fs::remove_file(&staging).is_ok();
    // A failed operation only retires its job once removal is durable. If
    // cleanup or the destination volume is unavailable, startup retries it.
    let safe_to_retire = result.is_ok()
        || (removed_staging
            && staging
                .parent()
                .and_then(|parent| File::open(parent).ok())
                .is_some_and(|directory| directory.sync_all().is_ok()));
    if safe_to_retire {
        let _ = db.execute(
            "DELETE FROM export_jobs WHERE id=?1",
            params![job_id.to_string()],
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup;

    #[test]
    fn cleanup_before_publish_discards_private_export() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hear.sqlite");
        let store = crate::Store::new(Connection::open(&path).unwrap()).unwrap();
        let gate = std::sync::Arc::new(Mutex::new(()));
        let hold = gate.lock().unwrap();
        let dest = directory.path().join("history.tar");
        let worker_gate = gate.clone();
        let worker_dir = directory.path().to_path_buf();
        let worker_dest = dest.clone();
        let worker = std::thread::spawn(move || {
            let db = Connection::open(worker_dir.join("hear.sqlite")).unwrap();
            export(
                &db,
                &worker_dir,
                500,
                1500,
                EventSources::Microphone,
                false,
                false,
                &worker_dest,
                &worker_gate,
            )
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".hear-export-")
                })
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "export did not stage");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!dest.exists());
        store
            .db
            .execute(
                "INSERT INTO clear_ranges
            (id,from_ms,to_ms,source_mask,mode,committed_ms)
            VALUES(?1,500,1500,1,'all_data',1000)",
                params![Uuid::new_v4().to_string()],
            )
            .unwrap();
        drop(hold);
        assert!(
            worker
                .join()
                .unwrap()
                .unwrap_err()
                .contains("cleanup changed")
        );
        assert!(!dest.exists());
        assert!(
            !fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".hear-export-"))
        );
    }

    #[test]
    fn unfinished_export_is_removed_on_startup() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let staging = directory
            .path()
            .join(format!(".hear-export-{}.pending", Uuid::new_v4()));
        fs::write(&staging, b"unfinished private content").unwrap();
        db.execute(
            "INSERT INTO export_jobs(id,staging_path,created_ms) VALUES(?1,?2,1)",
            params![
                Uuid::new_v4().to_string(),
                staging.to_string_lossy().to_string()
            ],
        )
        .unwrap();
        recover(&db).unwrap();
        assert!(!staging.exists());
        let remaining: i64 = db
            .query_row("SELECT count(*) FROM export_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn disconnected_export_volume_keeps_recovery_job_until_it_returns() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let mount = directory.path().join("external-volume");
        let staging = mount.join(format!(".hear-export-{}.pending", Uuid::new_v4()));
        db.execute(
            "INSERT INTO export_jobs(id,staging_path,created_ms) VALUES(?1,?2,1)",
            params![
                Uuid::new_v4().to_string(),
                staging.to_string_lossy().to_string()
            ],
        )
        .unwrap();
        recover(&db).unwrap();
        let pending: i64 = db
            .query_row("SELECT count(*) FROM export_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pending, 1);
        fs::create_dir(&mount).unwrap();
        fs::write(&staging, b"unfinished private content").unwrap();
        recover(&db).unwrap();
        assert!(!staging.exists());
        let pending: i64 = db
            .query_row("SELECT count(*) FROM export_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn recovery_removes_a_published_but_unacknowledged_export() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let job_id = Uuid::new_v4().to_string();
        let staging = directory
            .path()
            .join(format!(".hear-export-{job_id}.pending"));
        let destination = directory.path().join("history.tar");
        let mut builder = Builder::new(File::create(&staging).unwrap());
        archive_bytes(&mut builder, ".hear-export-id", job_id.as_bytes()).unwrap();
        builder.finish().unwrap();
        drop(builder);
        let meta = fs::metadata(&staging).unwrap();
        db.execute(
            "INSERT INTO export_jobs
             (id,staging_path,created_ms,destination_path,staging_dev)
             VALUES(?1,?2,1,?3,?4)",
            params![
                job_id,
                staging.to_string_lossy().to_string(),
                destination.to_string_lossy().to_string(),
                meta.dev() as i64
            ],
        )
        .unwrap();
        fs::rename(&staging, &destination).unwrap();
        recover(&db).unwrap();
        assert!(!destination.exists());
        let pending: i64 = db
            .query_row("SELECT count(*) FROM export_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn recovery_never_removes_an_unrelated_destination() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let staging = directory
            .path()
            .join(format!(".hear-export-{}.pending", Uuid::new_v4()));
        let destination = directory.path().join("history.tar");
        fs::write(&staging, b"discarded stage").unwrap();
        let meta = fs::metadata(&staging).unwrap();
        db.execute(
            "INSERT INTO export_jobs
             (id,staging_path,created_ms,destination_path,staging_dev)
             VALUES(?1,?2,1,?3,?4)",
            params![
                Uuid::new_v4().to_string(),
                staging.to_string_lossy().to_string(),
                destination.to_string_lossy().to_string(),
                meta.dev() as i64
            ],
        )
        .unwrap();
        fs::remove_file(&staging).unwrap();
        fs::write(&destination, b"user file").unwrap();
        recover(&db).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"user file");
    }

    #[test]
    fn published_job_survives_interrupted_job_gc() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let staging = directory
            .path()
            .join(format!(".hear-export-{}.pending", Uuid::new_v4()));
        let destination = directory.path().join("history.tar");
        fs::write(&destination, b"committed export").unwrap();
        let meta = fs::metadata(&destination).unwrap();
        db.execute(
            "INSERT INTO export_jobs
             (id,staging_path,created_ms,destination_path,staging_dev,state)
             VALUES(?1,?2,1,?3,?4,'published')",
            params![
                Uuid::new_v4().to_string(),
                staging.to_string_lossy().to_string(),
                destination.to_string_lossy().to_string(),
                meta.dev() as i64
            ],
        )
        .unwrap();
        recover(&db).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"committed export");
        let pending: i64 = db
            .query_row("SELECT count(*) FROM export_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn remounted_volume_cleans_only_a_tar_with_its_job_marker() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let job_id = Uuid::new_v4().to_string();
        let staging = directory
            .path()
            .join(format!(".hear-export-{job_id}.pending"));
        let destination = directory.path().join("history.tar");
        let mut builder = Builder::new(File::create(&staging).unwrap());
        archive_bytes(&mut builder, ".hear-export-id", job_id.as_bytes()).unwrap();
        builder.finish().unwrap();
        drop(builder);
        let meta = fs::metadata(&staging).unwrap();
        db.execute(
            "INSERT INTO export_jobs
             (id,staging_path,created_ms,destination_path,staging_dev)
             VALUES(?1,?2,1,?3,?4)",
            params![
                job_id,
                staging.to_string_lossy().to_string(),
                destination.to_string_lossy().to_string(),
                meta.dev() as i64 + 1
            ],
        )
        .unwrap();
        fs::rename(&staging, &destination).unwrap();
        recover(&db).unwrap();
        assert!(!destination.exists());
        let pending: i64 = db
            .query_row("SELECT count(*) FROM export_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn export_after_partial_clear_contains_only_surviving_source_and_interval() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        for source in ["microphone", "system"] {
            let id = Uuid::new_v4();
            let mut block =
                crate::media::OpusBlock::create(&directory.path().join("media").join(source), id)
                    .unwrap();
            block.push(&vec![0.1; 96_000]).unwrap();
            let file = block.seal().unwrap();
            store
                .db
                .execute(
                    "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,
              sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
              VALUES (?1,'session',?2,0,2000,0,96000,?3,?4,'retain','complete',1)",
                    params![
                        id.to_string(),
                        source,
                        file.bytes as i64,
                        format!("media/{source}/{id}.opus")
                    ],
                )
                .unwrap();
        }
        store
            .db
            .execute(
                "INSERT INTO coarse_segments VALUES
          ('mic','block','session','microphone',700,900,'secret','test'),
          ('sys','block','session','system',700,900,'preserved','test')",
                [],
            )
            .unwrap();
        let preview = cleanup::preview(
            &store.db,
            500,
            1500,
            EventSources::Microphone,
            hear_protocol::CleanupMode::AllData,
            10_000,
        )
        .unwrap();
        let scope = cleanup::verified_scope(&store.db, preview.token, true, 10_000).unwrap();
        let prepared = cleanup::prepare_media(
            directory.path(),
            &scope,
            cleanup::originals(&store.db, &scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        cleanup::apply(&tx, &scope, &prepared, 10_001).unwrap();
        tx.commit().unwrap();
        let exported = directory.path().join("selected.tar");
        let (audio, transcript) = export(
            &store.db,
            directory.path(),
            500,
            1500,
            EventSources::Both,
            true,
            false,
            &exported,
            &Mutex::new(()),
        )
        .unwrap();
        assert_eq!((audio, transcript), (1, 1));
        let mut tar = tar::Archive::new(File::open(exported).unwrap());
        let mut entries = Vec::new();
        for entry in tar.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().to_string();
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut content).unwrap();
            entries.push((name, content));
        }
        let coarse = entries
            .iter()
            .find(|(name, _)| name == "coarse.jsonl")
            .unwrap();
        assert!(!String::from_utf8_lossy(&coarse.1).contains("secret"));
        assert!(String::from_utf8_lossy(&coarse.1).contains("preserved"));
        let audio = entries
            .iter()
            .filter(|(name, _)| name.starts_with("audio/"))
            .collect::<Vec<_>>();
        assert_eq!(audio.len(), 1);
        assert!(audio[0].0.starts_with("audio/system/"));
        assert_eq!(audio[0].1.len(), 44 + 48_000 * 2);
    }
}
