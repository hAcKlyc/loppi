use base64::Engine;
use loppi_protocol::EventSources;
use rusqlite::{Connection, params};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn generation(db: &Connection) -> Result<i64, String> {
    db.query_row(
        "SELECT generation FROM ingestion_state WHERE id=1",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Reuse the core's committed cleanup generations. No audio is decoded and no
/// writer/cleanup lock is held; only clears since the fetched chunk matter.
pub fn valid(
    db: &Connection,
    from: i64,
    to: i64,
    sources: EventSources,
    generation: i64,
) -> Result<bool, String> {
    if from < 0 || to <= from || generation < 0 {
        return Err("invalid playback validity range".into());
    }
    let mask = match sources {
        EventSources::Microphone => 1,
        EventSources::System => 2,
        EventSources::Both => 3,
    };
    db.query_row("SELECT ?1<=(SELECT generation FROM ingestion_state WHERE id=1) AND NOT EXISTS(SELECT 1 FROM ingestion_clears g JOIN clear_ranges c ON c.id=g.clear_id
      WHERE g.generation>=?1 AND c.source_mask&?2!=0 AND c.from_ms<?4 AND min(c.to_ms,c.committed_ms)>?3)",
      params![generation,mask,from,to], |r| r.get(0)).map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn committed_cleanup_revokes_only_matching_source_range_and_generation() {
        let mut store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let baseline = generation(&store.db).unwrap();
        assert!(valid(&store.db, 0, 15_000, EventSources::Both, baseline).unwrap());
        // Preview and rolled-back cleanup cannot invalidate a fetched buffer.
        {
            let tx = store.db.transaction().unwrap();
            tx.execute(
                "INSERT INTO clear_ranges VALUES('rolled_back',5000,10000,1,'raw_audio',20000)",
                [],
            )
            .unwrap();
        }
        assert_eq!(generation(&store.db).unwrap(), baseline);
        store
            .db
            .execute(
                "INSERT INTO clear_ranges VALUES('committed',5000,10000,1,'raw_audio',20000)",
                [],
            )
            .unwrap();
        assert!(!valid(&store.db, 0, 15_000, EventSources::Microphone, baseline).unwrap());
        assert!(!valid(&store.db, 0, 15_000, EventSources::Both, baseline).unwrap());
        assert!(valid(&store.db, 0, 15_000, EventSources::System, baseline).unwrap());
        assert!(valid(&store.db, 10_000, 15_000, EventSources::Both, baseline).unwrap());
        assert!(
            valid(
                &store.db,
                0,
                15_000,
                EventSources::Both,
                generation(&store.db).unwrap()
            )
            .unwrap()
        );
        assert!(!valid(&store.db, 0, 15_000, EventSources::Both, 99).unwrap());
    }
}

pub fn chunk(
    db: &Connection,
    directory: &Path,
    from: i64,
    to: i64,
    sources: EventSources,
) -> Result<(String, bool, Option<i64>), String> {
    if from < 0 || to <= from || to - from > 5_000 {
        return Err("playback chunk must be at most five seconds".into());
    }
    let frames = (to - from) as usize * 48;
    let mut pcm = vec![0.0_f32; frames];
    let mask = match sources {
        EventSources::Microphone => 1,
        EventSources::System => 2,
        EventSources::Both => 3,
    };
    let mut statement = db
        .prepare(
            "SELECT id,source,started_ms,ended_ms,frames,media_path FROM audio_blocks
         WHERE media_path IS NOT NULL AND started_ms<?2 AND ended_ms>?1
         AND source IN (CASE WHEN ?3&1!=0 THEN 'microphone' END, CASE WHEN ?3&2!=0 THEN 'system' END)
         ORDER BY started_ms,id",
        )
        .map_err(|error| error.to_string())?;
    let blocks = statement
        .query_map(params![from, to, mask], |row| {
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
        .map_err(|error| error.to_string())?;
    let mut has_audio = false;
    for (id, source, a, b, count, relative) in blocks {
        if b <= a || count <= 0 || count > 48_000 * 11 || Uuid::parse_str(&id).is_err() {
            return Err("playback manifest invalid".into());
        }
        let expected = PathBuf::from("media")
            .join(&source)
            .join(format!("{id}.opus"));
        if !matches!(source.as_str(), "microphone" | "system") || Path::new(&relative) != expected {
            return Err("playback media path invalid".into());
        }
        let path = directory.join(expected);
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err("playback media is unavailable".into());
        }
        let decoded = crate::media::decode_block(&path, count as usize)?;
        let clip_start = a.max(from);
        let clip_end = b.min(to);
        let src = (clip_start - a) as usize * 48;
        let dst = (clip_start - from) as usize * 48;
        let length = ((clip_end - clip_start) as usize * 48)
            .min(decoded.len().saturating_sub(src))
            .min(pcm.len().saturating_sub(dst));
        if length > 0 {
            for index in 0..length {
                pcm[dst + index] = (pcm[dst + index] + decoded[src + index]).clamp(-1.0, 1.0);
            }
            has_audio = true;
        }
    }
    let next_audio_ms = if has_audio {
        None
    } else {
        db.query_row(
            "SELECT min(started_ms) FROM audio_blocks WHERE media_path IS NOT NULL
             AND started_ms>=?1 AND ((source='microphone' AND ?2&1!=0)
             OR (source='system' AND ?2&2!=0))",
            params![to, mask],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(|error| error.to_string())?
    };
    let wav = crate::archive::wav_mono_48k(&pcm)?;
    Ok((
        base64::engine::general_purpose::STANDARD.encode(wav),
        has_audio,
        next_audio_ms,
    ))
}
