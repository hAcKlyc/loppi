//! Durable coarse recognition jobs reference immutable source media. VAD and
//! retention commit independently of the heavy recognizer's availability.
use crate::{analysis, media};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::path::Path;
use uuid::Uuid;

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS coarse_jobs(
      id TEXT PRIMARY KEY,block_id TEXT NOT NULL,session_id TEXT NOT NULL,source TEXT NOT NULL,
      start_sample INTEGER NOT NULL,end_sample INTEGER NOT NULL,started_ms INTEGER NOT NULL,ended_ms INTEGER NOT NULL,
      state TEXT NOT NULL CHECK(state IN ('pending','processing','complete','failed','cancelled')),
      attempts INTEGER NOT NULL DEFAULT 0,available_ms INTEGER NOT NULL DEFAULT 0,error_code TEXT,
      UNIQUE(session_id,start_sample,end_sample));
      CREATE INDEX IF NOT EXISTS coarse_jobs_queue ON coarse_jobs(source,started_ms,id) WHERE state='pending';
      CREATE INDEX IF NOT EXISTS coarse_jobs_oldest ON coarse_jobs(started_ms,id) WHERE state='pending';
      CREATE INDEX IF NOT EXISTS coarse_jobs_session_end ON coarse_jobs(session_id,end_sample);
      CREATE INDEX IF NOT EXISTS coarse_segments_session_end ON coarse_segments(session_id,ended_ms);
      CREATE INDEX IF NOT EXISTS coarse_jobs_range ON coarse_jobs(source,started_ms,ended_ms);")
      .map_err(|e|e.to_string())
}
pub fn recover(db: &Connection) -> Result<(), String> {
    db.execute(
        "UPDATE coarse_jobs SET state='pending' WHERE state='processing'",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn enqueue(
    tx: &Transaction<'_>,
    block: &crate::PendingBlock,
    segments: Vec<analysis::SpeechRegion>,
) -> Result<(), String> {
    for segment in segments {
        if segment.end_sample <= segment.start_sample
            || segment.end_sample - segment.start_sample > 16_000 * 30
            || segment.end_sample > (block.sample_start + block.frames as u64) / 3
        {
            return Err("invalid VAD segment".into());
        }
        // A later job proves only its own coverage, not that every earlier
        // block succeeded. Subtract actual published ranges so historical
        // retries can fill holes without duplicating already-owned samples.
        let started_ms = crate::map_sample(tx, &block.session_id, segment.start_sample)?;
        let ended_ms = crate::map_sample(tx, &block.session_id, segment.end_sample)?;
        let mut statement = tx
            .prepare(
                "SELECT start_sample,end_sample FROM coarse_jobs
          WHERE session_id=?1 AND start_sample<?3 AND end_sample>?2",
            )
            .map_err(|e| e.to_string())?;
        let mut covered = statement
            .query_map(
                params![
                    block.session_id,
                    segment.start_sample as i64,
                    segment.end_sample as i64
                ],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        // Pre-upgrade coarse facts have no job. Preserve their actual ranges
        // at legacy millisecond precision, even after new jobs were added.
        let mut legacy = tx
            .prepare(
                "SELECT c.started_ms,c.ended_ms FROM coarse_segments c
          WHERE c.session_id=?1 AND c.started_ms<?3 AND c.ended_ms>?2
          AND NOT EXISTS(SELECT 1 FROM coarse_jobs j WHERE j.id=c.id)",
            )
            .map_err(|e| e.to_string())?;
        for row in legacy
            .query_map(params![block.session_id, started_ms, ended_ms], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| e.to_string())?
        {
            let (a, b) = row.map_err(|e| e.to_string())?;
            covered.push((
                sample_at_ms(tx, &block.session_id, a.max(started_ms))?,
                sample_at_ms(tx, &block.session_id, b.min(ended_ms))?,
            ));
        }
        covered.sort_unstable();
        let mut cursor = segment.start_sample;
        let mut missing = Vec::new();
        for (a, b) in covered {
            if a > cursor {
                missing.push((cursor, a.min(segment.end_sample)));
            }
            cursor = cursor.max(b);
            if cursor >= segment.end_sample {
                break;
            }
        }
        if cursor < segment.end_sample {
            missing.push((cursor, segment.end_sample));
        }
        for (start, end) in missing {
            let started_ms = crate::map_sample(tx, &block.session_id, start)?;
            let ended_ms = crate::map_sample(tx, &block.session_id, end)?;
            let erased:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM clear_ranges c
          LEFT JOIN ingestion_clears g ON g.clear_id=c.id
          LEFT JOIN session_generation sg ON sg.session_id=?1
          JOIN recording_sessions s ON s.id=?1
          WHERE c.mode='all_data' AND c.source_mask&?2!=0 AND c.from_ms<?4 AND min(c.to_ms,c.committed_ms)>?3
          AND ((sg.generation IS NOT NULL AND g.generation>=sg.generation) OR (sg.generation IS NULL AND c.committed_ms>=s.started_ms)))",
          params![block.session_id,if block.source=="microphone"{1}else{2},started_ms,ended_ms],|r|r.get(0)).map_err(|e|e.to_string())?;
            if erased {
                continue;
            }
            tx.execute("INSERT OR IGNORE INTO coarse_jobs(id,block_id,session_id,source,start_sample,end_sample,started_ms,ended_ms,state)
          VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending')",params![Uuid::new_v4().to_string(),block.id,block.session_id,block.source,start as i64,end as i64,started_ms,ended_ms]).map_err(|e|e.to_string())?;
        }
    }
    Ok(())
}

fn sample_at_ms(tx: &Transaction<'_>, session: &str, ms: i64) -> Result<u64, String> {
    tx.query_row(
        "SELECT (sample_start+2)/3+(?2-started_ms)*16 FROM audio_blocks
      WHERE session_id=?1 AND started_ms<=?2 AND ended_ms>=?2 ORDER BY sample_start DESC LIMIT 1",
        params![session, ms],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .map(|n| n as u64)
    .ok_or_else(|| "speech segment could not map to saved audio".into())
}
#[derive(Clone)]
pub struct Job {
    pub id: String,
    pub block_id: String,
    pub session_id: String,
    pub source: String,
    pub start: u64,
    pub end: u64,
    pub started_ms: i64,
    pub ended_ms: i64,
}
pub fn take(
    db: &mut Connection,
    source: &str,
    newest: bool,
    now: i64,
) -> Result<Option<Job>, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let sql=format!("SELECT id,block_id,session_id,source,start_sample,end_sample,started_ms,ended_ms FROM coarse_jobs
      WHERE state='pending' AND source=?1 AND available_ms<=?2 ORDER BY started_ms {},id LIMIT 1",if newest{"DESC"}else{"ASC"});
    let job = tx
        .query_row(&sql, params![source, now], |r| {
            Ok(Job {
                id: r.get(0)?,
                block_id: r.get(1)?,
                session_id: r.get(2)?,
                source: r.get(3)?,
                start: r.get::<_, i64>(4)? as u64,
                end: r.get::<_, i64>(5)? as u64,
                started_ms: r.get(6)?,
                ended_ms: r.get(7)?,
            })
        })
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(job) = &job {
        tx.execute(
            "UPDATE coarse_jobs SET state='processing' WHERE id=?1",
            [&job.id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(job)
}
pub struct Part {
    pub path: String,
    pub start: u64,
    pub frames: usize,
}
pub fn manifest(db: &Connection, job: &Job) -> Result<Vec<Part>, String> {
    let mut stmt=db.prepare("SELECT media_path,sample_start,frames FROM audio_blocks WHERE session_id=?1
      AND sample_start/3<?3 AND (sample_start+frames)/3>?2 AND media_path IS NOT NULL ORDER BY sample_start LIMIT 64").map_err(|e|e.to_string())?;
    let parts = stmt
        .query_map(
            params![job.session_id, job.start as i64, job.end as i64],
            |r| {
                Ok(Part {
                    path: r.get(0)?,
                    start: r.get::<_, i64>(1)? as u64,
                    frames: r.get::<_, i64>(2)? as usize,
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if parts.len() == 64 {
        return Err("coarse audio fragmentation exceeded limit".into());
    }
    Ok(parts)
}
pub fn pcm(directory: &Path, job: &Job, parts: Vec<Part>) -> Result<Vec<f32>, String> {
    let length = job
        .end
        .checked_sub(job.start)
        .ok_or("invalid coarse samples")?;
    if length == 0 || length > 16_000 * 30 {
        return Err("invalid coarse duration".into());
    }
    let mut output = Vec::with_capacity(length as usize);
    let mut cursor = job.start;
    for part in parts {
        if !(1..=48_000 * 11).contains(&part.frames) {
            return Err("invalid coarse block".into());
        }
        let path = Path::new(&part.path);
        if path.is_absolute()
            || path
                .components()
                .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            return Err("invalid coarse media path".into());
        }
        let decoded = media::decode_block(&directory.join(path), part.frames)?;
        // Match the VAD's exact independent-block resampling, including padding.
        let samples = analysis::resample_block(part.start, &decoded)?;
        let from = (part.start / 3).max(job.start);
        let to = (part.start / 3 + samples.len() as u64).min(job.end);
        if from != cursor || to <= from {
            return Err("coarse source audio unavailable".into());
        }
        output.extend_from_slice(
            &samples[(from - part.start / 3) as usize..(to - part.start / 3) as usize],
        );
        cursor = to;
    }
    if cursor != job.end {
        return Err("coarse source audio unavailable".into());
    }
    Ok(output)
}
pub fn publish(db: &mut Connection, job: &Job, text: &str) -> Result<bool, String> {
    if text.len() > 100_000 {
        return Err("coarse response exceeded limit".into());
    }
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let current: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM coarse_jobs WHERE id=?1 AND state='processing')",
            [&job.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !current {
        return Ok(false);
    }
    tx.execute("INSERT OR IGNORE INTO coarse_segments(id,block_id,session_id,source,started_ms,ended_ms,text,model_version)
      VALUES(?1,?2,?3,?4,?5,?6,?7,'sensevoice-int8-2024-07-17')",params![job.id,job.block_id,job.session_id,job.source,job.started_ms,job.ended_ms,text]).map_err(|e|e.to_string())?;
    tx.execute(
        "UPDATE coarse_jobs SET state='complete',error_code=NULL WHERE id=?1",
        [&job.id],
    )
    .map_err(|e| e.to_string())?;
    crate::events::note_analysis(&tx, &job.block_id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}
pub fn failed(db: &Connection, job: &Job, error: &str, now: i64) -> Result<(), String> {
    if error == "resource_deferred" {
        db.execute("UPDATE coarse_jobs SET state='pending',available_ms=?2,error_code='resource_deferred' WHERE id=?1 AND state='processing'",params![job.id,now+10_000]).map_err(|e|e.to_string())?;
    } else {
        db.execute(
            "UPDATE coarse_jobs SET state=CASE WHEN attempts>=2 THEN 'failed' ELSE 'pending' END,
          attempts=attempts+1,available_ms=?2+min(300000,5000*(1<<min(attempts,6))),error_code=?3
          WHERE id=?1 AND state='processing'",
            params![job.id, now, error],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, analysis::SpeechRegion, analysis::VadResult, record};
    use hear_protocol::{AudioSource, CleanupMode, EventSources};
    fn setup() -> (tempfile::TempDir, Store, crate::PendingBlock, i64) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let id = Uuid::new_v4();
        let clock = record::ClockAnchor::now().unwrap();
        let wall = clock.wall_ms;
        store
            .start_recording_session(id, AudioSource::Microphone, Uuid::new_v4(), clock)
            .unwrap();
        for (name, start, speech) in [("a", 0, None), ("b", 240000, Some(0))] {
            store.db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
              VALUES(?1,?2,'microphone',?3,?3+5000,?4,240000,100,?5,'pending_vad_v2',?6,?7)",params![name,id.to_string(),wall+start/48,start,format!("media/microphone/{name}.opus"),if name=="a"{"complete"}else{"processing"},speech]).unwrap();
        }
        let block = crate::PendingBlock {
            id: "b".into(),
            started_ms: wall + 5000,
            ended_ms: wall + 10000,
            session_id: id.to_string(),
            source: "microphone".into(),
            sample_start: 240000,
            frames: 240000,
            media_path: "media/microphone/b.opus".into(),
            flush: true,
        };
        (dir, store, block, wall)
    }
    fn decision() -> VadResult {
        VadResult {
            segments: vec![SpeechRegion {
                start_sample: 76_800,
                end_sample: 79_840,
            }],
            speech_active: false,
            speech_pending: false,
            error: None,
        }
    }
    fn clear(store: &mut Store, dir: &Path, from: i64, to: i64, mode: CleanupMode) {
        let now = to + 1000;
        // No actual files are needed for full-range clearing.
        let preview =
            crate::cleanup::preview(&store.db, from, to, EventSources::Microphone, mode, now)
                .unwrap();
        let scope = crate::cleanup::verified_scope(&store.db, preview.token, true, now).unwrap();
        let prepared = crate::cleanup::prepare_media(
            dir,
            &scope,
            crate::cleanup::originals(&store.db, &scope).unwrap(),
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        crate::cleanup::apply(&tx, &scope, &prepared, now).unwrap();
        tx.commit().unwrap();
    }
    #[test]
    fn vad_preserves_sentence_head_before_recognition_and_silence_reclaims_independently() {
        let (_, mut store, block, _) = setup();
        store.publish_analysis(&block, decision()).unwrap();
        let a: (String, i64) = store
            .db
            .query_row(
                "SELECT retention_decision,speech_active FROM audio_blocks WHERE id='a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(a, ("retain_speech_v2".into(), 1));
        let b: String = store
            .db
            .query_row(
                "SELECT retention_decision FROM audio_blocks WHERE id='b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b, "discard_no_speech_v2");
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM coarse_segments", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let job = take(&mut store.db, "microphone", false, i64::MAX)
            .unwrap()
            .unwrap();
        for i in 0..10 {
            failed(&store.db, &job, "resource_deferred", i).unwrap();
            assert_eq!(
                store
                    .db
                    .query_row("SELECT attempts FROM coarse_jobs", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            take(&mut store.db, "microphone", false, i64::MAX)
                .unwrap()
                .unwrap();
        }
        recover(&store.db).unwrap();
        let job = take(&mut store.db, "microphone", false, i64::MAX)
            .unwrap()
            .unwrap();
        let published = publish(&mut store.db, &job, "");
        let event = crate::coarse_log_event(&job, &published, "coarse_recognize");
        assert!(published.unwrap());
        assert_eq!(event.event, "coarse_complete");
        let duplicate = publish(&mut store.db, &job, "duplicate");
        let event = crate::coarse_log_event(&job, &duplicate, "coarse_recognize");
        assert!(!duplicate.unwrap());
        assert_eq!(event.event, "coarse_not_published");
        let kept: bool = store
            .db
            .query_row(
                "SELECT media_path IS NOT NULL FROM audio_blocks WHERE id='a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(kept);
    }
    #[test]
    fn cleanup_cancels_late_recognition_for_both_modes() {
        for mode in [CleanupMode::AllData, CleanupMode::RawAudio] {
            let (dir, mut store, block, wall) = setup();
            store.publish_analysis(&block, decision()).unwrap();
            let job = take(&mut store.db, "microphone", false, i64::MAX)
                .unwrap()
                .unwrap();
            clear(&mut store, dir.path(), wall, wall + 10000, mode);
            let published = publish(&mut store.db, &job, "must not return");
            let event = crate::coarse_log_event(&job, &published, "coarse_recognize");
            assert!(!published.unwrap());
            assert_eq!(event.event, "coarse_not_published");
            assert_eq!(event.level, hear_protocol::LogLevel::Info);
            assert!(event.error_code.is_none());
            assert_eq!(event.job_id, Uuid::parse_str(&job.id).ok());
            assert_eq!(
                store
                    .db
                    .query_row("SELECT count(*) FROM coarse_segments", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
    #[test]
    fn late_vad_cannot_reintroduce_an_erased_utterance() {
        let (_, mut store, block, wall) = setup();
        store
            .db
            .execute(
                "INSERT INTO clear_ranges VALUES(?1,?2,?3,1,'all_data',?4)",
                params![
                    Uuid::new_v4().to_string(),
                    wall + 4800,
                    wall + 4990,
                    wall + 10000
                ],
            )
            .unwrap();
        store.publish_analysis(&block, decision()).unwrap();
        assert!(
            take(&mut store.db, "microphone", false, i64::MAX)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn replay_publishes_each_confirmed_speech_range_only_once() {
        let (_, mut store, block, _) = setup();
        let tx = store.db.transaction().unwrap();
        enqueue(&tx, &block, decision().segments).unwrap();
        enqueue(
            &tx,
            &block,
            vec![SpeechRegion {
                start_sample: 76_000,
                end_sample: 79_840,
            }],
        )
        .unwrap();
        enqueue(
            &tx,
            &block,
            vec![SpeechRegion {
                start_sample: 76_000,
                end_sample: 79_840,
            }],
        )
        .unwrap();
        let ranges = tx
            .prepare("SELECT start_sample,end_sample FROM coarse_jobs ORDER BY start_sample")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ranges, [(76_000, 76_800), (76_800, 79_840)]);
    }
    #[test]
    fn historical_retry_fills_hole_before_later_jobs_and_protects_onset() {
        let (_, mut store, block, wall) = setup();
        store.db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES('c',?1,'microphone',?2,?2+5000,480000,240000,100,'media/microphone/c.opus','retain_speech_v2','complete',1)", params![block.session_id,wall+10000]).unwrap();
        store.db.execute("INSERT INTO coarse_jobs(id,block_id,session_id,source,start_sample,end_sample,started_ms,ended_ms,state)
          VALUES('later','c',?1,'microphone',160000,176000,?2,?3,'complete')", params![block.session_id,wall+10000,wall+11000]).unwrap();
        let result = || VadResult {
            segments: vec![SpeechRegion {
                start_sample: 78_400,
                end_sample: 96_000,
            }],
            speech_active: true,
            speech_pending: false,
            error: None,
        };
        store.publish_analysis(&block, result()).unwrap();
        assert_eq!(store.db.query_row("SELECT retention_decision FROM audio_blocks WHERE id='a' AND media_path IS NOT NULL", [], |r| r.get::<_,String>(0)).unwrap(), "retain_speech_v2");
        assert_eq!(store.db.query_row("SELECT count(*) FROM coarse_jobs WHERE start_sample=78400 AND end_sample=96000", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM media_deletion_jobs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        store
            .db
            .execute(
                "UPDATE audio_blocks SET analysis_state='processing' WHERE id='b'",
                [],
            )
            .unwrap();
        store.publish_analysis(&block, result()).unwrap();
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM coarse_jobs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn recovery_fills_only_uncovered_samples_between_existing_ranges() {
        let (_, mut store, block, _) = setup();
        let tx = store.db.transaction().unwrap();
        enqueue(
            &tx,
            &block,
            vec![
                SpeechRegion {
                    start_sample: 80_000,
                    end_sample: 90_000,
                },
                SpeechRegion {
                    start_sample: 110_000,
                    end_sample: 120_000,
                },
            ],
        )
        .unwrap();
        for _ in 0..2 {
            enqueue(
                &tx,
                &block,
                vec![SpeechRegion {
                    start_sample: 78_400,
                    end_sample: 128_000,
                }],
            )
            .unwrap();
        }
        let ranges = tx
            .prepare("SELECT start_sample,end_sample FROM coarse_jobs ORDER BY start_sample")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            ranges,
            [
                (78_400, 80_000),
                (80_000, 90_000),
                (90_000, 110_000),
                (110_000, 120_000),
                (120_000, 128_000)
            ]
        );
        assert_eq!(
            enqueue(
                &tx,
                &block,
                vec![SpeechRegion {
                    start_sample: 128_000,
                    end_sample: 128_000
                }]
            )
            .unwrap_err(),
            "invalid VAD segment"
        );
    }
    #[test]
    fn replay_persists_unpublished_suffix_and_respects_pre_upgrade_segments() {
        for legacy in [false, true] {
            let (_, mut store, block, wall) = setup();
            let tx = store.db.transaction().unwrap();
            if legacy {
                tx.execute("INSERT INTO coarse_segments VALUES('legacy','a',?1,'microphone',?2,?3,'prior','sensevoice-int8-v1')",
                  params![block.session_id,wall+1000,wall+3000]).unwrap();
            } else {
                enqueue(
                    &tx,
                    &block,
                    vec![SpeechRegion {
                        start_sample: 16_000,
                        end_sample: 48_000,
                    }],
                )
                .unwrap();
            }
            // Reconstructed VAD emitted an overlapping region during replay.
            enqueue(
                &tx,
                &block,
                vec![SpeechRegion {
                    start_sample: 32_000,
                    end_sample: 64_000,
                }],
            )
            .unwrap();
            enqueue(
                &tx,
                &block,
                vec![SpeechRegion {
                    start_sample: 32_000,
                    end_sample: 64_000,
                }],
            )
            .unwrap();
            let tail:(i64,i64)=tx.query_row("SELECT start_sample,end_sample FROM coarse_jobs ORDER BY end_sample DESC LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
            assert_eq!(tail, (48_000, 64_000));
            assert_eq!(
                tx.query_row("SELECT count(*) FROM coarse_jobs", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                if legacy { 1 } else { 2 }
            );
        }
    }
    #[test]
    fn legacy_watermark_preserves_milliseconds_after_nondivisible_capture_gap() {
        let (_, mut store, mut block, wall) = setup();
        store.db.execute("UPDATE audio_blocks SET sample_start=100000,started_ms=?1,ended_ms=?1+5000 WHERE id='b'",[wall+2000]).unwrap();
        block.sample_start = 100000;
        store.db.execute("INSERT INTO coarse_segments VALUES('legacy','b',?1,'microphone',?2,?3,'prior','sensevoice-int8-v1')",params![block.session_id,wall+2000,wall+5000]).unwrap();
        let tx = store.db.transaction().unwrap();
        enqueue(
            &tx,
            &block,
            vec![SpeechRegion {
                start_sample: 70_000,
                end_sample: 96_000,
            }],
        )
        .unwrap();
        let (sample, ms): (i64, i64) = tx
            .query_row("SELECT start_sample,started_ms FROM coarse_jobs", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(sample, 81_334);
        assert_eq!(ms, wall + 5000);
        assert_eq!(
            crate::map_sample(&tx, &block.session_id, sample as u64).unwrap(),
            wall + 5000
        );
    }
    #[test]
    fn replay_snapshot_cannot_revive_completed_full_cleanup() {
        let (dir, mut store, block, wall) = setup();
        clear(
            &mut store,
            dir.path(),
            wall,
            wall + 10000,
            CleanupMode::AllData,
        );
        let tx = store.db.transaction().unwrap();
        // A worker can still hold decoded replay input after the clear commits.
        // A missing sample map or tombstone both prevent publication.
        let _ = enqueue(&tx, &block, decision().segments);
        assert_eq!(
            tx.query_row("SELECT count(*) FROM coarse_jobs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            tx.query_row("SELECT count(*) FROM coarse_segments", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn recognition_uses_exact_vad_resampling_across_independent_opus_blocks() {
        let (dir, store, block, wall) = setup();
        let mut all = Vec::new();
        for (name, start) in [("a", 0), ("b", 240000)] {
            let id = Uuid::new_v4();
            let mut encoder =
                media::OpusBlock::create(&dir.path().join("media/microphone"), id).unwrap();
            let raw: Vec<f32> = (0..240000)
                .map(|i| ((i + start) as f32 * 0.013).sin() * 0.1)
                .collect();
            encoder.push(&raw).unwrap();
            let sealed = encoder.seal().unwrap();
            let decoded = media::decode_block(&sealed.path, 240000).unwrap();
            all.extend(analysis::resample_block(start as u64, &decoded).unwrap());
            store
                .db
                .execute(
                    "UPDATE audio_blocks SET media_path=?2 WHERE id=?1",
                    params![
                        name,
                        sealed
                            .path
                            .strip_prefix(dir.path())
                            .unwrap()
                            .to_string_lossy()
                    ],
                )
                .unwrap();
        }
        let job = Job {
            id: Uuid::new_v4().to_string(),
            block_id: block.id,
            session_id: block.session_id,
            source: block.source,
            start: 79001,
            end: 90007,
            started_ms: wall,
            ended_ms: wall + 1,
        };
        let actual = pcm(dir.path(), &job, manifest(&store.db, &job).unwrap()).unwrap();
        assert_eq!(actual, &all[79001..90007]);
    }
}
