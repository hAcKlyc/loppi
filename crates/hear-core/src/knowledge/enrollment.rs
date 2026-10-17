//! Enrollment is a consequence of explicit human identity confirmation, never
//! of an automatic match. Core owns the evidence and rechecks it at publication.
use super::*;

const MIN_VOICED_MS: i64 = 3_000;
const TARGET_MS: i64 = 6_000;
const MAX_COMBINED_MS: i64 = 12_000;
const MAX_GAP_MS: i64 = 1_500;
const PER_SCENE: usize = 3;
const PER_PERSON: usize = 24;
const DUPLICATE_SIMILARITY: f32 = 0.96;

#[derive(Clone, Copy, Debug)]
pub struct VoiceQuality {
    pub voiced_ms: i64,
    pub score: i64,
}

/// A deliberately modest quality proxy, not a calibrated speaker confidence.
/// Count 10 ms frames with sufficient energy; reject clipping and mostly silence.
pub fn enrollment_quality(pcm: &[f32]) -> Option<VoiceQuality> {
    if pcm.is_empty() || pcm.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let clipped = pcm.iter().filter(|v| v.abs() >= 0.98).count();
    if clipped * 100 > pcm.len() {
        return None;
    }
    let frames = pcm.as_chunks::<160>().0.iter();
    let total = frames.len();
    let voiced = frames
        .filter(|frame| frame.iter().map(|v| v * v).sum::<f32>() / 160.0 >= 0.005 * 0.005)
        .count();
    let voiced_ms = voiced as i64 * 10;
    if voiced_ms < MIN_VOICED_MS || voiced * 100 < total * 65 {
        return None;
    }
    Some(VoiceQuality {
        voiced_ms,
        score: voiced_ms.min(10_000) * voiced as i64 / total.max(1) as i64,
    })
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EnrollmentPart {
    pub segment_id: Uuid,
    binding_id: String,
    pub start: i64,
    pub end: i64,
}

#[derive(Clone, Debug)]
pub struct EnrollmentWork {
    pub id: Uuid,
    person: Uuid,
    pub source: String,
    bucket: String,
    pub parts: Vec<EnrollmentPart>,
}

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS voice_observation_quality(
      segment_id TEXT PRIMARY KEY, voiced_ms INTEGER NOT NULL, score INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS voice_sample_quality(
      sample_id TEXT PRIMARY KEY, bucket TEXT NOT NULL, score INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS voice_sample_parts(
      sample_id TEXT NOT NULL, segment_id TEXT NOT NULL, binding_id TEXT NOT NULL,
      PRIMARY KEY(sample_id,segment_id));
      CREATE INDEX IF NOT EXISTS voice_sample_parts_segment ON voice_sample_parts(segment_id);
      CREATE INDEX IF NOT EXISTS voice_sample_parts_binding ON voice_sample_parts(binding_id);
      CREATE TABLE IF NOT EXISTS voice_enrollments(
      id TEXT PRIMARY KEY, person_id TEXT NOT NULL, source TEXT NOT NULL,
      bucket TEXT NOT NULL, started_ms INTEGER NOT NULL, ended_ms INTEGER NOT NULL,
      state TEXT NOT NULL CHECK(state IN ('pending','processing','complete','unavailable','stale')),
      error_code TEXT, attempts INTEGER NOT NULL DEFAULT 0,
      available_ms INTEGER NOT NULL DEFAULT 0, updated_ms INTEGER NOT NULL,
      vector BLOB, score INTEGER);
      CREATE INDEX IF NOT EXISTS voice_enrollments_pending ON voice_enrollments(state,available_ms);
      CREATE INDEX IF NOT EXISTS voice_enrollments_ready ON voice_enrollments(updated_ms,score DESC,id)
        WHERE state='pending' AND vector IS NOT NULL;
      CREATE TABLE IF NOT EXISTS voice_enrollment_parts(
      job_id TEXT NOT NULL, segment_id TEXT NOT NULL, binding_id TEXT NOT NULL,
      started_ms INTEGER NOT NULL, ended_ms INTEGER NOT NULL,
      PRIMARY KEY(job_id,segment_id));
      CREATE INDEX IF NOT EXISTS voice_enrollment_parts_binding ON voice_enrollment_parts(binding_id);
      CREATE INDEX IF NOT EXISTS voice_enrollment_parts_segment ON voice_enrollment_parts(segment_id);
      CREATE TRIGGER IF NOT EXISTS enrollment_binding_update AFTER UPDATE ON speaker_bindings
      WHEN old.origin='manual' AND old.state='active' AND
        (new.state!='active' OR new.person_id!=old.person_id OR new.origin!='manual'
         OR new.current_segment_id IS NOT old.current_segment_id) BEGIN
        UPDATE voice_samples SET revoked_ms=new.updated_ms WHERE revoked_ms IS NULL AND
          id IN(SELECT sample_id FROM voice_sample_parts WHERE binding_id=old.id);
        UPDATE voice_enrollments SET state='stale',error_code='identity_changed',updated_ms=new.updated_ms
          WHERE state IN('pending','processing') AND id IN
          (SELECT job_id FROM voice_enrollment_parts WHERE binding_id=old.id);
        UPDATE identity_state SET generation=generation+1 WHERE id=1;
      END;
      CREATE TRIGGER IF NOT EXISTS enrollment_binding_delete AFTER DELETE ON speaker_bindings BEGIN
        UPDATE voice_samples SET revoked_ms=old.updated_ms WHERE revoked_ms IS NULL AND
          id IN(SELECT sample_id FROM voice_sample_parts WHERE binding_id=old.id);
        UPDATE voice_enrollments SET state='stale',error_code='identity_changed'
          WHERE state IN('pending','processing') AND id IN
          (SELECT job_id FROM voice_enrollment_parts WHERE binding_id=old.id);
        UPDATE identity_state SET generation=generation+1 WHERE id=1;
      END;
      CREATE TRIGGER IF NOT EXISTS enrollment_segment_delete AFTER DELETE ON precise_segments BEGIN
        UPDATE voice_samples SET revoked_ms=created_ms WHERE revoked_ms IS NULL AND id IN
          (SELECT sample_id FROM voice_sample_parts WHERE segment_id=old.id);
        DELETE FROM voice_enrollments WHERE id IN
          (SELECT job_id FROM voice_enrollment_parts WHERE segment_id=old.id);
        DELETE FROM voice_observation_quality WHERE segment_id=old.id;
        UPDATE identity_state SET generation=generation+1 WHERE id=1;
      END;
      CREATE TRIGGER IF NOT EXISTS enrollment_sample_delete AFTER DELETE ON voice_samples BEGIN
        DELETE FROM voice_sample_quality WHERE sample_id=old.id;
        DELETE FROM voice_sample_parts WHERE sample_id=old.id;
        UPDATE identity_state SET generation=generation+1 WHERE id=1;
      END;
      CREATE TRIGGER IF NOT EXISTS enrollment_job_delete AFTER DELETE ON voice_enrollments BEGIN
        DELETE FROM voice_enrollment_parts WHERE job_id=old.id;
      END;
      CREATE TRIGGER IF NOT EXISTS enrollment_person_delete AFTER DELETE ON people BEGIN
        DELETE FROM voice_enrollments WHERE person_id=old.id;
      END;") .map_err(|e| e.to_string())?;
    migrate_samples(db)
}

fn bucket(db: &Connection, segment: Uuid) -> Result<String, String> {
    let parts: (String, String, String) = db
        .query_row(
            "SELECT t.target_kind,t.target_id,t.target_version
      FROM precise_segments p JOIN cloud_tasks t ON t.id=p.task_id WHERE p.id=?1",
            [segment.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&parts).map_err(|e| e.to_string())
}

fn evidence(db: &Connection, segment: Uuid, person: Uuid) -> Result<EnrollmentPart, String> {
    let (source, start, end) =
        visible_segment(db, segment)?.ok_or("speaker is no longer current")?;
    let binding: String = db
        .query_row(
            "SELECT id FROM speaker_bindings WHERE current_segment_id=?1
      AND person_id=?2 AND origin='manual' AND state='active'",
            params![segment.to_string(), person.to_string()],
            |r| r.get(0),
        )
        .map_err(|_| "speaker is not manually confirmed".to_string())?;
    // Check all requests in this target/version, not just one provider request.
    let overlap: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM precise_segments p
      JOIN cloud_tasks t ON t.id=p.task_id JOIN precise_segments seed ON seed.id=?1
      JOIN cloud_tasks st ON st.id=seed.task_id
      WHERE t.target_kind=st.target_kind AND t.target_id=st.target_id AND t.target_version=st.target_version
      AND t.state='complete' AND p.source=?2 AND p.id!=seed.id
      AND p.started_ms<?4 AND p.ended_ms>?3)", params![segment.to_string(),source,start,end], |r|r.get(0)).map_err(|e|e.to_string())?;
    if overlap {
        return Err("overlapping_speakers".into());
    }
    Ok(EnrollmentPart {
        segment_id: segment,
        binding_id: binding,
        start,
        end,
    })
}

fn current_group(db: &Connection, segment: Uuid) -> Result<Option<String>, String> {
    db.query_row(
        "SELECT group_id FROM precise_group_membership WHERE segment_id=?1",
        [segment.to_string()],
        |r| r.get::<_, Option<String>>(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(|e| e.to_string())
}

fn evidence_current(db: &Connection, work: &EnrollmentWork) -> Result<bool, String> {
    let mut shared_group = None;
    for part in &work.parts {
        let Ok(current) = evidence(db, part.segment_id, work.person) else {
            return Ok(false);
        };
        if work.parts.len() > 1 {
            let Some(group) = current_group(db, part.segment_id)? else {
                return Ok(false);
            };
            if shared_group
                .as_ref()
                .is_some_and(|expected| expected != &group)
            {
                return Ok(false);
            }
            shared_group = Some(group);
        }
        if current.binding_id != part.binding_id
            || current.start != part.start
            || current.end != part.end
            || bucket(db, part.segment_id)? != work.bucket
            || visible_segment(db, part.segment_id)?.map(|s| s.0) != Some(work.source.clone())
        {
            return Ok(false);
        }
    }
    for pair in work.parts.windows(2) {
        if intervening_segment(db, &pair[0], &pair[1])? {
            return Ok(false);
        }
    }
    Ok(!work.parts.is_empty())
}

pub fn add_single(
    tx: &Transaction<'_>,
    segment: Uuid,
    person: Uuid,
    now: i64,
) -> Result<bool, String> {
    let Ok(part) = evidence(tx, segment, person) else {
        return Ok(false);
    };
    let row: Option<(String, Vec<u8>, i64, i64)> = tx
        .query_row(
            "SELECT o.source,o.vector,q.voiced_ms,q.score
      FROM voice_observations o JOIN voice_observation_quality q ON q.segment_id=o.segment_id
      WHERE o.segment_id=?1 AND o.state='ready' AND o.model_version=?2",
            params![segment.to_string(), VOICE_MODEL_VERSION],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((source, vector, voiced_ms, score)) = row else {
        return Ok(false);
    };
    if voiced_ms < MIN_VOICED_MS {
        return Ok(false);
    }
    let work = EnrollmentWork {
        id: Uuid::new_v4(),
        person,
        source,
        bucket: bucket(tx, segment)?,
        parts: vec![part],
    };
    queue_candidate(tx, &work, Some((&vector, score)), now)
}

struct Sample {
    id: String,
    source: String,
    start: i64,
    end: i64,
    bucket: String,
    score: i64,
    vector: Vec<f32>,
}
fn samples(db: &Connection, person: Uuid, model: &str) -> Result<Vec<Sample>, String> {
    let mut stmt=db.prepare("SELECT s.id,s.source,s.started_ms,s.ended_ms,q.bucket,q.score,s.vector
      FROM voice_samples s JOIN voice_sample_quality q ON q.sample_id=s.id
      WHERE s.person_id=?1 AND s.model_version=?2 AND s.revoked_ms IS NULL ORDER BY q.score DESC,s.created_ms,s.id").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![person.to_string(), model], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Vec<u8>>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(id, source, start, end, bucket, score, vector)| {
            Ok(Sample {
                id,
                source,
                start,
                end,
                bucket,
                score,
                vector: decode_vector(&vector)?,
            })
        })
        .collect()
}
fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn admit(
    tx: &Transaction<'_>,
    work: &EnrollmentWork,
    bytes: &[u8],
    score: i64,
    now: i64,
) -> Result<bool, String> {
    if !evidence_current(tx, work)? {
        return Ok(false);
    }
    let vector = decode_vector(bytes)?;
    // Do not silently restore a removed or evicted sample from the same human confirmation.
    for part in &work.parts {
        let seen: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM voice_sample_parts p
          JOIN voice_samples s ON s.id=p.sample_id WHERE p.binding_id=?1 AND s.person_id=?2)",
                params![part.binding_id, work.person.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if seen {
            return Ok(false);
        }
    }
    let existing = samples(tx, work.person, VOICE_MODEL_VERSION)?;
    let own = existing
        .iter()
        .map(|s| similarity(&s.vector, &vector))
        .fold(-1.0_f32, f32::max);
    if !existing.is_empty() && own < PERSON_MATCH_MIN {
        return Ok(false);
    }
    let start = work.parts.first().ok_or("empty enrollment")?.start;
    let end = work.parts.last().unwrap().end;
    let mut remove = std::collections::HashSet::new();
    for sample in &existing {
        let duplicate = similarity(&sample.vector, &vector) >= DUPLICATE_SIMILARITY
            || (sample.source == work.source && sample.start < end && sample.end > start);
        if duplicate {
            if sample.score >= score {
                return Ok(false);
            }
            remove.insert(sample.id.clone());
        }
    }
    let scene: Vec<_> = existing
        .iter()
        .filter(|s| s.source == work.source && s.bucket == work.bucket && !remove.contains(&s.id))
        .collect();
    if scene.len() >= PER_SCENE {
        let worst = scene.last().unwrap();
        if worst.score >= score {
            return Ok(false);
        }
        remove.insert(worst.id.clone());
    }
    let remaining: Vec<_> = existing
        .iter()
        .filter(|s| !remove.contains(&s.id))
        .collect();
    if remaining.len() >= PER_PERSON {
        let worst = remaining.last().unwrap();
        if worst.score >= score {
            return Ok(false);
        }
        remove.insert(worst.id.clone());
    }
    for id in remove {
        tx.execute(
            "UPDATE voice_samples SET revoked_ms=?2 WHERE id=?1",
            params![id, now],
        )
        .map_err(|e| e.to_string())?;
    }
    let id = work.id.to_string();
    tx.execute(
        "INSERT INTO voice_samples VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",
        params![
            id,
            work.person.to_string(),
            work.parts[0].segment_id.to_string(),
            work.source,
            start,
            end,
            bytes,
            VOICE_MODEL_VERSION,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO voice_sample_quality VALUES(?1,?2,?3)",
        params![id, work.bucket, score],
    )
    .map_err(|e| e.to_string())?;
    for part in &work.parts {
        tx.execute(
            "INSERT INTO voice_sample_parts VALUES(?1,?2,?3)",
            params![id, part.segment_id.to_string(), part.binding_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "UPDATE identity_state SET generation=generation+1 WHERE id=1",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Only the just-confirmed scope participates. A single-sentence action cannot
/// borrow neighboring speech just because the cloud reused the same label.
pub fn queue_short_samples(
    tx: &Transaction<'_>,
    ids: &[Uuid],
    person: Uuid,
    now: i64,
) -> Result<(), String> {
    let mut candidates = Vec::new();
    for id in ids {
        // Old vectors remain matchable, but new enrollment needs measured PCM quality.
        tx.execute(
            "UPDATE voice_observations SET state='pending',available_ms=0,attempts=0,updated_ms=?2
          WHERE segment_id=?1 AND state='ready' AND NOT EXISTS
          (SELECT 1 FROM voice_observation_quality q WHERE q.segment_id=?1)",
            params![id.to_string(), now],
        )
        .map_err(|e| e.to_string())?;
        let Ok(part) = evidence(tx, *id, person) else {
            continue;
        };
        if !(400..3_000).contains(&(part.end - part.start)) {
            continue;
        }
        let source = visible_segment(tx, *id)?.unwrap().0;
        let Some(group) = current_group(tx, *id)? else {
            continue;
        };
        candidates.push((source, bucket(tx, *id)?, group, part));
    }
    candidates.sort_by(|a, b| {
        (&a.0, &a.1, a.3.start, a.3.segment_id).cmp(&(&b.0, &b.1, b.3.start, b.3.segment_id))
    });
    let mut chunks: Vec<EnrollmentWork> = Vec::new();
    let mut current: Option<(String, EnrollmentWork)> = None;
    for (source, bucket, group, part) in candidates {
        let follows = if let Some((old_group, work)) = &current {
            let last = work.parts.last().unwrap();
            let sum: i64 = work.parts.iter().map(|p| p.end - p.start).sum();
            let gap = part.start - last.end;
            old_group == &group
                && work.source == source
                && work.bucket == bucket
                && (0..=MAX_GAP_MS).contains(&gap)
                && sum + part.end - part.start <= MAX_COMBINED_MS
                && work.parts.len() < 12
                && !intervening_segment(tx, last, &part)?
        } else {
            false
        };
        if !follows {
            if let Some((_, work)) = current.take() {
                chunks.push(work);
            }
            current = Some((
                group,
                EnrollmentWork {
                    id: Uuid::nil(),
                    person,
                    source,
                    bucket,
                    parts: Vec::new(),
                },
            ));
        }
        let work = &mut current.as_mut().unwrap().1;
        work.parts.push(part);
        if work.parts.iter().map(|p| p.end - p.start).sum::<i64>() >= TARGET_MS {
            chunks.push(current.take().unwrap().1);
        }
    }
    if let Some((_, work)) = current {
        chunks.push(work);
    }
    chunks.retain(|w| {
        w.parts.len() > 1 && w.parts.iter().map(|p| p.end - p.start).sum::<i64>() >= MIN_VOICED_MS
    });
    chunks.sort_by_key(|w| std::cmp::Reverse(w.parts.iter().map(|p| p.end - p.start).sum::<i64>()));
    // Bound inference work as well as the resulting active sample library.
    let mut counts = std::collections::HashMap::new();
    for work in chunks {
        let key = (work.source.clone(), work.bucket.clone());
        let count = counts.entry(key).or_insert(0);
        if *count >= 12 {
            continue;
        }
        *count += 1;
        queue_candidate(tx, &work, None, now)?;
    }
    Ok(())
}
fn queue_candidate(
    tx: &Transaction<'_>,
    work: &EnrollmentWork,
    ready: Option<(&[u8], i64)>,
    now: i64,
) -> Result<bool, String> {
    let mut hash = Sha256::new();
    hash.update(work.person.as_bytes());
    hash.update(serde_json::to_vec(&work.parts).map_err(|e| e.to_string())?);
    let digest = hash.finalize();
    let id = Uuid::from_slice(&digest[..16])
        .map_err(|e| e.to_string())?
        .to_string();
    let inserted = tx
        .execute(
            "INSERT OR IGNORE INTO voice_enrollments
      (id,person_id,source,bucket,started_ms,ended_ms,state,updated_ms,vector,score)
      VALUES(?1,?2,?3,?4,?5,?6,'pending',?7,?8,?9)",
            params![
                id,
                work.person.to_string(),
                work.source,
                work.bucket,
                work.parts[0].start,
                work.parts.last().unwrap().end,
                now,
                ready.map(|r| r.0),
                ready.map(|r| r.1)
            ],
        )
        .map_err(|e| e.to_string())?;
    if inserted != 0 {
        for part in &work.parts {
            tx.execute(
                "INSERT INTO voice_enrollment_parts VALUES(?1,?2,?3,?4,?5)",
                params![
                    id,
                    part.segment_id.to_string(),
                    part.binding_id,
                    part.start,
                    part.end
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(inserted != 0)
}

fn read_work(
    db: &Connection,
    id: &str,
    person: &str,
    source: String,
    bucket: String,
) -> Result<EnrollmentWork, String> {
    let mut stmt=db.prepare("SELECT segment_id,binding_id,started_ms,ended_ms FROM voice_enrollment_parts WHERE job_id=?1 ORDER BY started_ms,segment_id").map_err(|e|e.to_string())?;
    let parts = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .map(|r| {
            let (segment, binding_id, start, end) = r.map_err(|e| e.to_string())?;
            Ok(EnrollmentPart {
                segment_id: Uuid::parse_str(&segment).map_err(|e| e.to_string())?,
                binding_id,
                start,
                end,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(EnrollmentWork {
        id: Uuid::parse_str(id).map_err(|e| e.to_string())?,
        person: Uuid::parse_str(person).map_err(|e| e.to_string())?,
        source,
        bucket,
        parts,
    })
}

pub struct EnrollmentInput {
    pub matching: MatchInput,
    pub person: String,
    work: EnrollmentWork,
    score: i64,
}
/// Small snapshot only. Global scoring reuses VoiceMatcher outside the Store lock.
pub fn prepare_enrollment(db: &Connection) -> Result<Option<EnrollmentInput>, String> {
    let row: Option<(String, String, String, String, Vec<u8>, i64)> = db
        .query_row(
            "SELECT id,person_id,source,bucket,vector,score FROM voice_enrollments
      WHERE state='pending' AND vector IS NOT NULL ORDER BY updated_ms,score DESC,id LIMIT 1",
            [],
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
        .map_err(|e| e.to_string())?;
    let Some((id, person, source, bucket, bytes, score)) = row else {
        return Ok(None);
    };
    let generation = db
        .query_row(
            "SELECT generation FROM identity_state WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let work = read_work(db, &id, &person, source, bucket)?;
    Ok(Some(EnrollmentInput {
        work,
        person,
        score,
        matching: MatchInput {
            generation,
            id,
            bytes,
            model: VOICE_MODEL_VERSION.into(),
            denied: Default::default(),
        },
    }))
}

pub fn publish_enrollment(
    db: &mut Connection,
    input: &EnrollmentInput,
    allowed: bool,
    now: i64,
) -> Result<bool, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM voice_enrollments WHERE id=?1 AND state='pending' AND vector=?2 AND score=?3)
      AND (SELECT generation FROM identity_state WHERE id=1)=?4",params![input.matching.id,input.matching.bytes,input.score,input.matching.generation],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !current {
        return Ok(false);
    }
    let valid = evidence_current(&tx, &input.work)?;
    let added =
        valid && allowed && admit(&tx, &input.work, &input.matching.bytes, input.score, now)?;
    tx.execute(
        "UPDATE voice_enrollments SET state=?2,error_code=?3,updated_ms=?4,vector=NULL WHERE id=?1",
        params![
            input.matching.id,
            if valid { "complete" } else { "stale" },
            if added {
                None
            } else {
                Some(if valid {
                    "sample_not_selected"
                } else {
                    "identity_changed"
                })
            },
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
pub(super) fn drain_enrollment(db: &mut Connection, now: i64) -> Result<(), String> {
    let mut matcher = VoiceMatcher::default();
    while let Some(input) = prepare_enrollment(db)? {
        let allowed = matcher.allows_enrollment(&input.matching, &input.person, |after| {
            match_sample_page(db, &input.matching, after)
        })?;
        publish_enrollment(db, &input, allowed, now)?;
    }
    Ok(())
}

fn intervening_segment(
    db: &Connection,
    a: &EnrollmentPart,
    b: &EnrollmentPart,
) -> Result<bool, String> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM precise_segments p
      JOIN cloud_tasks t ON t.id=p.task_id JOIN precise_segments seed ON seed.id=?1
      JOIN cloud_tasks st ON st.id=seed.task_id
      WHERE t.target_kind=st.target_kind AND t.target_id=st.target_id AND t.target_version=st.target_version
      AND t.state='complete' AND p.source=seed.source AND p.id NOT IN(?1,?2)
      AND p.started_ms<?4 AND p.ended_ms>?3)",params![a.segment_id.to_string(),b.segment_id.to_string(),a.end,b.start],|r|r.get(0)).map_err(|e|e.to_string())
}

pub fn take(db: &mut Connection, now: i64) -> Result<Option<EnrollmentWork>, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let row: Option<(String, String, String, String)> = tx
        .query_row(
            "SELECT id,person_id,source,bucket FROM voice_enrollments
      WHERE state='pending' AND vector IS NULL AND available_ms<=?1 ORDER BY updated_ms,
      (SELECT sum(ended_ms-started_ms) FROM voice_enrollment_parts WHERE job_id=voice_enrollments.id) DESC,id LIMIT 1",
            [now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((id, person, source, bucket)) = row else {
        return Ok(None);
    };
    let work = read_work(&tx, &id, &person, source, bucket)?;
    let valid = evidence_current(&tx, &work)?;
    tx.execute(
        "UPDATE voice_enrollments SET state=?2,updated_ms=?3 WHERE id=?1",
        params![id, if valid { "processing" } else { "stale" }, now],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(valid.then_some(work))
}
pub fn publish(
    db: &mut Connection,
    work: &EnrollmentWork,
    bytes: &[u8],
    quality: Option<VoiceQuality>,
    now: i64,
) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let active: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM voice_enrollments WHERE id=?1 AND state='processing')",
            [work.id.to_string()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !active {
        return Ok(());
    }
    let valid = evidence_current(&tx, work)?;
    let eligible = quality.filter(|q| q.voiced_ms >= MIN_VOICED_MS);
    tx.execute("UPDATE voice_enrollments SET state=?2,error_code=?3,updated_ms=?4,vector=?5,score=?6 WHERE id=?1",
      params![work.id.to_string(),if !valid{"stale"}else if eligible.is_some(){"pending"}else{"unavailable"},
      if !valid{Some("identity_changed")}else if eligible.is_none(){Some("audio_quality")}else{None},now,
      if valid && eligible.is_some(){Some(bytes)}else{None},eligible.map(|q|q.score)]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
pub fn fail(db: &Connection, id: Uuid, error: &str, now: i64) -> Result<(), String> {
    let transient = matches!(
        error,
        "worker_failed" | "worker_timeout" | "resource_deferred"
    );
    db.execute("UPDATE voice_enrollments SET state=CASE WHEN ?4 AND (attempts<3 OR ?5) THEN 'pending' ELSE 'unavailable' END,
      attempts=attempts+CASE WHEN ?5 THEN 0 ELSE 1 END,available_ms=?3+min(300000,5000*(1<<min(attempts,6))),error_code=?2,updated_ms=?3
      WHERE id=?1 AND state='processing'",params![id.to_string(),error,now,transient,error=="resource_deferred"]).map_err(|e|e.to_string())?;
    Ok(())
}
fn migrate_samples(db: &Connection) -> Result<(), String> {
    let done: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM core_migrations WHERE name='voice_enrollment_v1')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if done {
        return Ok(());
    }
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let rows = {
        let mut stmt=tx.prepare("SELECT id,person_id,segment_id,model_version,ended_ms-started_ms FROM voice_samples WHERE revoked_ms IS NULL ORDER BY created_ms,id").map_err(|e|e.to_string())?;
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    let mut identities = std::collections::HashSet::new();
    for (id, person, segment, model, duration) in rows {
        let person_id = Uuid::parse_str(&person).map_err(|e| e.to_string())?;
        let segment_id = Uuid::parse_str(&segment).map_err(|e| e.to_string())?;
        if let Ok(part) = evidence(&tx, segment_id, person_id) {
            tx.execute(
                "INSERT OR IGNORE INTO voice_sample_parts VALUES(?1,?2,?3)",
                params![id, segment, part.binding_id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT OR IGNORE INTO voice_sample_quality VALUES(?1,?2,?3)",
                params![id, bucket(&tx, segment_id)?, duration.clamp(0, 10_000)],
            )
            .map_err(|e| e.to_string())?;
            identities.insert((person_id, model));
        } else {
            tx.execute(
                "UPDATE voice_samples SET revoked_ms=created_ms WHERE id=?1",
                [id],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    for (person, model) in identities {
        let all = samples(&tx, person, &model)?;
        let mut kept: Vec<&Sample> = Vec::new();
        for sample in &all {
            let duplicate = kept.iter().any(|s| {
                similarity(&s.vector, &sample.vector) >= DUPLICATE_SIMILARITY
                    || (s.source == sample.source && s.start < sample.end && s.end > sample.start)
            });
            let scene = kept
                .iter()
                .filter(|s| s.source == sample.source && s.bucket == sample.bucket)
                .count();
            if duplicate || scene >= PER_SCENE || kept.len() >= PER_PERSON {
                tx.execute(
                    "UPDATE voice_samples SET revoked_ms=created_ms WHERE id=?1",
                    [&sample.id],
                )
                .map_err(|e| e.to_string())?;
            } else {
                kept.push(sample);
            }
        }
    }
    tx.execute(
        "UPDATE identity_state SET generation=generation+1 WHERE id=1",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO core_migrations VALUES('voice_enrollment_v1')",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
