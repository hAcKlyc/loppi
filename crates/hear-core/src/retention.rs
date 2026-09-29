use rusqlite::{OptionalExtension, Transaction, params};

/// Audio is sealed before local analysis so a slow or failed worker cannot
/// create a capture gap. The current policy discards non-speech after VAD has
/// analyzed it; one adjacent block is kept temporarily because VAD can return
/// an utterance that began near the previous block's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    DiscardDigitalSilence,
    AwaitSpeechDecision,
}

impl Retention {
    pub fn code(self) -> &'static str {
        match self {
            Self::DiscardDigitalSilence => "discard_digital_silence_v1",
            Self::AwaitSpeechDecision => "pending_vad_v2",
        }
    }
}

pub fn classify(mono: &[f32]) -> Retention {
    if mono.iter().all(|sample| *sample == 0.0) {
        Retention::DiscardDigitalSilence
    } else {
        Retention::AwaitSpeechDecision
    }
}

const PENDING: &str = "pending_vad_v2";

#[derive(Default)]
pub struct Resolution {
    pub deleted_media: bool,
    pub newly_speech: Vec<String>,
}

/// Called after the worker's segments and speech flag have been written in
/// the same transaction. Old policy blocks are deliberately left untouched:
/// changing the policy must not silently erase recordings from before upgrade.
pub fn resolve_analysis(
    tx: &Transaction<'_>,
    session_id: &str,
    sample_start: u64,
    current_id: &str,
    current_flush: bool,
    now_ms: i64,
) -> Result<Resolution, String> {
    let mut result = Resolution::default();
    let previous = tx
        .query_row(
            "SELECT id FROM audio_blocks WHERE session_id=?1
             AND sample_start+frames=?2 AND retention_decision=?3
             AND analysis_state='complete' AND media_path IS NOT NULL LIMIT 1",
            params![session_id, sample_start as i64, PENDING],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(previous_id) = previous {
        resolve_block(tx, &previous_id, true, now_ms, &mut result)?;
    }
    resolve_block(tx, current_id, current_flush, now_ms, &mut result)?;
    Ok(result)
}

fn resolve_block(
    tx: &Transaction<'_>,
    id: &str,
    may_discard: bool,
    now_ms: i64,
    result: &mut Resolution,
) -> Result<(), String> {
    let block = tx
        .query_row(
            "SELECT session_id,source,started_ms,ended_ms,media_path,coalesce(speech_active,0)
             FROM audio_blocks WHERE id=?1 AND retention_decision=?2
             AND analysis_state='complete' AND media_path IS NOT NULL",
            params![id, PENDING],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((session_id, source, started_ms, ended_ms, path, speech_active)) = block else {
        return Ok(());
    };
    let has_segment: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM coarse_segments WHERE session_id=?1 AND source=?2
             AND started_ms<?4 AND ended_ms>?3)",
            params![session_id, source, started_ms, ended_ms],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if speech_active || has_segment {
        tx.execute(
            "UPDATE audio_blocks SET retention_decision='retain_speech_v2',speech_active=1
             WHERE id=?1 AND retention_decision=?2",
            params![id, PENDING],
        )
        .map_err(|error| error.to_string())?;
        if !speech_active {
            result.newly_speech.push(id.to_owned());
        }
    } else if may_discard {
        tx.execute(
            "UPDATE audio_blocks SET media_path=NULL,bytes=0,
             retention_decision='discard_no_speech_v2',speech_active=0
             WHERE id=?1 AND retention_decision=?2 AND media_path=?3",
            params![id, PENDING, path],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT OR IGNORE INTO media_deletion_jobs
             (media_path,created_ms,completed_ms,error_code) VALUES (?1,?2,NULL,NULL)",
            params![path, now_ms],
        )
        .map_err(|error| error.to_string())?;
        result.deleted_media = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn store() -> crate::Store {
        crate::Store::new(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn block(store: &crate::Store, id: &str, sample_start: i64, decision: &str, speech: i64) {
        store
            .db
            .execute(
                "INSERT INTO audio_blocks
             (id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,
              media_path,retention_decision,analysis_state,speech_active)
             VALUES (?1,'session','system',?2,?3,?4,240000,100,?5,?6,'complete',?7)",
                params![
                    id,
                    sample_start / 48,
                    sample_start / 48 + 5_000,
                    sample_start,
                    format!("media/system/{id}.opus"),
                    decision,
                    speech
                ],
            )
            .unwrap();
    }

    #[test]
    fn nonzero_audio_awaits_worker_decision() {
        assert_eq!(classify(&[0.0; 48_000]), Retention::DiscardDigitalSilence);
        assert_eq!(
            classify(&[0.0, 0.000_001, 0.0]),
            Retention::AwaitSpeechDecision
        );
        assert_eq!(classify(&[0.0, f32::NAN]), Retention::AwaitSpeechDecision);
    }

    #[test]
    fn no_speech_waits_for_the_next_block_then_schedules_durable_deletion() {
        let mut store = store();
        block(&store, "a", 0, PENDING, 0);
        block(&store, "b", 240_000, PENDING, 0);
        let tx = store.db.transaction().unwrap();
        let resolved = resolve_analysis(&tx, "session", 240_000, "b", false, 10_000).unwrap();
        assert!(resolved.deleted_media);
        assert!(resolved.newly_speech.is_empty());
        tx.commit().unwrap();
        let a: (Option<String>, i64, String) = store
            .db
            .query_row(
                "SELECT media_path,bytes,retention_decision FROM audio_blocks WHERE id='a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(a, (None, 0, "discard_no_speech_v2".to_owned()));
        let b: String = store
            .db
            .query_row(
                "SELECT retention_decision FROM audio_blocks WHERE id='b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b, PENDING);
        let jobs: i64 = store
            .db
            .query_row(
                "SELECT count(*) FROM media_deletion_jobs WHERE completed_ms IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(jobs, 1);
    }

    #[test]
    fn late_sentence_protects_the_previous_block() {
        let mut store = store();
        block(&store, "a", 0, PENDING, 0);
        block(&store, "b", 240_000, PENDING, 0);
        store
            .db
            .execute(
                "INSERT INTO coarse_segments VALUES ('sentence','b','session','system',
             4800,4990,'hello','model')",
                [],
            )
            .unwrap();
        let tx = store.db.transaction().unwrap();
        let resolved = resolve_analysis(&tx, "session", 240_000, "b", true, 10_000).unwrap();
        assert_eq!(resolved.newly_speech, vec!["a"]);
        assert!(resolved.deleted_media);
        tx.commit().unwrap();
        let a: (String, i64) = store
            .db
            .query_row(
                "SELECT retention_decision,speech_active FROM audio_blocks WHERE id='a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(a, ("retain_speech_v2".to_owned(), 1));
        let b: String = store
            .db
            .query_row(
                "SELECT retention_decision FROM audio_blocks WHERE id='b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b, "discard_no_speech_v2");
    }

    #[test]
    fn speech_without_text_stays_and_old_policy_is_not_reclassified() {
        let mut store = store();
        block(&store, "old", 0, "pending_conservative_v1", 0);
        block(&store, "speech", 240_000, PENDING, 1);
        let tx = store.db.transaction().unwrap();
        let resolved = resolve_analysis(&tx, "session", 240_000, "speech", true, 10_000).unwrap();
        assert!(!resolved.deleted_media);
        tx.commit().unwrap();
        let decisions: Vec<String> = store
            .db
            .prepare("SELECT retention_decision FROM audio_blocks ORDER BY sample_start")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            decisions,
            vec!["pending_conservative_v1", "retain_speech_v2"]
        );
    }
}
