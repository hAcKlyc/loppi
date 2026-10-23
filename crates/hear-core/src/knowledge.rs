mod enrollment;
pub use enrollment::{VoiceQuality, enrollment_quality, prepare_enrollment, publish_enrollment};

use hear_protocol::{AudioSource, PersonView, ReviewItem, SearchHit, VoiceSampleView};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS text_corrections (
            id TEXT PRIMARY KEY,
            original_segment_id TEXT NOT NULL,
            anchor_segment_id TEXT,
            current_segment_id TEXT,
            source TEXT NOT NULL CHECK(source IN ('microphone','system')),
            started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL,
            text TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('active','pending_review','reverted')),
            revision INTEGER NOT NULL,
            created_ms INTEGER NOT NULL,
            updated_ms INTEGER NOT NULL,
            CHECK(ended_ms>started_ms)
         );
         CREATE INDEX IF NOT EXISTS text_corrections_segment
           ON text_corrections(current_segment_id,state);
         CREATE INDEX IF NOT EXISTS text_corrections_range
           ON text_corrections(source,started_ms,ended_ms);
         CREATE TABLE IF NOT EXISTS people (
           id TEXT PRIMARY KEY,
           display_name TEXT NOT NULL,
           created_ms INTEGER NOT NULL,
           updated_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS voice_observations (
           segment_id TEXT PRIMARY KEY,
           speaker_key TEXT NOT NULL,
           source TEXT NOT NULL,
           started_ms INTEGER NOT NULL,
           ended_ms INTEGER NOT NULL,
           state TEXT NOT NULL CHECK(state IN ('pending','processing','ready','unavailable','stale')),
           vector BLOB,
           model_version TEXT,
           error_code TEXT,
           matched_generation INTEGER NOT NULL DEFAULT 0,
           updated_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS voice_observations_pending
           ON voice_observations(state,updated_ms);
         CREATE TABLE IF NOT EXISTS speaker_groups (
           speaker_scope TEXT NOT NULL,
           speaker_label TEXT NOT NULL,
           group_id TEXT NOT NULL,
           PRIMARY KEY(speaker_scope,speaker_label)
         );
         CREATE INDEX IF NOT EXISTS speaker_groups_group ON speaker_groups(group_id);
         CREATE TABLE IF NOT EXISTS local_speaker_assignments (
           segment_id TEXT PRIMARY KEY REFERENCES precise_segments(id) ON DELETE CASCADE,
           group_id TEXT NOT NULL,
           updated_ms INTEGER NOT NULL
         );
         CREATE TRIGGER IF NOT EXISTS precise_delete_local_speaker AFTER DELETE ON precise_segments BEGIN
           DELETE FROM local_speaker_assignments WHERE segment_id=old.id;
         END;
         CREATE INDEX IF NOT EXISTS local_speaker_assignments_group
           ON local_speaker_assignments(group_id);
         CREATE TABLE IF NOT EXISTS voice_samples (
           id TEXT PRIMARY KEY,
           person_id TEXT NOT NULL REFERENCES people(id),
           segment_id TEXT NOT NULL,
           source TEXT NOT NULL,
           started_ms INTEGER NOT NULL,
           ended_ms INTEGER NOT NULL,
           vector BLOB NOT NULL,
           model_version TEXT NOT NULL,
           created_ms INTEGER NOT NULL,
           revoked_ms INTEGER
         );
         CREATE INDEX IF NOT EXISTS voice_samples_person ON voice_samples(person_id,revoked_ms);
         CREATE TABLE IF NOT EXISTS speaker_bindings (
           id TEXT PRIMARY KEY,
           original_segment_id TEXT NOT NULL,
           current_segment_id TEXT,
           person_id TEXT NOT NULL REFERENCES people(id),
           source TEXT NOT NULL,
           started_ms INTEGER NOT NULL,
           ended_ms INTEGER NOT NULL,
           origin TEXT NOT NULL CHECK(origin IN ('manual','automatic')),
           apply_scope TEXT NOT NULL DEFAULT 'segment' CHECK(apply_scope IN ('segment','group')),
           state TEXT NOT NULL CHECK(state IN ('active','pending_review','revoked')),
           created_ms INTEGER NOT NULL,
           updated_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS speaker_bindings_current
           ON speaker_bindings(current_segment_id,state);
         CREATE UNIQUE INDEX IF NOT EXISTS speaker_bindings_one_active
           ON speaker_bindings(current_segment_id) WHERE state='active';
         DROP VIEW IF EXISTS precise_identity_projection;
         DROP VIEW IF EXISTS precise_speaker_projection;
         DROP VIEW IF EXISTS precise_group_membership;
         CREATE VIEW precise_group_membership AS
           SELECT segment_id,group_id FROM local_speaker_assignments
           UNION ALL
           SELECT p.id AS segment_id,g.group_id FROM precise_segments p
           LEFT JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope
             AND g.speaker_label=p.speaker_label
           WHERE NOT EXISTS(SELECT 1 FROM local_speaker_assignments a WHERE a.segment_id=p.id);
         CREATE VIEW precise_speaker_projection AS
           SELECT p.id AS segment_id,
             CASE WHEN a.group_id IS NOT NULL THEN a.group_id
             WHEN g.group_id IS NOT NULL AND (
               (SELECT sum(x.ended_ms-x.started_ms) FROM precise_segments x
                WHERE x.speaker_scope=p.speaker_scope AND x.speaker_label=p.speaker_label)>=15000
               OR (SELECT sum(x.ended_ms-x.started_ms) FROM precise_segments x
                   WHERE x.speaker_scope=p.speaker_scope AND x.speaker_label=p.speaker_label)*20
                  >=(SELECT sum(x.ended_ms-x.started_ms) FROM precise_segments x
                     WHERE x.speaker_scope=p.speaker_scope)
             ) THEN g.group_id ELSE NULL END AS speaker
           FROM precise_segments p
           LEFT JOIN local_speaker_assignments a ON a.segment_id=p.id
           LEFT JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope
             AND g.speaker_label=p.speaker_label;
         CREATE VIEW precise_identity_projection AS
           SELECT projection.segment_id,projection.speaker,
             CASE WHEN b.origin='automatic' AND projection.speaker IS NULL
                  THEN NULL ELSE b.person_id END AS person_id,
             CASE WHEN b.origin='automatic' AND projection.speaker IS NULL
                  THEN NULL ELSE person.display_name END AS person_name,
             CASE WHEN b.origin='automatic' AND projection.speaker IS NULL
                  THEN NULL ELSE b.origin END AS person_origin
           FROM precise_speaker_projection projection
           LEFT JOIN speaker_bindings b ON b.current_segment_id=projection.segment_id
              AND b.state='active'
           LEFT JOIN people person ON person.id=b.person_id;
         CREATE TABLE IF NOT EXISTS denied_matches (
           segment_id TEXT NOT NULL,
           person_id TEXT NOT NULL,
           created_ms INTEGER NOT NULL,
           PRIMARY KEY(segment_id,person_id)
         );
         CREATE TABLE IF NOT EXISTS identity_state (
           id INTEGER PRIMARY KEY CHECK(id=1), generation INTEGER NOT NULL
         );
         INSERT OR IGNORE INTO identity_state(id,generation) VALUES (1,1);
         CREATE TABLE IF NOT EXISTS search_records (
           rowid INTEGER PRIMARY KEY,
           kind TEXT NOT NULL CHECK(kind IN ('coarse','precise','note','ocr')),
           item_id TEXT NOT NULL,
           source TEXT,
           started_ms INTEGER NOT NULL,
           ended_ms INTEGER NOT NULL,
           text TEXT NOT NULL,
           UNIQUE(kind,item_id)
         );
         CREATE INDEX IF NOT EXISTS search_records_time ON search_records(started_ms,ended_ms);
         CREATE INDEX IF NOT EXISTS search_records_ended ON search_records(ended_ms,started_ms);
         CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5(text,
           content='search_records',content_rowid='rowid',tokenize='trigram');
         CREATE TRIGGER IF NOT EXISTS search_records_insert AFTER INSERT ON search_records BEGIN
           INSERT INTO search_fts(rowid,text) VALUES (new.rowid,new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS search_records_delete AFTER DELETE ON search_records BEGIN
           INSERT INTO search_fts(search_fts,rowid,text) VALUES('delete',old.rowid,old.text);
         END;
         CREATE TRIGGER IF NOT EXISTS search_records_update AFTER UPDATE OF text ON search_records BEGIN
           INSERT INTO search_fts(search_fts,rowid,text) VALUES('delete',old.rowid,old.text);
           INSERT INTO search_fts(rowid,text) VALUES(new.rowid,new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS coarse_search_insert AFTER INSERT ON coarse_segments BEGIN
           INSERT OR IGNORE INTO search_records(kind,item_id,source,started_ms,ended_ms,text)
             VALUES('coarse',new.id,new.source,new.started_ms,new.ended_ms,new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS coarse_search_delete AFTER DELETE ON coarse_segments BEGIN
           DELETE FROM search_records WHERE kind='coarse' AND item_id=old.id;
         END;
         CREATE TRIGGER IF NOT EXISTS precise_search_insert AFTER INSERT ON precise_segments BEGIN
           INSERT OR IGNORE INTO search_records(kind,item_id,source,started_ms,ended_ms,text)
             VALUES('precise',new.id,new.source,new.started_ms,new.ended_ms,new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS precise_search_delete AFTER DELETE ON precise_segments BEGIN
           DELETE FROM search_records WHERE kind='precise' AND item_id=old.id;
         END;
         CREATE TRIGGER IF NOT EXISTS correction_search_insert AFTER INSERT ON text_corrections
           WHEN new.state='active' BEGIN
           UPDATE search_records SET text=new.text WHERE kind='precise'
             AND item_id=new.current_segment_id;
         END;
         CREATE TRIGGER IF NOT EXISTS correction_search_update AFTER UPDATE OF state,text,current_segment_id
           ON text_corrections BEGIN
           UPDATE search_records SET text=(SELECT text FROM precise_segments WHERE id=old.current_segment_id)
             WHERE kind='precise' AND item_id=old.current_segment_id;
           UPDATE search_records SET text=new.text WHERE new.state='active'
             AND kind='precise' AND item_id=new.current_segment_id;
         END;",
    )
    .map_err(|error| error.to_string())?;
    // Historical repairs run once; live publication maintains these projections.
    crate::storage::migrate(
        db,
        "search_backfill_v1",
        "
        INSERT OR IGNORE INTO search_records(kind,item_id,source,started_ms,ended_ms,text)
          SELECT 'coarse',id,source,started_ms,ended_ms,text FROM coarse_segments;
        INSERT OR IGNORE INTO search_records(kind,item_id,source,started_ms,ended_ms,text)
          SELECT 'precise',id,source,started_ms,ended_ms,text FROM precise_segments;
    ",
    )?;
    crate::storage::migrate(
        db,
        "speaker_groups_backfill_v1",
        "
        INSERT OR IGNORE INTO speaker_groups(speaker_scope,speaker_label,group_id)
          SELECT DISTINCT speaker_scope,speaker_label,speaker_scope||':'||speaker_label
          FROM precise_segments WHERE speaker_label IS NOT NULL;
    ",
    )?;
    let correction_columns = db
        .prepare("PRAGMA table_info(text_corrections)")
        .map_err(|error| error.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if !correction_columns
        .iter()
        .any(|column| column == "anchor_segment_id")
    {
        db.execute(
            "ALTER TABLE text_corrections ADD COLUMN anchor_segment_id TEXT",
            [],
        )
        .map_err(|error| error.to_string())?;
    }
    crate::storage::migrate(
        db,
        "correction_anchor_backfill_v1",
        "
        UPDATE text_corrections SET anchor_segment_id=original_segment_id
          WHERE anchor_segment_id IS NULL;
    ",
    )?;
    let columns = db
        .prepare("PRAGMA table_info(speaker_bindings)")
        .map_err(|error| error.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if !columns.iter().any(|column| column == "apply_scope") {
        db.execute(
            "ALTER TABLE speaker_bindings ADD COLUMN apply_scope TEXT NOT NULL DEFAULT 'segment'",
            [],
        )
        .map_err(|error| error.to_string())?;
    }
    let identity_columns = db
        .prepare("PRAGMA table_info(identity_state)")
        .map_err(|error| error.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if !identity_columns
        .iter()
        .any(|column| column == "speaker_policy_version")
    {
        db.execute(
            "ALTER TABLE identity_state ADD COLUMN speaker_policy_version INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|error| error.to_string())?;
    }
    // The matcher already replays ready observations after a generation
    // change. Previous automatic names spread one matching utterance to its
    // entire cloud speaker group. Cloud labels can contain different people,
    // so discard only those automatic projections before replaying each
    // utterance from its own voice evidence.
    db.execute(
        "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL
         WHERE state='active' AND origin='automatic' AND
           (SELECT speaker_policy_version FROM identity_state WHERE id=1)<3",
        [],
    )
    .map_err(|error| error.to_string())?;
    db.execute(
        "UPDATE identity_state SET generation=generation+
           CASE WHEN EXISTS(SELECT 1 FROM voice_observations WHERE state='ready') THEN 1 ELSE 0 END,
           speaker_policy_version=3
         WHERE id=1 AND speaker_policy_version<3",
        [],
    )
    .map_err(|error| error.to_string())?;
    crate::storage::migrate(db,"voice_retry_v1","ALTER TABLE voice_observations ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
      ALTER TABLE voice_observations ADD COLUMN available_ms INTEGER NOT NULL DEFAULT 0;
      CREATE INDEX voice_observations_retry ON voice_observations(available_ms,updated_ms,segment_id) WHERE state='pending';
      CREATE INDEX voice_samples_model_page ON voice_samples(model_version,id) WHERE revoked_ms IS NULL;
      CREATE INDEX voice_observations_unmatched ON voice_observations(matched_generation,updated_ms,segment_id) WHERE state='ready';")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS voice_samples_segment_revision ON voice_samples(segment_id,id) WHERE revoked_ms IS NULL;").map_err(|e|e.to_string())?;
    enrollment::init(db)?;
    Ok(())
}

fn visible_segment(
    db: &Connection,
    segment_id: Uuid,
) -> Result<Option<(String, i64, i64)>, String> {
    db.query_row(
        "SELECT p.source,p.started_ms,p.ended_ms FROM precise_segments p
         JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
         WHERE p.id=?1 AND t.state='complete' AND
           ((t.target_kind='event' AND EXISTS (
              SELECT 1 FROM named_events e WHERE e.id=t.target_id
              AND e.deleted_ms IS NULL
              AND t.target_version=printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms)))
             OR (t.target_kind='range' AND EXISTS (
              SELECT 1 FROM manual_ranges m WHERE m.id=t.target_id
              AND t.target_version=cast(m.revision AS TEXT))))",
        params![segment_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.contains('\0') {
        return Err("person name must contain 1 to 80 characters".to_owned());
    }
    Ok(name)
}

fn revision_part(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value.as_bytes());
}

#[cfg(test)]
fn legacy_segment_revision_part(
    db: &Connection,
    id: Uuid,
    hash: &mut Sha256,
) -> Result<(), String> {
    if visible_segment(db, id)?.is_none() {
        return Err("segment is no longer current".into());
    }
    let row: (String, i64, i64, String, String, String, String) = db
        .query_row(
            "SELECT p.source,p.started_ms,p.ended_ms,p.text,p.speaker_scope,
         coalesce(p.speaker_label,''),coalesce(g.group_id,'') FROM precise_segments p
         LEFT JOIN precise_group_membership g ON g.segment_id=p.id WHERE p.id=?1",
            params![id.to_string()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    revision_part(hash, &format!("{id}:{row:?}"));
    for sql in [
        "SELECT id||':'||state||':'||revision||':'||text FROM text_corrections
         WHERE current_segment_id=?1 ORDER BY id",
        "SELECT id||':'||person_id||':'||origin||':'||apply_scope||':'||state
         FROM speaker_bindings WHERE current_segment_id=?1 ORDER BY id",
        "SELECT id||':'||person_id FROM voice_samples
         WHERE (segment_id=?1 OR id IN(SELECT sample_id FROM voice_sample_parts WHERE segment_id=?1))
           AND revoked_ms IS NULL ORDER BY id",
    ] {
        let mut stmt = db.prepare(sql).map_err(|error| error.to_string())?;
        for item in stmt
            .query_map(params![id.to_string()], |r| r.get::<_, String>(0))
            .map_err(|error| error.to_string())?
        {
            revision_part(hash, &item.map_err(|error| error.to_string())?);
        }
    }
    Ok(())
}

// A page shares four indexed reads instead of five queries per segment.
// Parts retain the exact historical digest encoding for already-open clients.
struct RevisionParts {
    group: String,
    parts: Vec<String>,
}
fn revision_parts(
    db: &Connection,
    ids: &[String],
) -> Result<std::collections::HashMap<String, RevisionParts>, String> {
    let ids = serde_json::to_string(ids).map_err(|e| e.to_string())?;
    let mut output = std::collections::HashMap::new();
    let mut stmt = db.prepare("SELECT p.id,p.source,p.started_ms,p.ended_ms,p.text,p.speaker_scope,
      coalesce(p.speaker_label,''),coalesce(g.group_id,'') FROM precise_segments p
      JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms LEFT JOIN precise_group_membership g
      ON g.segment_id=p.id
      WHERE p.id IN (SELECT value FROM json_each(?1)) AND t.state='complete' AND
      ((t.target_kind='event' AND EXISTS(SELECT 1 FROM named_events e WHERE e.id=t.target_id
        AND e.deleted_ms IS NULL AND t.target_version=printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms)))
       OR (t.target_kind='range' AND EXISTS(SELECT 1 FROM manual_ranges m WHERE m.id=t.target_id
        AND t.target_version=cast(m.revision AS TEXT))))").map_err(|e|e.to_string())?;
    for row in stmt
        .query_map([&ids], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                ),
            ))
        })
        .map_err(|e| e.to_string())?
    {
        let (id, row) = row.map_err(|e| e.to_string())?;
        output.insert(
            id.clone(),
            RevisionParts {
                group: row.6.clone(),
                parts: vec![format!("{id}:{row:?}")],
            },
        );
    }
    for sql in [
        "SELECT current_segment_id,id||':'||state||':'||revision||':'||text FROM text_corrections WHERE current_segment_id IN(SELECT value FROM json_each(?1)) ORDER BY current_segment_id,id",
        "SELECT current_segment_id,id||':'||person_id||':'||origin||':'||apply_scope||':'||state FROM speaker_bindings WHERE current_segment_id IN(SELECT value FROM json_each(?1)) ORDER BY current_segment_id,id",
        "SELECT p.segment_id,s.id||':'||s.person_id FROM voice_samples s JOIN
          (SELECT id AS sample_id,segment_id FROM voice_samples WHERE segment_id IN(SELECT value FROM json_each(?1))
           UNION SELECT sample_id,segment_id FROM voice_sample_parts WHERE segment_id IN(SELECT value FROM json_each(?1))) p
          ON p.sample_id=s.id WHERE s.revoked_ms IS NULL ORDER BY p.segment_id,s.id",
    ] {
        let mut stmt = db.prepare(sql).map_err(|e| e.to_string())?;
        for row in stmt
            .query_map([&ids], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
        {
            let (id, part) = row.map_err(|e| e.to_string())?;
            if let Some(entry) = output.get_mut(&id) {
                entry.parts.push(part);
            }
        }
    }
    Ok(output)
}
fn append_revision(hash: &mut Sha256, parts: &RevisionParts) {
    for part in &parts.parts {
        revision_part(hash, part);
    }
}
fn finish_revision(hash: Sha256) -> i64 {
    let bytes = hash.finalize();
    i64::from_le_bytes(bytes[..8].try_into().unwrap()) & ((1_i64 << 53) - 1)
}
fn group_revision(db: &Connection, group: &str) -> Result<i64, String> {
    let mut hash = Sha256::new();
    revision_part(&mut hash, "speaker_group");
    revision_part(&mut hash, group);
    let mut after = String::new();
    loop {
        let ids=db.prepare("SELECT p.id FROM precise_segments p JOIN precise_group_membership g ON g.segment_id=p.id WHERE g.group_id=?1 AND p.id>?2 ORDER BY p.id LIMIT 256").map_err(|e|e.to_string())?
          .query_map(params![group,after],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        if ids.is_empty() {
            break;
        }
        let parts = revision_parts(db, &ids)?;
        for id in &ids {
            if let Some(part) = parts.get(id) {
                append_revision(&mut hash, part);
            }
        }
        after = ids.last().unwrap().clone();
    }
    Ok(finish_revision(hash))
}
pub fn fill_revisions(
    db: &Connection,
    segments: &mut [hear_protocol::PreciseSegment],
) -> Result<(), String> {
    let mut groups = std::collections::HashMap::new();
    for page in segments.chunks_mut(256) {
        let parts = revision_parts(
            db,
            &page.iter().map(|s| s.id.to_string()).collect::<Vec<_>>(),
        )?;
        for segment in page {
            let part = parts
                .get(&segment.id.to_string())
                .ok_or("segment is no longer current")?;
            let mut hash = Sha256::new();
            revision_part(&mut hash, "segment");
            append_revision(&mut hash, part);
            segment.revision = finish_revision(hash);
            if !part.group.is_empty() {
                let revision = if let Some(revision) = groups.get(&part.group) {
                    *revision
                } else {
                    let revision = group_revision(db, &part.group)?;
                    groups.insert(part.group.clone(), revision);
                    revision
                };
                segment.speaker_revision = revision;
            }
        }
    }
    Ok(())
}
/// Content revision is a digest of the current object projection. It changes
/// for manual and background edits without a parallel revision owner/table.
pub fn object_revision(db: &Connection, kind: &str, id: Uuid) -> Result<i64, String> {
    let mut hash = Sha256::new();
    revision_part(&mut hash, kind);
    match kind {
        "segment" => {
            let parts = revision_parts(db, &[id.to_string()])?;
            append_revision(
                &mut hash,
                parts
                    .get(&id.to_string())
                    .ok_or("segment is no longer current")?,
            );
        }
        "speaker_group" => {
            let group: String = db
                .query_row(
                    "SELECT group_id FROM precise_group_membership WHERE segment_id=?1",
                    params![id.to_string()],
                    |r| r.get(0),
                )
                .map_err(|error| error.to_string())?;
            return group_revision(db, &group);
        }

        "person" => {
            let row: (String, i64) = db
                .query_row(
                    "SELECT display_name,created_ms FROM people WHERE id=?1",
                    params![id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| "person not found".to_owned())?;
            revision_part(&mut hash, &format!("{id}:{row:?}"));
            let mut stmt = db
                .prepare(
                    "SELECT id,segment_id FROM voice_samples WHERE person_id=?1
                 AND revoked_ms IS NULL ORDER BY id",
                )
                .map_err(|error| error.to_string())?;
            for sample in stmt
                .query_map(params![id.to_string()], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?
            {
                revision_part(
                    &mut hash,
                    &format!("{:?}", sample.map_err(|error| error.to_string())?),
                );
            }
            let mut stmt = db
                .prepare(
                    "SELECT id,original_segment_id,coalesce(current_segment_id,''),state
                 FROM speaker_bindings WHERE person_id=?1
                   AND state IN ('active','pending_review') ORDER BY id",
                )
                .map_err(|error| error.to_string())?;
            for binding in stmt
                .query_map(params![id.to_string()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(|error| error.to_string())?
            {
                revision_part(
                    &mut hash,
                    &format!("{:?}", binding.map_err(|error| error.to_string())?),
                );
            }
        }
        "sample" => {
            let row: (String, String, String, i64) = db
                .query_row(
                    "SELECT person_id,segment_id,model_version,created_ms FROM voice_samples
                 WHERE id=?1 AND revoked_ms IS NULL",
                    params![id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(|_| "voice sample not found".to_owned())?;
            revision_part(&mut hash, &format!("{id}:{row:?}"));
        }
        "review" => {
            let text: Option<(String, i64, i64, String, i64, String)> = db
                .query_row(
                    "SELECT source,started_ms,ended_ms,text,revision,coalesce(anchor_segment_id,'')
                 FROM text_corrections WHERE id=?1 AND state='pending_review'",
                    params![id.to_string()],
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
            if let Some(row) = text {
                revision_part(&mut hash, &format!("text:{id}:{row:?}"));
            } else {
                let row: (String, i64, i64, String, String) = db
                    .query_row(
                        "SELECT source,started_ms,ended_ms,person_id,original_segment_id
                     FROM speaker_bindings WHERE id=?1 AND state='pending_review'",
                        params![id.to_string()],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    )
                    .map_err(|_| "review item not found".to_owned())?;
                revision_part(&mut hash, &format!("speaker:{id}:{row:?}"));
            }
        }
        _ => return Err("unknown revision subject".into()),
    }
    Ok(finish_revision(hash))
}

pub fn expect_revision(db: &Connection, kind: &str, id: Uuid, expected: i64) -> Result<(), String> {
    if object_revision(db, kind, id)? != expected {
        return Err("revision_conflict: object changed; reload before editing".into());
    }
    Ok(())
}

pub fn people(db: &Connection) -> Result<Vec<PersonView>, String> {
    let mut stmt = db
        .prepare(
            "SELECT p.id,p.display_name,p.created_ms,count(s.id)
         FROM people p LEFT JOIN voice_samples s ON s.person_id=p.id AND s.revoked_ms IS NULL
         GROUP BY p.id ORDER BY lower(p.display_name),p.created_ms,p.id",
        )
        .map_err(|error| error.to_string())?;
    let mut people = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(PersonView {
                id: Uuid::parse_str(&id).map_err(|_| rusqlite::Error::InvalidQuery)?,
                display_name: row.get(1)?,
                created_ms: row.get(2)?,
                sample_count: row.get::<_, i64>(3)? as u32,
                revision: 0,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for person in &mut people {
        person.revision = object_revision(db, "person", person.id)?;
    }
    Ok(people)
}

pub fn voice_samples(db: &Connection, person_id: Uuid) -> Result<Vec<VoiceSampleView>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id,source,started_ms,ended_ms,model_version,created_ms
      FROM voice_samples WHERE person_id=?1 AND revoked_ms IS NULL ORDER BY created_ms,id",
        )
        .map_err(|error| error.to_string())?;
    let mut samples = stmt
        .query_map(params![person_id.to_string()], |r| {
            let source: String = r.get(1)?;
            Ok(VoiceSampleView {
                id: Uuid::parse_str(&r.get::<_, String>(0)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                person_id,
                source: if source == "microphone" {
                    AudioSource::Microphone
                } else {
                    AudioSource::System
                },
                started_ms: r.get(2)?,
                ended_ms: r.get(3)?,
                model_version: r.get(4)?,
                created_ms: r.get(5)?,
                revision: 0,
                parts: Vec::new(),
                audio_available: None,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for sample in &mut samples {
        sample.revision = object_revision(db, "sample", sample.id)?;
        let mut parts = db
            .prepare(
                "SELECT p.started_ms,p.ended_ms FROM voice_sample_parts s
          JOIN precise_segments p ON p.id=s.segment_id WHERE s.sample_id=?1
          ORDER BY p.started_ms,p.ended_ms,p.id",
            )
            .map_err(|e| e.to_string())?;
        sample.parts = parts
            .query_map([sample.id.to_string()], |r| {
                Ok(hear_protocol::VoiceSamplePart {
                    started_ms: r.get(0)?,
                    ended_ms: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if sample.parts.is_empty() {
            sample.parts.push(hear_protocol::VoiceSamplePart {
                started_ms: sample.started_ms,
                ended_ms: sample.ended_ms,
            });
        }
        let source = match sample.source {
            AudioSource::Microphone => "microphone",
            AudioSource::System => "system",
        };
        let mut available = true;
        for part in &sample.parts {
            if !crate::events::utterance_has_retained_audio(
                db,
                source,
                part.started_ms,
                part.ended_ms,
                false,
            )? {
                available = false;
                break;
            }
        }
        sample.audio_available = Some(available);
    }
    Ok(samples)
}

pub fn review_items(db: &Connection) -> Result<Vec<ReviewItem>, String> {
    let mut stmt = db
        .prepare(
            "SELECT c.id,'text',c.original_segment_id,c.source,c.started_ms,c.ended_ms,
          c.text,NULL,NULL FROM text_corrections c WHERE c.state='pending_review'
         UNION ALL
         SELECT b.id,'speaker',b.original_segment_id,b.source,b.started_ms,b.ended_ms,
          NULL,b.person_id,p.display_name FROM speaker_bindings b
          JOIN people p ON p.id=b.person_id WHERE b.state='pending_review'
         ORDER BY 5,1",
        )
        .map_err(|error| error.to_string())?;
    let mut items = stmt
        .query_map([], |r| {
            let source: String = r.get(3)?;
            Ok(ReviewItem {
                id: Uuid::parse_str(&r.get::<_, String>(0)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                kind: r.get(1)?,
                original_segment_id: Uuid::parse_str(&r.get::<_, String>(2)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                source: if source == "microphone" {
                    AudioSource::Microphone
                } else {
                    AudioSource::System
                },
                started_ms: r.get(4)?,
                ended_ms: r.get(5)?,
                text: r.get(6)?,
                person_id: r
                    .get::<_, Option<String>>(7)?
                    .map(|value| Uuid::parse_str(&value).unwrap()),
                person_name: r.get(8)?,
                revision: 0,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for item in &mut items {
        item.revision = object_revision(db, "review", item.id)?;
    }
    Ok(items)
}

pub fn resolve_review(
    tx: &Transaction<'_>,
    item_id: Uuid,
    target: Option<Uuid>,
    now: i64,
) -> Result<(), String> {
    let id = item_id.to_string();
    let item: Option<(String, String, String, i64, i64)> = tx
        .query_row(
            "SELECT 'text',original_segment_id,source,started_ms,ended_ms FROM text_corrections
           WHERE id=?1 AND state='pending_review'
         UNION ALL SELECT 'speaker',original_segment_id,source,started_ms,ended_ms
           FROM speaker_bindings WHERE id=?1 AND state='pending_review'",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((kind, original, source, start, end)) = item else {
        return Err("review item not found".into());
    };
    let group_binding: Option<String> = if kind == "speaker" {
        tx.query_row(
            "SELECT person_id FROM speaker_bindings WHERE id=?1 AND apply_scope='group'",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
    } else {
        None
    };
    let (new_state, new_segment, anchor_start, anchor_end) = if let Some(target) = target {
        let target_id = target.to_string();
        let current = visible_segment(tx, target)?
            .ok_or_else(|| "target segment is no longer current".to_owned())?;
        if current.0 != source || current.1 >= end || current.2 <= start {
            return Err("review target has different source or time".into());
        }
        let same_target: i64 = tx
            .query_row(
                "WITH RECURSIVE descendants(id) AS (
                  SELECT old_t.target_id FROM precise_segments old_p
                    JOIN cloud_tasks old_t ON old_t.id=old_p.task_id WHERE old_p.id=?1
                  UNION SELECT l.new_event_id FROM event_lineage l
                    JOIN descendants d ON d.id=l.old_event_id)
                 SELECT EXISTS(SELECT 1 FROM precise_segments old_p JOIN cloud_tasks old_t
              ON old_t.id=old_p.task_id JOIN precise_segments new_p ON new_p.id=?2
              JOIN transcript_tasks new_t ON new_t.id=new_p.task_id AND new_p.started_ms>=new_t.started_ms AND new_p.ended_ms<=new_t.ended_ms
              WHERE old_p.id=?1 AND old_t.target_kind=new_t.target_kind
              AND (old_t.target_id=new_t.target_id OR
                (old_t.target_kind='event' AND new_t.target_id IN descendants)))",
                params![original, target_id],
                |r| r.get(0),
            )
            .map_err(|error| error.to_string())?;
        if same_target == 0 {
            return Err("review target belongs to another event or range".into());
        }
        let conflict: i64 = if kind == "text" {
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM text_corrections WHERE current_segment_id=?1
             AND state='active')",
                params![target_id],
                |r| r.get(0),
            )
        } else if group_binding.is_some() {
            Ok(0)
        } else {
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM speaker_bindings WHERE current_segment_id=?1
             AND state='active' AND origin='manual')",
                params![target_id],
                |r| r.get(0),
            )
        }
        .map_err(|error| error.to_string())?;
        if conflict != 0 {
            return Err("review target already has a manual correction".into());
        }
        if let Some(person) = &group_binding {
            let target_scope: (i64, i64) = tx
                .query_row(
                    "SELECT started_ms,ended_ms FROM named_events WHERE id=(
                   SELECT t.target_id FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
                   WHERE p.id=?1) UNION ALL
                 SELECT started_ms,ended_ms FROM manual_ranges WHERE id=(
                   SELECT t.target_id FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
                   WHERE p.id=?1) LIMIT 1",
                    params![target_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|error| error.to_string())?;
            let mut stmt = tx
                .prepare(
                    "SELECT b.id,b.started_ms,b.ended_ms FROM speaker_bindings b
                 JOIN precise_segments p ON p.id=b.original_segment_id
                 JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope
                   AND g.speaker_label=p.speaker_label
                 WHERE b.state='pending_review' AND b.person_id=?2
                   AND b.apply_scope='group' AND g.group_id=(
                     SELECT g0.group_id FROM precise_segments p0 JOIN speaker_groups g0
                       ON g0.speaker_scope=p0.speaker_scope AND g0.speaker_label=p0.speaker_label
                     WHERE p0.id=?1)
                   AND b.started_ms<?4 AND b.ended_ms>?3",
                )
                .map_err(|error| error.to_string())?;
            let selected = stmt
                .query_map(
                    params![original, person, target_scope.0, target_scope.1],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    },
                )
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            drop(stmt);
            for (old_id, a, b) in &selected {
                for (part_start, part_end) in
                    [(*a, target_scope.0.min(*b)), (target_scope.1.max(*a), *b)]
                {
                    if part_end <= part_start {
                        continue;
                    }
                    tx.execute(
                        "INSERT INTO speaker_bindings
                      (id,original_segment_id,current_segment_id,person_id,source,
                       started_ms,ended_ms,origin,apply_scope,state,created_ms,updated_ms)
                      SELECT ?2,original_segment_id,NULL,person_id,source,
                       ?3,?4,origin,apply_scope,'pending_review',created_ms,?5
                      FROM speaker_bindings WHERE id=?1",
                        params![
                            old_id,
                            Uuid::new_v4().to_string(),
                            part_start,
                            part_end,
                            now
                        ],
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
            tx.execute(
                "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,
               updated_ms=?3 WHERE state='pending_review' AND person_id=?2
               AND apply_scope='group' AND started_ms<?5 AND ended_ms>?4
               AND original_segment_id IN (
                 SELECT p.id FROM precise_segments p JOIN speaker_groups g
                   ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
                 WHERE g.group_id=(SELECT g0.group_id FROM precise_segments p0
                   JOIN speaker_groups g0 ON g0.speaker_scope=p0.speaker_scope
                     AND g0.speaker_label=p0.speaker_label
                   WHERE p0.id=?1))",
                params![original, person, now, target_scope.0, target_scope.1],
            )
            .map_err(|error| error.to_string())?;
            bind_speaker(
                tx,
                target,
                Uuid::parse_str(person).map_err(|error| error.to_string())?,
                true,
                now,
            )?;
            return Ok(());
        }
        ("active", Some(target_id), current.1, current.2)
    } else {
        (
            if kind == "text" {
                "reverted"
            } else {
                "revoked"
            },
            None,
            start,
            end,
        )
    };
    if kind == "text" {
        tx.execute(
            "UPDATE text_corrections SET state=?2,current_segment_id=?3,
          anchor_segment_id=coalesce(?3,anchor_segment_id),
          revision=revision+1,updated_ms=?4,started_ms=?5,ended_ms=?6 WHERE id=?1",
            params![id, new_state, new_segment, now, anchor_start, anchor_end],
        )
        .map_err(|error| error.to_string())?;
    } else {
        if let Some(target_id) = &new_segment {
            tx.execute(
                "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,
             updated_ms=?2 WHERE current_segment_id=?1 AND state='active' AND origin='automatic'",
                params![target_id, now],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.execute(
            "UPDATE speaker_bindings SET state=?2,current_segment_id=?3,
          updated_ms=?4,started_ms=?5,ended_ms=?6 WHERE id=?1",
            params![id, new_state, new_segment, now, anchor_start, anchor_end],
        )
        .map_err(|error| error.to_string())?;
        if let Some(target_id) = new_segment {
            rebuild_for_segment(
                tx,
                Uuid::parse_str(&target_id).map_err(|error| error.to_string())?,
                now,
            )?;
        }
    }
    Ok(())
}

pub fn search(
    db: &Connection,
    query: Option<&str>,
    start: i64,
    end: i64,
    person_id: Option<Uuid>,
    offset: u32,
    limit: u16,
) -> Result<Vec<SearchHit>, String> {
    if start < 0 || end <= start || limit == 0 || limit > 200 {
        return Err("invalid search range or limit".to_owned());
    }
    let (sql, search_text) = search_sql(db, query)?;
    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    stmt.query_map(
        params![
            start,
            end,
            person_id.map(|id| id.to_string()),
            search_text,
            i64::from(limit),
            i64::from(offset)
        ],
        |r| {
            let id: String = r.get(1)?;
            let source: Option<String> = r.get(2)?;
            let person: Option<String> = r.get(6)?;
            Ok(SearchHit {
                kind: r.get(0)?,
                item_id: Uuid::parse_str(&id).map_err(|_| rusqlite::Error::InvalidQuery)?,
                source: source.map(|s| {
                    if s == "microphone" {
                        AudioSource::Microphone
                    } else {
                        AudioSource::System
                    }
                }),
                started_ms: r.get(3)?,
                ended_ms: r.get(4)?,
                text: r.get(5)?,
                person_id: person.map(|s| Uuid::parse_str(&s).unwrap()),
                person_name: r.get(7)?,
            })
        },
    )
    .map_err(|error| error.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| error.to_string())
}

fn search_sql(db: &Connection, query: Option<&str>) -> Result<(String, String), String> {
    let query = query.map(str::trim).filter(|value| !value.is_empty());
    if query.is_some_and(|value| value.chars().count() > 256 || value.chars().any(char::is_control))
    {
        return Err("search phrase is invalid".to_owned());
    }
    let use_fts = query.is_some_and(|value| value.chars().count() >= 3);
    let search_text = if use_fts {
        format!("\"{}\"", query.unwrap().replace('"', "\"\""))
    } else {
        query.unwrap_or("").to_owned()
    };
    // Frequent phrases should stream newest time-indexed rows and stop at the
    // requested page; rare phrases should seek FTS directly. Probe only a fixed
    // number of postings, never count all historical matches. Both plans apply
    // exactly the same visibility, identity and deduplication predicates.
    let common_phrase = use_fts && db.query_row(
        "SELECT EXISTS(SELECT 1 FROM search_fts WHERE search_fts MATCH ?1 LIMIT 1 OFFSET 4095)",
        [&search_text], |r|r.get::<_,bool>(0)).map_err(|e|e.to_string())?;
    let match_clause = if query.is_none() {
        "1"
    } else if common_phrase {
        "EXISTS(SELECT 1 FROM search_fts WHERE rowid=sr.rowid AND search_fts MATCH ?4)"
    } else if use_fts {
        "sr.rowid IN (SELECT rowid FROM search_fts WHERE search_fts MATCH ?4)"
    } else {
        "instr(lower(sr.text),lower(?4))>0"
    };
    let coarse_duplicate = if query.is_some() {
        "hit.kind!='coarse' OR NOT EXISTS (SELECT 1 FROM visible refined
          WHERE refined.kind='precise' AND refined.source=hit.source
            AND refined.started_ms<hit.ended_ms AND refined.ended_ms>hit.started_ms)"
    } else {
        "1"
    };
    let sql = format!(
        "WITH visible AS (SELECT sr.rowid AS result_rowid,sr.kind,sr.item_id,sr.source,
      sr.started_ms,sr.ended_ms,sr.text,
      (SELECT identity.person_id FROM precise_identity_projection identity
        WHERE identity.segment_id=p.id) AS person_id,
      (SELECT identity.person_name FROM precise_identity_projection identity
        WHERE identity.segment_id=p.id) AS person_name,t.target_kind
      FROM search_records sr
      LEFT JOIN precise_segments p ON sr.kind='precise' AND p.id=sr.item_id
      LEFT JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
      WHERE sr.started_ms<?2 AND sr.ended_ms>?1
        AND (?3 IS NULL OR EXISTS(SELECT 1 FROM precise_identity_projection identity
          WHERE identity.segment_id=p.id AND identity.person_id=?3))
        AND {match_clause}
        AND (sr.kind!='precise' OR (t.state='complete' AND (
          (t.target_kind='event' AND EXISTS (SELECT 1 FROM named_events e
            WHERE e.id=t.target_id AND e.deleted_ms IS NULL
            AND t.target_version=printf('%d:%d:%d',e.source_mask,e.started_ms,e.ended_ms)))
          OR (t.target_kind='range' AND EXISTS (SELECT 1 FROM manual_ranges m
            WHERE m.id=t.target_id AND t.target_version=cast(m.revision AS TEXT)))))))
      SELECT hit.kind,hit.item_id,hit.source,hit.started_ms,hit.ended_ms,hit.text,
        hit.person_id,hit.person_name FROM visible hit WHERE ({coarse_duplicate})
        AND (hit.kind!='precise' OR NOT EXISTS (SELECT 1 FROM visible twin
          WHERE twin.kind='precise' AND twin.source=hit.source
            AND twin.started_ms=hit.started_ms AND twin.ended_ms=hit.ended_ms
            AND twin.text=hit.text AND twin.result_rowid!=hit.result_rowid
            AND (twin.target_kind='event' AND hit.target_kind!='event'
              OR twin.target_kind=hit.target_kind AND twin.result_rowid<hit.result_rowid)))
      ORDER BY hit.started_ms DESC,hit.result_rowid DESC LIMIT ?5 OFFSET ?6"
    );
    Ok((sql, search_text))
}
#[cfg(test)]
pub fn search_query_plan(
    db: &Connection,
    query: Option<&str>,
    start: i64,
    end: i64,
) -> Vec<String> {
    let (sql, text) = search_sql(db, query).unwrap();
    db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap()
        .query_map(params![start, end, None::<String>, text, 50, 0], |r| {
            r.get(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

pub fn search_empty_reason(
    db: &Connection,
    start: i64,
    end: i64,
    person_id: Option<Uuid>,
) -> Result<String, String> {
    if person_id.is_some() {
        let pending:i64=db.query_row(
            "SELECT EXISTS(SELECT 1 FROM voice_observations WHERE state IN ('pending','processing')
             AND started_ms<?2 AND ended_ms>?1)",params![start,end],|r|r.get(0)
        ).map_err(|error|error.to_string())?;
        if pending != 0 {
            return Ok("identity_pending".into());
        }
    }
    for (sql, reason) in [
        (
            "SELECT EXISTS(SELECT 1 FROM moments WHERE kind='screenshot' AND ocr_state='failed'
             AND anchor_ms>=?1 AND anchor_ms<?2)",
            "ocr_failed",
        ),
        (
            "SELECT EXISTS(SELECT 1 FROM cloud_tasks WHERE state='failed'
            AND started_ms<?2 AND ended_ms>?1)",
            "processing_failed",
        ),
        (
            "SELECT EXISTS(SELECT 1 FROM cloud_tasks WHERE state IN
            ('pending','processing','submitted') AND started_ms<?2 AND ended_ms>?1)",
            "processing_pending",
        ),
    ] {
        let found: i64 = db
            .query_row(sql, params![start, end], |r| r.get(0))
            .map_err(|error| error.to_string())?;
        if found != 0 {
            return Ok(reason.into());
        }
    }
    let material: i64 = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE started_ms<?2 AND ended_ms>?1
           AND retention_decision!='all_data') OR
         EXISTS(SELECT 1 FROM moments WHERE anchor_ms>=?1 AND anchor_ms<?2) OR
         EXISTS(SELECT 1 FROM search_records WHERE started_ms<?2 AND ended_ms>?1)",
            params![start, end],
            |r| r.get(0),
        )
        .map_err(|error| error.to_string())?;
    if material != 0 {
        return Ok("no_match".into());
    }
    let cleared: i64 = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM clear_ranges
       WHERE mode='all_data' AND from_ms<?2 AND to_ms>?1)",
            params![start, end],
            |r| r.get(0),
        )
        .map_err(|error| error.to_string())?;
    if cleared != 0 {
        return Ok("cleared".into());
    }
    let recorded: i64 = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM recording_sessions
       WHERE started_ms<?2 AND coalesce(ended_ms,last_confirmed_ms)>?1)",
            params![start, end],
            |r| r.get(0),
        )
        .map_err(|error| error.to_string())?;
    if recorded == 0 {
        return Ok("not_recorded".into());
    }
    Ok("no_match".into())
}

pub fn create_person(tx: &Transaction<'_>, name: &str, now: i64) -> Result<Uuid, String> {
    let name = validate_name(name)?;
    let id = Uuid::new_v4();
    tx.execute(
        "INSERT INTO people VALUES (?1,?2,?3,?3)",
        params![id.to_string(), name, now],
    )
    .map_err(|error| error.to_string())?;
    Ok(id)
}

pub fn rename_person(tx: &Transaction<'_>, id: Uuid, name: &str, now: i64) -> Result<(), String> {
    let name = validate_name(name)?;
    let updated = tx
        .execute(
            "UPDATE people SET display_name=?2,updated_ms=?3 WHERE id=?1",
            params![id.to_string(), name, now],
        )
        .map_err(|error| error.to_string())?;
    if updated == 0 {
        return Err("person not found".to_owned());
    }
    Ok(())
}

pub fn delete_person(tx: &Transaction<'_>, id: Uuid) -> Result<(), String> {
    let id = id.to_string();
    tx.execute("DELETE FROM voice_samples WHERE person_id=?1", params![id])
        .map_err(|error| error.to_string())?;
    tx.execute(
        "DELETE FROM speaker_bindings WHERE person_id=?1",
        params![id],
    )
    .map_err(|error| error.to_string())?;
    tx.execute("DELETE FROM denied_matches WHERE person_id=?1", params![id])
        .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE identity_state SET generation=generation+1 WHERE id=1",
        [],
    )
    .map_err(|error| error.to_string())?;
    let removed = tx
        .execute("DELETE FROM people WHERE id=?1", params![id])
        .map_err(|error| error.to_string())?;
    if removed == 0 {
        return Err("person not found".to_owned());
    }
    Ok(())
}

fn current_scope_segments(
    db: &Connection,
    segment_id: Uuid,
    entire: bool,
) -> Result<Vec<(Uuid, String, i64, i64)>, String> {
    if !entire {
        let (source, start, end) =
            visible_segment(db, segment_id)?.ok_or("segment is no longer current")?;
        return Ok(vec![(segment_id, source, start, end)]);
    }
    let group: String = db
        .query_row(
            "SELECT group_id FROM precise_group_membership WHERE segment_id=?1",
            [segment_id.to_string()],
            |r| r.get(0),
        )
        .map_err(|_| "speaker is unknown".to_owned())?;
    let mut stmt = db
        .prepare(
            "SELECT p.id,p.source,p.started_ms,p.ended_ms FROM precise_segments p
         JOIN precise_group_membership g ON g.segment_id=p.id
         WHERE g.group_id=?1 ORDER BY p.started_ms,p.id",
        )
        .map_err(|e| e.to_string())?;
    let candidates = stmt
        .query_map(params![group], |r| {
            let id: String = r.get(0)?;
            Ok((
                Uuid::parse_str(&id).map_err(|_| rusqlite::Error::InvalidQuery)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut selected = Vec::new();
    for candidate in candidates {
        if (!entire && candidate.0 != segment_id) || visible_segment(db, candidate.0)?.is_none() {
            continue;
        }
        selected.push(candidate);
    }
    if selected.is_empty() || !selected.iter().any(|item| item.0 == segment_id) {
        return Err("speaker is no longer current".to_owned());
    }
    Ok(selected)
}

/// Complete, chronologically numbered candidates for the current ASR target/version.
/// UI pagination must never determine the available identities or their names.
pub fn session_speakers(
    db: &Connection,
    segment_id: Uuid,
) -> Result<Vec<hear_protocol::LocalSpeaker>, String> {
    if visible_segment(db, segment_id)?.is_none() {
        return Err("segment is no longer current".into());
    }
    let mut stmt = db
        .prepare(
            "SELECT i.speaker,p.id,i.person_id FROM precise_segments p
         JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
         JOIN precise_identity_projection i ON i.segment_id=p.id
         JOIN precise_segments seed ON seed.id=?1
         JOIN transcript_tasks st ON st.id=seed.task_id AND seed.started_ms>=st.started_ms AND seed.ended_ms<=st.ended_ms
         WHERE t.target_kind=st.target_kind AND t.target_id=st.target_id
         AND t.target_version=st.target_version AND t.state='complete'
         AND i.speaker IS NOT NULL ORDER BY p.started_ms,p.id",
        )
        .map_err(|e| e.to_string())?;
    let mut result: Vec<hear_protocol::LocalSpeaker> = Vec::new();
    let mut indices = std::collections::HashMap::new();
    for row in stmt
        .query_map([segment_id.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
    {
        let (group, id, person) = row.map_err(|e| e.to_string())?;
        if let Some(&index) = indices.get(&group) {
            let speaker: &mut hear_protocol::LocalSpeaker = &mut result[index];
            speaker.anonymous &= person.is_none();
        } else {
            indices.insert(group.clone(), result.len());
            result.push(hear_protocol::LocalSpeaker {
                name: format!("说话人 {}", result.len() + 1),
                revision: group_revision(db, &group)?,
                group_id: group,
                segment_id: Uuid::parse_str(&id).map_err(|e| e.to_string())?,
                anonymous: person.is_none(),
            });
        }
    }
    Ok(result)
}

/// Human grouping overrides the derived acoustic grouping for these exact utterances.
/// Never reuse cloud label numbers to carry this decision into a new ASR version.
pub fn assign_local_speaker(
    tx: &Transaction<'_>,
    segment_id: Uuid,
    target_segment_id: Uuid,
    entire: bool,
    expected_revision: i64,
    target_revision: i64,
    now: i64,
) -> Result<(), String> {
    expect_revision(
        tx,
        if entire { "speaker_group" } else { "segment" },
        segment_id,
        expected_revision,
    )?;
    expect_revision(tx, "speaker_group", target_segment_id, target_revision)?;
    let candidates = session_speakers(tx, segment_id)?;
    let target = candidates
        .iter()
        .find(|s| s.segment_id == target_segment_id && s.anonymous)
        .ok_or("target must be an anonymous speaker in the current session")?;
    let source = current_scope_segments(tx, segment_id, entire)?;
    let destination = current_scope_segments(tx, target_segment_id, true)?;
    if destination.iter().any(|s| s.0 == segment_id) {
        return Err("speaker is already in this group".into());
    }
    // Snapshot both sets before unbinding: that operation rebuilds acoustic groups.
    let group = if target.group_id.starts_with("manual:") {
        target.group_id.clone()
    } else {
        format!("manual:{}", Uuid::new_v4())
    };
    unbind_speaker(tx, segment_id, entire, now)?;
    for (id, _, _, _) in source.iter().chain(destination.iter()) {
        tx.execute(
            "INSERT INTO local_speaker_assignments(segment_id,group_id,updated_ms)
          VALUES(?1,?2,?3) ON CONFLICT(segment_id) DO UPDATE SET
          group_id=excluded.group_id,updated_ms=excluded.updated_ms",
            params![id.to_string(), group, now],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn bind_speaker(
    tx: &Transaction<'_>,
    segment_id: Uuid,
    person_id: Uuid,
    entire: bool,
    now: i64,
) -> Result<(), String> {
    let exists: i64 = tx
        .query_row(
            "SELECT count(*) FROM people WHERE id=?1",
            params![person_id.to_string()],
            |r| r.get(0),
        )
        .map_err(|error| error.to_string())?;
    if exists == 0 {
        return Err("person not found".to_owned());
    }
    let mut samples_revoked = false;
    let selected = current_scope_segments(tx, segment_id, entire)?;
    let selected_ids: Vec<_> = selected.iter().map(|s| s.0).collect();
    for (id, source, start, end) in selected {
        let previous: Option<String> = tx
            .query_row(
                "SELECT person_id FROM speaker_bindings WHERE current_segment_id=?1
              AND state='active' ORDER BY CASE origin WHEN 'manual' THEN 0 ELSE 1 END,
              updated_ms DESC LIMIT 1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(old) = previous {
            if old != person_id.to_string() {
                tx.execute(
                    "INSERT OR IGNORE INTO denied_matches VALUES (?1,?2,?3)",
                    params![id.to_string(), old, now],
                )
                .map_err(|error| error.to_string())?;
            }
        }
        tx.execute(
            "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,
          updated_ms=?2 WHERE current_segment_id=?1 AND state='active'",
            params![id.to_string(), now],
        )
        .map_err(|error| error.to_string())?;
        samples_revoked |= tx
            .execute(
                "UPDATE voice_samples SET revoked_ms=?2
          WHERE segment_id=?1 AND person_id!=?3 AND revoked_ms IS NULL",
                params![id.to_string(), now, person_id.to_string()],
            )
            .map_err(|error| error.to_string())?
            > 0;
        tx.execute(
            "INSERT INTO speaker_bindings
          (id,original_segment_id,current_segment_id,person_id,source,started_ms,ended_ms,
           origin,apply_scope,state,created_ms,updated_ms)
          VALUES (?1,?2,?2,?3,?4,?5,?6,'manual',?8,'active',?7,?7)",
            params![
                Uuid::new_v4().to_string(),
                id.to_string(),
                person_id.to_string(),
                source,
                start,
                end,
                now,
                if entire { "group" } else { "segment" }
            ],
        )
        .map_err(|error| error.to_string())?;
        maybe_add_sample(tx, id, person_id, now)?;
    }
    if samples_revoked {
        tx.execute(
            "UPDATE identity_state SET generation=generation+1 WHERE id=1",
            [],
        )
        .map_err(|error| error.to_string())?;
    }
    enrollment::queue_short_samples(tx, &selected_ids, person_id, now)?;
    rebuild_for_segment(tx, segment_id, now)?;
    Ok(())
}

pub fn unbind_speaker(
    tx: &Transaction<'_>,
    segment_id: Uuid,
    entire: bool,
    now: i64,
) -> Result<(), String> {
    let mut samples_revoked = false;
    for (id, _, _, _) in current_scope_segments(tx, segment_id, entire)? {
        let mut stmt = tx
            .prepare(
                "SELECT person_id FROM speaker_bindings
          WHERE current_segment_id=?1 AND state='active'",
            )
            .map_err(|error| error.to_string())?;
        let former = stmt
            .query_map(params![id.to_string()], |r| r.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        for person in former {
            tx.execute(
                "INSERT OR IGNORE INTO denied_matches VALUES (?1,?2,?3)",
                params![id.to_string(), person, now],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.execute(
            "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,
          updated_ms=?2 WHERE current_segment_id=?1 AND state='active'",
            params![id.to_string(), now],
        )
        .map_err(|error| error.to_string())?;
        samples_revoked |= tx
            .execute(
                "UPDATE voice_samples SET revoked_ms=?2
          WHERE segment_id=?1 AND revoked_ms IS NULL",
                params![id.to_string(), now],
            )
            .map_err(|error| error.to_string())?
            > 0;
    }
    if samples_revoked {
        tx.execute(
            "UPDATE identity_state SET generation=generation+1 WHERE id=1",
            [],
        )
        .map_err(|error| error.to_string())?;
    }
    rebuild_for_segment(tx, segment_id, now)?;
    Ok(())
}

fn maybe_add_sample(
    tx: &Transaction<'_>,
    segment_id: Uuid,
    person_id: Uuid,
    now: i64,
) -> Result<bool, String> {
    enrollment::add_single(tx, segment_id, person_id, now)
}

pub fn add_voice_sample(tx: &Transaction<'_>, segment_id: Uuid, now: i64) -> Result<(), String> {
    if visible_segment(tx, segment_id)?.is_none() {
        return Err("segment is no longer current".into());
    }
    let person: Option<String> = tx
        .query_row(
            "SELECT person_id FROM speaker_bindings
      WHERE current_segment_id=?1 AND state='active' AND origin='manual'
      ORDER BY updated_ms DESC LIMIT 1",
            params![segment_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let person = person.ok_or_else(|| "confirm this speaker's person first".to_owned())?;
    if !maybe_add_sample(
        tx,
        segment_id,
        Uuid::parse_str(&person).map_err(|e| e.to_string())?,
        now,
    )? {
        return Err("no eligible clean speaker sample".to_owned());
    }
    Ok(())
}

pub fn remove_voice_sample(
    tx: &Transaction<'_>,
    sample_id: Uuid,
    now: i64,
) -> Result<Uuid, String> {
    let sample: Option<(String, String)> = tx
        .query_row(
            "SELECT person_id,segment_id FROM voice_samples
      WHERE id=?1 AND revoked_ms IS NULL",
            params![sample_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (person, segment) = sample.ok_or_else(|| "voice sample not found".to_owned())?;
    tx.execute(
        "UPDATE voice_samples SET revoked_ms=?2 WHERE id=?1",
        params![sample_id.to_string(), now],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE identity_state SET generation=generation+1 WHERE id=1",
        [],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE speaker_bindings SET state='pending_review',current_segment_id=NULL,
      updated_ms=?2 WHERE person_id=?1 AND state='active' AND origin='automatic'",
        params![person, now],
    )
    .map_err(|error| error.to_string())?;
    rebuild_for_segment(
        tx,
        Uuid::parse_str(&segment).map_err(|error| error.to_string())?,
        now,
    )?;
    Uuid::parse_str(&person).map_err(|error| error.to_string())
}

#[derive(Clone)]
pub struct VoiceWork {
    pub segment_id: Uuid,
    pub source: String,
    pub start: i64,
    pub end: i64,
    pub enrollment: Option<enrollment::EnrollmentWork>,
}

impl VoiceWork {
    pub fn audio_ranges(&self) -> Vec<(i64, i64)> {
        match &self.enrollment {
            Some(work) => work.parts.iter().map(|p| (p.start, p.end)).collect(),
            None => vec![(self.start, self.end)],
        }
    }
}

pub fn recover_voice(db: &Connection) -> Result<(), String> {
    db.execute(
        "UPDATE voice_enrollments SET state='pending' WHERE state='processing'",
        [],
    )
    .map_err(|e| e.to_string())?;
    db.execute(
        "UPDATE voice_observations SET state='pending' WHERE state='processing'",
        [],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn take_voice(db: &mut Connection, now: i64) -> Result<Option<VoiceWork>, String> {
    if let Some(work) = enrollment::take(db, now)? {
        return Ok(Some(VoiceWork {
            segment_id: work.id,
            source: work.source.clone(),
            start: work.parts[0].start,
            end: work.parts.last().unwrap().end,
            enrollment: Some(work),
        }));
    }
    let tx = db.transaction().map_err(|error| error.to_string())?;
    let row: Option<(String, String, i64, i64)> = tx
        .query_row(
            "SELECT segment_id,source,started_ms,ended_ms FROM voice_observations
         WHERE state='pending' AND available_ms<=?1 ORDER BY available_ms,updated_ms,segment_id LIMIT 1",
            [now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((id, source, start, end)) = row else {
        return Ok(None);
    };
    let segment_id = Uuid::parse_str(&id).map_err(|error| error.to_string())?;
    if visible_segment(&tx, segment_id)?.is_none() {
        tx.execute(
            "UPDATE voice_observations SET state='stale',updated_ms=?2
          WHERE segment_id=?1",
            params![id, now],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok(None);
    }
    let overlap: i64 = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM precise_segments other
          JOIN precise_segments seed ON seed.id=?1
          WHERE other.task_id=seed.task_id AND other.id!=seed.id
            AND other.source=seed.source AND other.started_ms<seed.ended_ms
            AND other.ended_ms>seed.started_ms
            AND coalesce(other.speaker_label,'')!=coalesce(seed.speaker_label,''))",
            params![id],
            |r| r.get(0),
        )
        .map_err(|error| error.to_string())?;
    if overlap != 0 {
        tx.execute(
            "UPDATE voice_observations SET state='unavailable',error_code='overlapping_speakers',
          updated_ms=?2 WHERE segment_id=?1",
            params![id, now],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok(None);
    }
    tx.execute(
        "UPDATE voice_observations SET state='processing',updated_ms=?2
      WHERE segment_id=?1",
        params![id, now],
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(Some(VoiceWork {
        segment_id,
        source,
        start,
        end,
        enrollment: None,
    }))
}

pub fn fail_voice(db: &Connection, id: Uuid, error_code: &str, now: i64) -> Result<(), String> {
    let deferred = error_code == "resource_deferred";
    let code = match error_code {
        "audio_unavailable" | "audio_quality" | "model_invalid" | "worker_failed"
        | "worker_timeout" | "resource_deferred" => error_code,
        _ => "worker_failed",
    };
    let transient = matches!(code, "worker_failed" | "worker_timeout") || deferred;
    enrollment::fail(db, id, code, now)?;
    db.execute("UPDATE voice_observations SET state=CASE WHEN ?4 AND (attempts<3 OR ?5) THEN 'pending' ELSE 'unavailable' END,
      attempts=attempts+CASE WHEN ?5 THEN 0 ELSE 1 END,available_ms=?3+min(300000,5000*(1<<min(attempts,6))),error_code=?2,
      updated_ms=?3 WHERE segment_id=?1 AND state='processing'",params![id.to_string(),code,now,transient,deferred]).map_err(|e|e.to_string())?;
    Ok(())
}

const VOICE_MODEL_VERSION: &str = "campplus-zh-v1";
const VOICE_DIM: usize = 192;
// Prototype thresholds, pending held-out headset and dual-source evaluation.
const SESSION_MATCH_MIN: f32 = 0.66;
const SESSION_MATCH_MARGIN: f32 = 0.15;
// Distinct labels in one cloud request normally mean distinct voices. Permit
// an acoustic correction only with stronger evidence than cross-request sync.
const SAME_REQUEST_MATCH_MIN: f32 = 0.74;
const SAME_REQUEST_MATCH_MARGIN: f32 = 0.12;
const PERSON_MATCH_MIN: f32 = 0.72;
const PERSON_MATCH_MARGIN: f32 = 0.15;

fn encode_vector(vector: &[f32]) -> Result<Vec<u8>, String> {
    if vector.len() != VOICE_DIM || vector.iter().any(|v| !v.is_finite()) {
        return Err("voice embedding invalid".to_owned());
    }
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm < 0.0001 {
        return Err("voice embedding invalid".to_owned());
    }
    let mut bytes = Vec::with_capacity(VOICE_DIM * 4);
    for value in vector {
        bytes.extend_from_slice(&(value / norm).to_le_bytes());
    }
    Ok(bytes)
}

fn decode_vector(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() != VOICE_DIM * 4 {
        return Err("stored voice embedding invalid".to_owned());
    }
    let values: Vec<_> = bytes
        .chunks_exact(4)
        .map(|piece| f32::from_le_bytes(piece.try_into().unwrap()))
        .collect();
    if values.iter().any(|v| !v.is_finite()) {
        return Err("stored voice embedding invalid".to_owned());
    }
    Ok(values)
}

pub fn publish_voice(
    db: &mut Connection,
    work: &VoiceWork,
    vector: &[f32],
    quality: Option<VoiceQuality>,
    now: i64,
) -> Result<(), String> {
    let bytes = encode_vector(vector)?;
    if let Some(enrollment) = &work.enrollment {
        return enrollment::publish(db, enrollment, &bytes, quality, now);
    }
    let tx = db.transaction().map_err(|error| error.to_string())?;
    if visible_segment(&tx, work.segment_id)?.as_ref()
        != Some(&(work.source.clone(), work.start, work.end))
    {
        tx.execute(
            "UPDATE voice_observations SET state='stale',updated_ms=?2
          WHERE segment_id=?1",
            params![work.segment_id.to_string(), now],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let updated = tx
        .execute(
            "UPDATE voice_observations SET state='ready',vector=?2,
      model_version=?3,error_code=NULL,matched_generation=0,updated_ms=?4
      WHERE segment_id=?1 AND state='processing'",
            params![work.segment_id.to_string(), bytes, VOICE_MODEL_VERSION, now],
        )
        .map_err(|error| error.to_string())?;
    if updated == 0 {
        return Ok(());
    }
    let quality = quality.unwrap_or(VoiceQuality {
        voiced_ms: 0,
        score: 0,
    });
    tx.execute(
        "INSERT OR REPLACE INTO voice_observation_quality VALUES(?1,?2,?3)",
        params![
            work.segment_id.to_string(),
            quality.voiced_ms,
            quality.score
        ],
    )
    .map_err(|e| e.to_string())?;
    let manual: Option<String> = tx
        .query_row(
            "SELECT person_id FROM speaker_bindings
      WHERE current_segment_id=?1 AND state='active' AND origin='manual'",
            params![work.segment_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(person) = manual {
        maybe_add_sample(
            &tx,
            work.segment_id,
            Uuid::parse_str(&person).map_err(|error| error.to_string())?,
            now,
        )?;
    }
    reconcile_session_speaker(&tx, work.segment_id, &bytes, now)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn group_voice_evidence(
    tx: &Transaction<'_>,
    group: &str,
    seed: &(String, String, String, String, String, String),
) -> Result<Vec<(String, String, Vec<f32>)>, String> {
    let mut stmt = tx.prepare(
        "SELECT p.speaker_scope,p.speaker_label,o.vector FROM voice_observations o
         JOIN precise_segments p ON p.id=o.segment_id
         JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
         JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
         WHERE g.group_id=?1 AND t.target_kind=?2 AND t.target_id=?3
           AND t.target_version=?4 AND t.state='complete' AND p.source=?5
           AND o.state='ready' AND o.model_version=?6",
    ).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(
            params![group, seed.3, seed.4, seed.5, seed.2, VOICE_MODEL_VERSION],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    rows.into_iter()
        .map(|(scope, label, bytes)| Ok((scope, label, decode_vector(&bytes)?)))
        .collect()
}

fn group_labels(
    tx: &Transaction<'_>,
    group: &str,
    seed: &(String, String, String, String, String, String),
) -> Result<Vec<(String, String)>, String> {
    let mut stmt = tx.prepare(
        "SELECT DISTINCT p.speaker_scope,p.speaker_label FROM precise_segments p
         JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
         JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
         WHERE g.group_id=?1 AND t.target_kind=?2 AND t.target_id=?3
           AND t.target_version=?4 AND t.state='complete' AND p.source=?5",
    ).map_err(|error| error.to_string())?;
    stmt.query_map(params![group, seed.3, seed.4, seed.5, seed.2], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })
    .map_err(|error| error.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| error.to_string())
}

/// A provider's speaker numbers are local to its request. A merge of two
/// existing groups must be supported by *every* pair of different labels in
/// the same request, not just the closest pair of voice observations.
fn safe_session_merge(
    tx: &Transaction<'_>,
    own: &str,
    winner: &str,
    seed: &(String, String, String, String, String, String),
) -> Result<(bool, bool), String> {
    let overlap: i64 = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM precise_segments a
           JOIN speaker_groups ga ON ga.speaker_scope=a.speaker_scope AND ga.speaker_label=a.speaker_label
           JOIN transcript_tasks ta ON ta.id=a.task_id AND a.started_ms>=ta.started_ms AND a.ended_ms<=ta.ended_ms
           JOIN precise_segments b ON a.source=b.source AND a.started_ms<b.ended_ms
             AND b.started_ms<a.ended_ms
           JOIN speaker_groups gb ON gb.speaker_scope=b.speaker_scope AND gb.speaker_label=b.speaker_label
           JOIN transcript_tasks tb ON tb.id=b.task_id AND b.started_ms>=tb.started_ms AND b.ended_ms<=tb.ended_ms
           WHERE ga.group_id=?1 AND gb.group_id=?2
             AND ta.target_kind=?3 AND ta.target_id=?4 AND ta.target_version=?5
             AND tb.target_kind=ta.target_kind AND tb.target_id=ta.target_id
             AND tb.target_version=ta.target_version
             AND ta.state='complete' AND tb.state='complete')",
        params![own, winner, seed.3, seed.4, seed.5], |r| r.get(0),
    ).map_err(|error| error.to_string())?;
    if overlap != 0 {
        return Ok((false, false));
    }
    let people: i64 = tx.query_row(
        "SELECT count(DISTINCT sb.person_id) FROM speaker_bindings sb
         JOIN precise_segments p ON p.id=sb.current_segment_id
         JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
         JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
         WHERE g.group_id IN (?1,?2) AND sb.state='active'
           AND t.target_kind=?3 AND t.target_id=?4 AND t.target_version=?5
           AND t.state='complete'",
        params![own, winner, seed.3, seed.4, seed.5], |r| r.get(0),
    ).map_err(|error| error.to_string())?;
    if people > 1 {
        return Ok((false, false));
    }
    let own_labels = group_labels(tx, own, seed)?;
    let other_labels = group_labels(tx, winner, seed)?;
    let own_evidence = group_voice_evidence(tx, own, seed)?;
    let other_evidence = group_voice_evidence(tx, winner, seed)?;
    let mut same_request = false;
    for (own_scope, own_label) in &own_labels {
        for (other_scope, other_label) in &other_labels {
            if own_scope != other_scope || own_label == other_label {
                continue;
            }
            same_request = true;
            let scores = own_evidence
                .iter()
                .filter(|(scope, label, _)| scope == own_scope && label == own_label)
                .flat_map(|(_, _, left)| {
                    other_evidence
                        .iter()
                        .filter(move |(scope, label, _)| {
                            scope == other_scope && label == other_label
                        })
                        .map(move |(_, _, right)| {
                            left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>()
                        })
                })
                .collect::<Vec<_>>();
            if !scores.iter().any(|score| *score >= SAME_REQUEST_MATCH_MIN)
                || scores.iter().filter(|score| **score >= 0.70).count() < 2
            {
                return Ok((false, true));
            }
        }
    }
    Ok((true, same_request))
}

fn reconcile_session_speaker(
    tx: &Transaction<'_>,
    segment_id: Uuid,
    vector: &[u8],
    now: i64,
) -> Result<(), String> {
    let seed: (String, String, String, String, String, String) = tx
        .query_row(
            "SELECT p.speaker_scope,p.speaker_label,p.source,t.target_kind,t.target_id,
           t.target_version FROM precise_segments p
           JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms WHERE p.id=?1",
            params![segment_id.to_string()],
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
        .map_err(|error| error.to_string())?;
    let own_group: String = tx
        .query_row(
            "SELECT group_id FROM speaker_groups WHERE speaker_scope=?1 AND speaker_label=?2",
            params![seed.0, seed.1],
            |r| r.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut stmt=tx.prepare(
        "SELECT DISTINCT g.group_id,o.vector,p.speaker_scope FROM voice_observations o
         JOIN precise_segments p ON p.id=o.segment_id
         JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
         JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
         WHERE t.target_kind=?1 AND t.target_id=?2 AND t.target_version=?3
           AND t.state='complete' AND p.source=?4
           AND o.state='ready' AND o.model_version=?5 AND g.group_id!=?6"
    ).map_err(|error| error.to_string())?;
    let candidates = stmt
        .query_map(
            params![
                seed.3,
                seed.4,
                seed.5,
                seed.2,
                VOICE_MODEL_VERSION,
                own_group
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let query = decode_vector(vector)?;
    let mut scores = std::collections::HashMap::<String, f32>::new();
    for (group, candidate, _) in candidates {
        let reference = decode_vector(&candidate)?;
        let score = query.iter().zip(reference).map(|(a, b)| a * b).sum::<f32>();
        scores
            .entry(group)
            .and_modify(|value| *value = value.max(score))
            .or_insert(score);
    }
    let mut ranking: Vec<_> = scores.into_iter().collect();
    ranking.sort_by(|a, b| b.1.total_cmp(&a.1));
    let Some((winner, score)) = ranking.first() else {
        return Ok(());
    };
    let (safe, same_request) = safe_session_merge(tx, &own_group, winner, &seed)?;
    if !safe {
        return Ok(());
    }
    let minimum = if same_request {
        SAME_REQUEST_MATCH_MIN
    } else {
        SESSION_MATCH_MIN
    };
    let margin = if same_request {
        SAME_REQUEST_MATCH_MARGIN
    } else {
        SESSION_MATCH_MARGIN
    };
    if *score < minimum
        || ranking
            .get(1)
            .is_some_and(|(_, other)| *score - *other < margin)
    {
        return Ok(());
    }
    let manual_people = |group: &str| -> Result<Vec<String>, String> {
        let mut stmt = tx.prepare(
            "SELECT DISTINCT sb.person_id FROM speaker_bindings sb
              JOIN precise_segments p ON p.id=sb.current_segment_id
              JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
              WHERE g.group_id=?1 AND sb.state='active' AND sb.origin='manual'",
        ).map_err(|error|error.to_string())?;
        stmt.query_map(params![group], |r| r.get(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    };
    let own_people = manual_people(&own_group)?;
    let winner_people = manual_people(winner)?;
    if own_people.iter().chain(winner_people.iter()).any(|person| {
        own_people
            .first()
            .or(winner_people.first())
            .is_some_and(|first| person != first)
    }) {
        return Ok(());
    }
    tx.execute(
        "UPDATE speaker_groups SET group_id=?2 WHERE group_id=?1",
        params![own_group, winner],
    )
    .map_err(|error| error.to_string())?;
    // A single-sentence correction is evidence against a conflicting merge,
    // but never an instruction to name every turn in the merged group.
    let confirmed_group_person = confirmed_group_person(tx, winner)?;
    if let Some(person) = confirmed_group_person {
        apply_group_binding(tx, winner, None, &person, "automatic", now)?;
    }
    Ok(())
}

/// The stored group ids are a projection of current observations, not durable
/// identity evidence. Rebuild them whenever source data or a human assignment
/// changes, so an old acoustic merge cannot survive its supporting evidence.
pub fn rebuild_speaker_groups(
    tx: &Transaction<'_>,
    target_kind: &str,
    target_id: &str,
    target_version: &str,
    now: i64,
) -> Result<(), String> {
    let target = (target_kind, target_id, target_version);
    tx.execute(
        "UPDATE speaker_groups SET group_id=speaker_scope||':'||speaker_label
         WHERE speaker_scope IN (SELECT DISTINCT p.speaker_scope FROM precise_segments p
           JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
           WHERE t.target_kind=?1 AND t.target_id=?2 AND t.target_version=?3)",
        params![target.0, target.1, target.2],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,updated_ms=?4
         WHERE state='active' AND origin='automatic' AND current_segment_id IN
         (SELECT p.id FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
          WHERE t.target_kind=?1 AND t.target_id=?2 AND t.target_version=?3)",
        params![target.0, target.1, target.2, now],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE voice_observations SET matched_generation=0 WHERE segment_id IN
         (SELECT p.id FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
          WHERE t.target_kind=?1 AND t.target_id=?2 AND t.target_version=?3)",
        params![target.0, target.1, target.2],
    )
    .map_err(|error| error.to_string())?;
    let mut stmt = tx
        .prepare(
            "SELECT o.segment_id,o.vector FROM voice_observations o
             JOIN precise_segments p ON p.id=o.segment_id
             JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
             WHERE t.target_kind=?1 AND t.target_id=?2 AND t.target_version=?3
               AND t.state='complete' AND o.state='ready' AND o.model_version=?4
             ORDER BY p.started_ms,p.id",
        )
        .map_err(|error| error.to_string())?;
    let observations = stmt
        .query_map(
            params![target.0, target.1, target.2, VOICE_MODEL_VERSION],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for (id, vector) in observations {
        reconcile_session_speaker(
            tx,
            Uuid::parse_str(&id).map_err(|error| error.to_string())?,
            &vector,
            now,
        )?;
    }
    Ok(())
}

fn rebuild_for_segment(tx: &Transaction<'_>, segment_id: Uuid, now: i64) -> Result<(), String> {
    let target: (String, String, String) = tx
        .query_row(
            "SELECT t.target_kind,t.target_id,t.target_version FROM precise_segments p
             JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms WHERE p.id=?1",
            params![segment_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|error| error.to_string())?;
    rebuild_speaker_groups(tx, &target.0, &target.1, &target.2, now)
}

// Both acoustic reconciliation and background matching must read human scope
// through effective membership, never the provider group a correction replaced.
fn confirmed_group_person(db: &Connection, group: &str) -> Result<Option<String>, String> {
    db.query_row(
        "SELECT sb.person_id FROM speaker_bindings sb
         JOIN precise_group_membership g ON g.segment_id=sb.current_segment_id
         WHERE g.group_id=?1 AND sb.state='active' AND sb.origin='manual'
         AND sb.apply_scope='group' LIMIT 1",
        [group],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn apply_group_binding(
    tx: &Transaction<'_>,
    group: &str,
    only_segment: Option<Uuid>,
    person: &str,
    origin: &str,
    now: i64,
) -> Result<(), String> {
    let mut stmt=tx.prepare(
        "SELECT p.id,p.source,p.started_ms,p.ended_ms FROM precise_segments p
         JOIN speaker_groups g ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
         WHERE (p.id=?2 OR (?2 IS NULL AND g.group_id=?1
           AND NOT EXISTS(SELECT 1 FROM local_speaker_assignments a WHERE a.segment_id=p.id)))
         ORDER BY p.started_ms,p.id"
    ).map_err(|error|error.to_string())?;
    let segments = stmt
        .query_map(params![group, only_segment.map(|id| id.to_string())], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for (id, source, start, end) in segments {
        let uuid = Uuid::parse_str(&id).map_err(|error| error.to_string())?;
        if only_segment.is_some_and(|selected| selected != uuid) {
            continue;
        }
        if visible_segment(tx, uuid)?.is_none() {
            continue;
        }
        let manual: Option<String> = tx
            .query_row(
                "SELECT person_id FROM speaker_bindings WHERE current_segment_id=?1
             AND state='active' AND origin='manual'",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if manual.is_some() {
            continue;
        }
        let denied: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM denied_matches WHERE segment_id=?1 AND person_id=?2)",
                params![id, person],
                |r| r.get(0),
            )
            .map_err(|error| error.to_string())?;
        if denied != 0 {
            continue;
        }
        let current: Option<String> = tx
            .query_row(
                "SELECT person_id FROM speaker_bindings WHERE current_segment_id=?1
             AND state='active' AND origin='automatic'",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if current.as_deref() == Some(person) {
            continue;
        }
        tx.execute(
            "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,
          updated_ms=?2 WHERE current_segment_id=?1 AND state='active' AND origin='automatic'",
            params![id, now],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO speaker_bindings
             (id,original_segment_id,current_segment_id,person_id,source,started_ms,ended_ms,
              origin,apply_scope,state,created_ms,updated_ms)
             VALUES (?1,?2,?2,?3,?4,?5,?6,?7,'segment','active',?8,?8)",
            params![
                Uuid::new_v4().to_string(),
                id,
                person,
                source,
                start,
                end,
                origin,
                now
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub struct MatchInput {
    generation: i64,
    id: String,
    bytes: Vec<u8>,
    model: String,
    denied: std::collections::HashSet<String>,
}
pub fn prepare_match(db: &Connection) -> Result<Option<MatchInput>, String> {
    let generation: i64 = db
        .query_row(
            "SELECT generation FROM identity_state WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let row: Option<(String, Vec<u8>, String)> = db
        .query_row(
            "SELECT segment_id,vector,model_version FROM voice_observations
      WHERE state='ready' AND matched_generation<?1 ORDER BY matched_generation,updated_ms,segment_id LIMIT 1",
            [generation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((id, bytes, model)) = row else {
        return Ok(None);
    };
    let denied = db
        .prepare("SELECT person_id FROM denied_matches WHERE segment_id=?1")
        .map_err(|e| e.to_string())?
        .query_map([&id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Some(MatchInput {
        generation,
        id,
        bytes,
        model,
        denied,
    }))
}
pub struct SampleRow {
    id: String,
    person: String,
    bytes: Vec<u8>,
}
pub fn match_sample_page(
    db: &Connection,
    input: &MatchInput,
    after: &str,
) -> Result<Vec<SampleRow>, String> {
    db.prepare("SELECT id,person_id,vector FROM voice_samples WHERE revoked_ms IS NULL AND model_version=?1 AND id>?2 ORDER BY id LIMIT 256").map_err(|e|e.to_string())?
      .query_map(params![input.model,after],|r|Ok(SampleRow{id:r.get(0)?,person:r.get(1)?,bytes:r.get(2)?})).map_err(|e|e.to_string())?.collect::<Result<_,_>>().map_err(|e|e.to_string())
}
#[derive(Default)]
pub struct VoiceMatcher {
    key: Option<(i64, String)>,
    samples: Vec<(String, Vec<f32>)>,
}
impl VoiceMatcher {
    pub fn clear(&mut self) {
        self.key = None;
        self.samples.clear();
        self.samples.shrink_to_fit();
    }
    /// At most 4096 decoded samples are cached (about 3 MiB of vectors).
    /// Larger libraries are scored in bounded pages, never truncated.
    pub fn select(
        &mut self,
        input: &MatchInput,
        page: impl FnMut(&str) -> Result<Vec<SampleRow>, String>,
    ) -> Result<Option<String>, String> {
        let ranking = self.rank(input, page)?;
        Ok(ranking
            .first()
            .filter(|(_, value)| {
                *value >= PERSON_MATCH_MIN
                    && ranking
                        .get(1)
                        .is_none_or(|(_, other)| *value - *other >= PERSON_MATCH_MARGIN)
            })
            .map(|(person, _)| person.clone()))
    }
    pub fn allows_enrollment(
        &mut self,
        input: &MatchInput,
        person: &str,
        page: impl FnMut(&str) -> Result<Vec<SampleRow>, String>,
    ) -> Result<bool, String> {
        let ranking = self.rank(input, page)?;
        Ok(ranking.first().is_none_or(|(first, score)| {
            *score < PERSON_MATCH_MIN
                || (first == person
                    && ranking.get(1).is_none_or(|(_, other)| {
                        *other < PERSON_MATCH_MIN || *score - *other >= PERSON_MATCH_MARGIN
                    }))
        }))
    }
    fn rank(
        &mut self,
        input: &MatchInput,
        mut page: impl FnMut(&str) -> Result<Vec<SampleRow>, String>,
    ) -> Result<Vec<(String, f32)>, String> {
        let query = decode_vector(&input.bytes)?;
        let mut ranking: Vec<(String, f32)> = Vec::with_capacity(3);
        let mut score = |person: &str, sample: &[f32]| {
            if input.denied.contains(person) {
                return;
            }
            let value = query.iter().zip(sample).map(|(a, b)| a * b).sum::<f32>();
            if let Some(prior) = ranking.iter_mut().find(|(id, _)| id == person) {
                prior.1 = prior.1.max(value);
            } else {
                ranking.push((person.to_owned(), value));
            }
            ranking.sort_by(|a, b| b.1.total_cmp(&a.1));
            ranking.truncate(2);
        };
        let key = (input.generation, input.model.clone());
        if self.key.as_ref() == Some(&key) {
            for (person, sample) in &self.samples {
                score(person, sample);
            }
        } else {
            self.clear();
            let mut after = String::new();
            let mut cacheable = true;
            loop {
                let rows = page(&after)?;
                let finished = rows.len() < 256;
                for row in rows {
                    after = row.id;
                    let sample = decode_vector(&row.bytes)?;
                    score(&row.person, &sample);
                    if cacheable {
                        if self.samples.len() < 4096 {
                            self.samples.push((row.person, sample));
                        } else {
                            self.samples.clear();
                            cacheable = false;
                        }
                    }
                }
                if finished {
                    break;
                }
            }
            if cacheable {
                self.key = Some(key);
            }
        }
        Ok(ranking)
    }
}

pub fn publish_match(
    db: &mut Connection,
    input: &MatchInput,
    selected: Option<String>,
    now: i64,
) -> Result<bool, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM voice_observations WHERE segment_id=?1 AND state='ready' AND matched_generation<?2 AND vector=?3 AND model_version=?4)
      AND (SELECT generation FROM identity_state WHERE id=1)=?2",params![input.id,input.generation,input.bytes,input.model],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !current {
        return Ok(false);
    }
    let id = &input.id;
    let bytes = &input.bytes;
    let generation = input.generation;
    let segment = Uuid::parse_str(&id).map_err(|error| error.to_string())?;
    // A generation changes when samples or denials change. Re-evaluate the
    // old automatic answer within this transaction, including the case where
    // no person now meets the threshold.
    tx.execute(
        "UPDATE speaker_bindings SET state='revoked',current_segment_id=NULL,updated_ms=?2
         WHERE current_segment_id=?1 AND state='active' AND origin='automatic'",
        params![id, now],
    )
    .map_err(|error| error.to_string())?;
    if visible_segment(&tx, segment)?.is_some() {
        reconcile_session_speaker(&tx, segment, &bytes, now)?;
        let group: String = tx
            .query_row(
                "SELECT g.group_id FROM precise_segments p JOIN speaker_groups g
              ON g.speaker_scope=p.speaker_scope AND g.speaker_label=p.speaker_label
              WHERE p.id=?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(|error| error.to_string())?;
        let manual_person = confirmed_group_person(&tx, &group)?;
        let manual: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM speaker_bindings
          WHERE current_segment_id=?1 AND state='active' AND origin='manual')",
                params![id],
                |r| r.get(0),
            )
            .map_err(|error| error.to_string())?;
        if let Some(person) = manual_person {
            apply_group_binding(&tx, &group, None, &person, "automatic", now)?;
        } else if manual == 0 {
            let selected_id = selected;
            if let Some(person) = selected_id {
                // A provider label is an anonymous hint, not proof that all
                // its utterances have one voice. Name only the utterance with
                // a matching local embedding; short/unmatched turns stay
                // anonymous until the user confirms them.
                apply_group_binding(&tx, &group, Some(segment), &person, "automatic", now)?;
            }
        }
    }
    tx.execute(
        "UPDATE voice_observations SET matched_generation=?2 WHERE segment_id=?1",
        params![id, generation],
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(test)]
pub fn match_one_ready(db: &mut Connection, now: i64) -> Result<bool, String> {
    let Some(input) = prepare_match(db)? else {
        return Ok(false);
    };
    let selected =
        VoiceMatcher::default().select(&input, |after| match_sample_page(db, &input, after))?;
    publish_match(db, &input, selected, now)
}

pub fn correct_text(
    tx: &Transaction<'_>,
    segment_id: Uuid,
    text: &str,
    now: i64,
) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > 2_000 || text.contains('\0') {
        return Err("invalid correction text".to_owned());
    }
    let (source, start, end) = visible_segment(tx, segment_id)?
        .ok_or_else(|| "precise segment is no longer current".to_owned())?;
    crate::events::invalidate_event_understanding(
        tx,
        start,
        end,
        if source == "microphone" { 1 } else { 2 },
    )?;
    tx.execute(
        "UPDATE text_corrections SET state='reverted',current_segment_id=NULL,
          updated_ms=?2,revision=revision+1
          WHERE current_segment_id=?1 AND state='active'",
        params![segment_id.to_string(), now],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "INSERT INTO text_corrections
         (id,original_segment_id,anchor_segment_id,current_segment_id,source,
          started_ms,ended_ms,text,state,revision,created_ms,updated_ms)
         VALUES (?1,?2,?2,?2,?3,?4,?5,?6,'active',1,?7,?7)",
        params![
            Uuid::new_v4().to_string(),
            segment_id.to_string(),
            source,
            start,
            end,
            text,
            now
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn revert_text(tx: &Transaction<'_>, segment_id: Uuid, now: i64) -> Result<(), String> {
    if visible_segment(tx, segment_id)?.is_none() {
        return Err("precise segment is no longer current".to_owned());
    }
    let changed = tx
        .execute(
            "UPDATE text_corrections SET state='reverted',current_segment_id=NULL,
              updated_ms=?2,revision=revision+1
              WHERE current_segment_id=?1 AND state='active'",
            params![segment_id.to_string(), now],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("segment has no active correction".to_owned());
    }
    Ok(())
}

/// Reattach only an unambiguous source/time match. Ambiguous edits remain
/// visible to a later review tool, but cannot overwrite a different sentence.
pub fn reconcile_corrections(
    tx: &Transaction<'_>,
    new_segments: &[(Uuid, String, i64, i64)],
    now: i64,
) -> Result<(), String> {
    for (new_id, source, start, end) in new_segments {
        let mut stmt = tx
            .prepare(
                "SELECT c.id,c.started_ms,c.ended_ms FROM text_corrections c
                 JOIN precise_segments old_p ON old_p.id=c.anchor_segment_id
                 JOIN cloud_tasks old_t ON old_t.id=old_p.task_id
                 JOIN precise_segments new_p ON new_p.id=?4
                 JOIN transcript_tasks new_t ON new_t.id=new_p.task_id AND new_p.started_ms>=new_t.started_ms AND new_p.ended_ms<=new_t.ended_ms
                 WHERE c.state='pending_review' AND c.source=?1
                   AND c.started_ms<?3 AND c.ended_ms>?2
                   AND old_t.target_kind=new_t.target_kind
                   AND old_t.target_id=new_t.target_id
                 ORDER BY c.updated_ms DESC",
            )
            .map_err(|error| error.to_string())?;
        let candidates = stmt
            .query_map(params![source, start, end, new_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let plausible: Vec<_> = candidates
            .into_iter()
            .filter(|(_, old_start, old_end)| {
                let overlap = (*end).min(*old_end) - (*start).max(*old_start);
                overlap > 0
                    && overlap * 10 >= (*end - *start) * 9
                    && overlap * 10 >= (old_end - old_start) * 9
            })
            .collect();
        if plausible.len() != 1 {
            continue;
        }
        let correction_id = &plausible[0].0;
        let competing = new_segments
            .iter()
            .filter(|(_, other_source, a, b)| {
                other_source == source && {
                    let overlap = (b.min(&plausible[0].2) - a.max(&plausible[0].1)).max(0);
                    overlap * 10 >= (plausible[0].2 - plausible[0].1) * 9
                        && overlap * 10 >= (b - a) * 9
                }
            })
            .count();
        if competing != 1 {
            continue;
        }
        tx.execute(
            "UPDATE text_corrections SET current_segment_id=?2,anchor_segment_id=?2,
              started_ms=?4,ended_ms=?5,state='active',updated_ms=?3,
              revision=revision+1 WHERE id=?1",
            params![correction_id, new_id.to_string(), now, start, end],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn enqueue_voice_observation(
    tx: &Transaction<'_>,
    segment_id: Uuid,
    speaker_key: &str,
    source: &str,
    start: i64,
    end: i64,
    now: i64,
) -> Result<(), String> {
    if end - start < 2000 || end - start > 15_000 {
        return Ok(());
    }
    tx.execute(
        "INSERT OR IGNORE INTO voice_observations
      (segment_id,speaker_key,source,started_ms,ended_ms,state,vector,model_version,
       error_code,matched_generation,updated_ms)
      VALUES (?1,?2,?3,?4,?5,'pending',NULL,NULL,NULL,0,?6)",
        params![segment_id.to_string(), speaker_key, source, start, end, now],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn register_speaker(
    tx: &Transaction<'_>,
    speaker_scope: &str,
    label: &str,
) -> Result<(), String> {
    tx.execute(
        "INSERT OR IGNORE INTO speaker_groups VALUES (?1,?2,?3)",
        params![speaker_scope, label, format!("{speaker_scope}:{label}")],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn pending_event_corrections(
    tx: &Transaction<'_>,
    event_id: Uuid,
    now: i64,
) -> Result<(), String> {
    tx.execute(
        "UPDATE text_corrections SET state='pending_review',current_segment_id=NULL,
         updated_ms=?2,revision=revision+1
         WHERE state='active' AND current_segment_id IN (
           SELECT p.id FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
           WHERE t.target_kind='event' AND t.target_id=?1)",
        params![event_id.to_string(), now],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE speaker_bindings SET state='pending_review',current_segment_id=NULL,
      updated_ms=?2 WHERE state='active' AND current_segment_id IN (
        SELECT p.id FROM precise_segments p JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
        WHERE t.target_kind='event' AND t.target_id=?1)",
        params![event_id.to_string(), now],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::AsrUtterance;
    use crate::events;
    use hear_protocol::{CloudProvider, EventSources};

    fn publish_voice(
        db: &mut Connection,
        work: &VoiceWork,
        vector: &[f32],
        now: i64,
    ) -> Result<(), String> {
        let ms = work.end - work.start;
        super::publish_voice(
            db,
            work,
            vector,
            Some(VoiceQuality {
                voiced_ms: ms,
                score: ms.min(10000),
            }),
            now,
        )?;
        enrollment::drain_enrollment(db, now)
    }

    fn assign_test(
        db: &mut Connection,
        source: Uuid,
        target: Uuid,
        entire: bool,
    ) -> Result<(), String> {
        let source_revision =
            object_revision(db, if entire { "speaker_group" } else { "segment" }, source)?;
        let target_revision = object_revision(db, "speaker_group", target)?;
        let tx = db.transaction().unwrap();
        assign_local_speaker(
            &tx,
            source,
            target,
            entire,
            source_revision,
            target_revision,
            70000,
        )?;
        tx.commit().map_err(|e| e.to_string())
    }

    #[test]
    fn manual_local_speaker_assignment_respects_sentence_scope_and_survives_rebuild() {
        let (mut db, event, segments) = speaker_merge_fixture(&[
            ("A", 0, 4000),
            ("A", 4000, 8000),
            ("B", 8000, 12000),
            ("B", 12000, 16000),
        ]);
        let before = segments[0].speaker_revision;
        assign_test(&mut db, segments[0].id, segments[2].id, false).unwrap();
        let detail = events::event_detail(&db, event, 0, 20).unwrap();
        assert_eq!(
            detail.precise_segments[0].speaker,
            detail.precise_segments[2].speaker
        );
        assert_ne!(
            detail.precise_segments[1].speaker,
            detail.precise_segments[2].speaker
        );
        assert_ne!(before, detail.precise_segments[1].speaker_revision);
        assert!(people(&db).unwrap().is_empty());
        // Reopen the schema and re-run automatic grouping, as on application restart.
        init(&db).unwrap();
        let tx = db.transaction().unwrap();
        rebuild_for_segment(&tx, segments[0].id, 71000).unwrap();
        tx.commit().unwrap();
        let detail = events::event_detail(&db, event, 0, 20).unwrap();
        assert_eq!(
            detail.precise_segments[0].speaker,
            detail.precise_segments[2].speaker
        );
        assert_ne!(
            detail.precise_segments[1].speaker,
            detail.precise_segments[2].speaker
        );
        // Later whole-group naming follows the corrected group, not the provider label.
        let tx = db.transaction().unwrap();
        let person = create_person(&tx, "本场人物", 72000).unwrap();
        bind_speaker(&tx, segments[0].id, person, true, 72001).unwrap();
        tx.commit().unwrap();
        let detail = events::event_detail(&db, event, 0, 20).unwrap();
        for i in [0, 2, 3] {
            assert_eq!(detail.precise_segments[i].person_id, Some(person));
        }
        assert!(detail.precise_segments[1].person_id.is_none());
        assert_eq!(people(&db).unwrap()[0].sample_count, 0);
        // A later background match with no person hit must not inherit the old A label's name.
        let mut other_voice = vec![0.0; VOICE_DIM];
        other_voice[1] = 1.0;
        db.execute(
            "UPDATE voice_observations SET state='ready',vector=?2,model_version=?3,
          matched_generation=0 WHERE segment_id=?1",
            params![
                segments[1].id.to_string(),
                encode_vector(&other_voice).unwrap(),
                "campplus-zh-v1"
            ],
        )
        .unwrap();
        assert!(match_one_ready(&mut db, 73000).unwrap());
        let detail = events::event_detail(&db, event, 0, 20).unwrap();
        assert!(
            detail.precise_segments[1].person_id.is_none(),
            "old acoustic group leaked the manually corrected group's identity"
        );
        // Explicit delete trigger also works when SQLite foreign keys are not enabled.
        db.execute(
            "DELETE FROM precise_segments WHERE id=?1",
            [segments[0].id.to_string()],
        )
        .unwrap();
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM local_speaker_assignments WHERE segment_id=?1",
                [segments[0].id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn manual_local_speaker_merge_validates_both_revisions_and_session_boundaries() {
        let (mut db, event, segments) = speaker_merge_fixture(&[
            ("A", 0, 4000),
            ("A", 4000, 8000),
            ("B", 8000, 12000),
            ("B", 12000, 16000),
        ]);
        let tx = db.transaction().unwrap();
        assert!(
            assign_local_speaker(
                &tx,
                segments[0].id,
                segments[2].id,
                true,
                segments[0].speaker_revision + 1,
                segments[2].speaker_revision,
                70000
            )
            .unwrap_err()
            .contains("revision_conflict")
        );
        assert!(
            assign_local_speaker(
                &tx,
                segments[0].id,
                segments[2].id,
                true,
                segments[0].speaker_revision,
                segments[2].speaker_revision + 1,
                70000
            )
            .unwrap_err()
            .contains("revision_conflict")
        );
        tx.rollback().unwrap();
        // A named target is not an anonymous merge destination.
        let tx = db.transaction().unwrap();
        let person = create_person(&tx, "已有姓名", 70000).unwrap();
        bind_speaker(&tx, segments[2].id, person, false, 70001).unwrap();
        tx.commit().unwrap();
        assert!(assign_test(&mut db, segments[0].id, segments[2].id, true).is_err());
        let tx = db.transaction().unwrap();
        unbind_speaker(&tx, segments[2].id, false, 70002).unwrap();
        tx.commit().unwrap();
        assign_test(&mut db, segments[0].id, segments[2].id, true).unwrap();
        let detail = events::event_detail(&db, event, 0, 20).unwrap();
        assert!(
            detail
                .precise_segments
                .iter()
                .all(|s| s.speaker == detail.precise_segments[0].speaker)
        );
        // A separately targeted event using the same cloud labels cannot be chosen.
        db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES('other-audio','session','microphone',60000,100000,2832000,1920000,100,'media/microphone/other.opus','retain','complete',1)",[]).unwrap();
        let other = events::create_event(
            &mut db,
            60000,
            100000,
            "另一场",
            EventSources::Microphone,
            80000,
        )
        .unwrap();
        let task = events::take_task(&mut db, CloudProvider::Asr, 80001)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &task,
            &[AsrUtterance {
                words: None,
                start_ms: 0,
                end_ms: 4000,
                text: "另一场发言".into(),
                speaker: Some("B".into()),
            }],
            80002,
        )
        .unwrap();
        let other_segment = events::event_detail(&db, other, 0, 20)
            .unwrap()
            .precise_segments[0]
            .id;
        assert!(
            assign_test(&mut db, segments[0].id, other_segment, true)
                .unwrap_err()
                .contains("current session")
        );
    }

    #[test]
    fn session_speaker_choices_include_unloaded_pages_and_do_not_cross_retranscription() {
        let mut utterances = Vec::new();
        for i in 0..105 {
            utterances.push(("A", i * 400, i * 400 + 400));
        }
        utterances.push(("B", 43000, 47000));
        let (mut db, event, segments) = speaker_merge_fixture(&utterances);
        let speakers = session_speakers(&db, segments[0].id).unwrap();
        assert_eq!(speakers.len(), 2);
        assert_eq!(speakers[1].name, "说话人 2");
        assert!(!segments.iter().any(|s| s.id == speakers[1].segment_id));
        assign_test(&mut db, segments[0].id, speakers[1].segment_id, false).unwrap();
        let tx = db.transaction().unwrap();
        // Superseded output is no longer current and may not accept further corrections.
        tx.execute(
            "UPDATE cloud_tasks SET state='superseded' WHERE target_id=?1",
            [event.to_string()],
        )
        .unwrap();
        assert!(session_speakers(&tx, segments[0].id).is_err());
        tx.commit().unwrap();
        // New segments have no assignment: local decisions are tied to original utterance IDs.
        let count: i64 = db
            .query_row("SELECT count(*) FROM local_speaker_assignments", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn common_and_rare_fts_plans_preserve_time_order_ranges_and_pagination() {
        let store = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        let tx = store.db.unchecked_transaction().unwrap();
        for i in 1..=4200 {
            tx.execute(
                "INSERT INTO search_records(kind,item_id,source,started_ms,ended_ms,text)
                VALUES('note',?1,NULL,?2,?2+1,?3)",
                params![
                    Uuid::from_u128(i as u128).to_string(),
                    i,
                    if i % 100 == 0 {
                        "commonword rareword"
                    } else {
                        "commonword"
                    }
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let hits = search(&store.db, Some("commonword"), 0, 5000, None, 20, 50).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.started_ms).collect::<Vec<_>>(),
            (4131..=4180).rev().collect::<Vec<_>>()
        );
        let hits = search(&store.db, Some("commonword"), 4000, 4050, None, 0, 50).unwrap();
        assert_eq!(hits.len(), 50);
        assert_eq!(hits[0].started_ms, 4049);
        let hits = search(&store.db, Some("rareword"), 0, 5000, None, 0, 50).unwrap();
        assert_eq!(hits.len(), 42);
        assert_eq!(hits[0].started_ms, 4200);
    }

    #[test]
    fn transient_voice_failures_retry_but_pressure_does_not_consume_attempts() {
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let id = Uuid::new_v4();
        db.execute("INSERT INTO voice_observations(segment_id,speaker_key,source,started_ms,ended_ms,state,updated_ms) VALUES(?1,'speaker','microphone',0,3000,'processing',0)",[id.to_string()]).unwrap();
        fail_voice(&db, id, "resource_deferred", 100).unwrap();
        let row: (String, i64, i64) = db
            .query_row(
                "SELECT state,attempts,available_ms FROM voice_observations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("pending".into(), 0, 5100));
        db.execute("UPDATE voice_observations SET state='processing'", [])
            .unwrap();
        fail_voice(&db, id, "worker_timeout", 200).unwrap();
        let row: (String, i64) = db
            .query_row("SELECT state,attempts FROM voice_observations", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row, ("pending".into(), 1));
        db.execute("UPDATE voice_observations SET state='processing'", [])
            .unwrap();
        fail_voice(&db, id, "audio_quality", 300).unwrap();
        assert_eq!(
            db.query_row("SELECT state FROM voice_observations", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "unavailable"
        );
    }
    #[test]
    fn voice_scoring_cache_is_bounded_and_never_truncates_a_large_library() {
        let mut vector = vec![0.; VOICE_DIM];
        vector[0] = 1.;
        let input = MatchInput {
            generation: 1,
            id: "q".into(),
            bytes: encode_vector(&vector).unwrap(),
            model: VOICE_MODEL_VERSION.into(),
            denied: Default::default(),
        };
        let mut matcher = VoiceMatcher::default();
        let mut calls = 0;
        let result = matcher
            .select(&input, |after| {
                calls += 1;
                let start = if after.is_empty() {
                    0
                } else {
                    after.parse::<usize>().unwrap() + 1
                };
                Ok((start..(start + 256).min(4100))
                    .map(|i| {
                        let mut v = vec![0.; VOICE_DIM];
                        v[usize::from(i != 4099)] = 1.;
                        SampleRow {
                            id: format!("{i:06}"),
                            person: if i == 4099 { "winner" } else { "other" }.into(),
                            bytes: encode_vector(&v).unwrap(),
                        }
                    })
                    .collect())
            })
            .unwrap();
        assert_eq!(result, Some("winner".into()));
        assert!(calls > 16);
        assert!(matcher.samples.len() <= 4096);
        assert!(matcher.key.is_none());
        let page = |after: &str| -> Result<Vec<SampleRow>, String> {
            assert!(after.is_empty());
            Ok(vec![SampleRow {
                id: "a".into(),
                person: "winner".into(),
                bytes: input.bytes.clone(),
            }])
        };
        matcher.select(&input, page).unwrap();
        assert_eq!(
            matcher
                .select(&input, |_| panic!("cache should be reused"))
                .unwrap(),
            Some("winner".into())
        );
    }
    #[test]
    fn stale_voice_match_generation_cannot_publish() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let id = Uuid::new_v4();
        let mut v = vec![0.; VOICE_DIM];
        v[0] = 1.;
        let bytes = encode_vector(&v).unwrap();
        db.execute("INSERT INTO voice_observations(segment_id,speaker_key,source,started_ms,ended_ms,state,vector,model_version,updated_ms)
          VALUES(?1,'s','microphone',0,3000,'ready',?2,?3,0)",params![id.to_string(),bytes,VOICE_MODEL_VERSION]).unwrap();
        let input = prepare_match(&db).unwrap().unwrap();
        db.execute("UPDATE identity_state SET generation=generation+1", [])
            .unwrap();
        assert!(!publish_match(&mut db, &input, Some("obsolete".into()), 1).unwrap());
        assert_eq!(
            db.query_row(
                "SELECT matched_generation FROM voice_observations",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn content_revision_survives_javascript_number_round_trip() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        let tx = db.transaction().unwrap();
        let person = create_person(&tx, "测试人物", 1000).unwrap();
        tx.commit().unwrap();
        let revision = object_revision(&db, "person", person).unwrap();
        assert!(revision > 0);
        assert!(revision <= (1 << 53) - 1);
        assert_eq!(revision, revision as f64 as i64);
    }

    #[test]
    fn search_empty_reason_distinguishes_unrecorded_and_identity_pending() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        assert_eq!(
            search_empty_reason(&db, 1000, 2000, None).unwrap(),
            "not_recorded"
        );
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
          frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES ('a','session','microphone',1000,6000,0,240000,100,
          'media/microphone/a.opus','retain','complete',1)",
            [],
        )
        .unwrap();
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event =
            events::create_event(&mut db, 1000, 6000, "会话", EventSources::Microphone, 8000)
                .unwrap();
        let task = events::take_task(&mut db, CloudProvider::Asr, 8001)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &task,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 4000,
                text: "你好".into(),
                speaker: Some("1".into()),
            }],
            8002,
        )
        .unwrap();
        assert_eq!(
            events::event_detail(&db, event, 0, 10)
                .unwrap()
                .precise_segments
                .len(),
            1
        );
        assert_eq!(
            search_empty_reason(&db, 1000, 6000, Some(Uuid::new_v4())).unwrap(),
            "identity_pending"
        );
        assert_eq!(
            search_empty_reason(&db, 1000, 6000, None).unwrap(),
            "no_match"
        );
    }

    #[test]
    fn review_restores_whole_speaker_group_after_retranscription() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
          frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES ('a','session','microphone',1000,6000,0,240000,100,
          'media/microphone/a.opus','retain','complete',1)",
            [],
        )
        .unwrap();
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event =
            events::create_event(&mut db, 1000, 6000, "会议", EventSources::Microphone, 8000)
                .unwrap();
        let task = events::take_task(&mut db, CloudProvider::Asr, 8001)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &task,
            &[
                AsrUtterance {
                    words: None,
                    start_ms: 500,
                    end_ms: 1800,
                    text: "一句".into(),
                    speaker: Some("1".into()),
                },
                AsrUtterance {
                    words: None,
                    start_ms: 1800,
                    end_ms: 2800,
                    text: "二句".into(),
                    speaker: Some("1".into()),
                },
            ],
            8002,
        )
        .unwrap();
        let old = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        let tx = db.transaction().unwrap();
        let person = create_person(&tx, "参会者", 8003).unwrap();
        bind_speaker(&tx, old[0].id, person, true, 8004).unwrap();
        tx.commit().unwrap();
        events::edit_event(&mut db, event, 1, 1000, 7000, "会议", None, 8005).unwrap();
        {
            let tx = db.transaction().unwrap();
            events::retranscribe_event_in(&tx, event, 2, 8005).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(review_items(&db).unwrap().len(), 2);
        let task = events::take_task(&mut db, CloudProvider::Asr, 8006)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &task,
            &[
                AsrUtterance {
                    words: None,
                    start_ms: 500,
                    end_ms: 1800,
                    text: "一句新版".into(),
                    speaker: Some("2".into()),
                },
                AsrUtterance {
                    words: None,
                    start_ms: 1800,
                    end_ms: 2800,
                    text: "二句新版".into(),
                    speaker: Some("2".into()),
                },
            ],
            8007,
        )
        .unwrap();
        let new = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        let review = review_items(&db).unwrap().remove(0);
        let tx = db.transaction().unwrap();
        resolve_review(&tx, review.id, Some(new[0].id), 8008).unwrap();
        tx.commit().unwrap();
        assert!(review_items(&db).unwrap().is_empty());
        let restored = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert!(restored.iter().all(|item| item.person_id == Some(person)));
        let (left, right) =
            events::split_event(&mut db, event, 2, 2800, "左段", "右段", 8009).unwrap();
        assert!(
            events::take_task(&mut db, CloudProvider::Asr, 8010)
                .unwrap()
                .is_none()
        );
        for (id, expected) in [(left, restored[0].id), (right, restored[1].id)] {
            let detail = events::event_detail(&db, id, 0, 10).unwrap();
            assert_eq!(detail.precise_segments.len(), 1);
            assert_eq!(detail.precise_segments[0].id, expected);
            assert_eq!(detail.precise_segments[0].person_id, Some(person));
        }
        assert!(review_items(&db).unwrap().is_empty());
    }

    #[test]
    fn long_event_merges_acoustically_verified_speakers_and_short_turns() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        for (id, start, end) in [("early", 1000, 6000), ("late", 931000, 936000)] {
            db.execute("INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,
                sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
                VALUES (?1,'session','microphone',?2,?3,0,240000,100,
                'media/microphone/block.opus','retain','complete',1)",params![id,start,end]).unwrap();
        }
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event = events::create_event(
            &mut db,
            1000,
            936000,
            "长会议",
            EventSources::Microphone,
            20000,
        )
        .unwrap();
        let first = events::take_task(&mut db, CloudProvider::Asr, 20001)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &first,
            &[AsrUtterance {
                words: None,
                start_ms: 0,
                end_ms: 3500,
                text: "第一段".into(),
                speaker: Some("1".into()),
            }],
            20002,
        )
        .unwrap();
        let second = events::take_task(&mut db, CloudProvider::Asr, 20003)
            .unwrap()
            .unwrap();
        assert_ne!(first.id, second.id);
        events::publish_asr(
            &mut db,
            &second,
            &[
                AsrUtterance {
                    words: None,
                    start_ms: 0,
                    end_ms: 3500,
                    text: "第二段".into(),
                    speaker: Some("2".into()),
                },
                AsrUtterance {
                    words: None,
                    start_ms: 3500,
                    end_ms: 4500,
                    text: "短句".into(),
                    speaker: Some("2".into()),
                },
            ],
            20004,
        )
        .unwrap();
        let segments = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert_eq!(segments.len(), 3);
        db.execute(
            "UPDATE voice_observations SET updated_ms=0 WHERE segment_id=?1",
            params![segments[1].id.to_string()],
        )
        .unwrap();
        let mut vector = vec![0.0; VOICE_DIM];
        vector[0] = 1.0;
        let work = take_voice(&mut db, 20005).unwrap().unwrap();
        publish_voice(&mut db, &work, &vector, 20006).unwrap();
        let work = take_voice(&mut db, 20007).unwrap().unwrap();
        publish_voice(&mut db, &work, &vector, 20008).unwrap();
        let grouped = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert!(
            grouped
                .iter()
                .all(|item| item.speaker == grouped[0].speaker)
        );
        let tx = db.transaction().unwrap();
        let person = create_person(&tx, "同一个人", 20009).unwrap();
        bind_speaker(&tx, segments[0].id, person, false, 20009).unwrap();
        tx.commit().unwrap();
        let local_only = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert_eq!(local_only[0].person_id, Some(person));
        assert!(local_only[1..].iter().all(|item| item.person_id.is_none()));
        let tx = db.transaction().unwrap();
        bind_speaker(&tx, segments[0].id, person, true, 20010).unwrap();
        tx.commit().unwrap();
        let bound = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert!(bound.iter().all(|item| item.person_id == Some(person)));
        let tx = db.transaction().unwrap();
        let other = create_person(&tx, "误分段者", 20011).unwrap();
        bind_speaker(&tx, segments[2].id, other, false, 20012).unwrap();
        tx.commit().unwrap();
        while match_one_ready(&mut db, 20013).unwrap() {}
        let corrected = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert_eq!(corrected[0].person_id, Some(person));
        assert_eq!(corrected[1].person_id, Some(person));
        assert_eq!(corrected[2].person_id, Some(other));
        assert_ne!(corrected[0].speaker, corrected[1].speaker);
        let tx = db.transaction().unwrap();
        tx.execute(
            "UPDATE speaker_bindings SET state='pending_review',current_segment_id=NULL
             WHERE original_segment_id=?1 AND person_id=?2 AND origin='manual'",
            params![segments[2].id.to_string(), other.to_string()],
        )
        .unwrap();
        let target: (String, String, String) = tx
            .query_row(
                "SELECT t.target_kind,t.target_id,t.target_version FROM precise_segments p
             JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms WHERE p.id=?1",
                params![segments[2].id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        rebuild_speaker_groups(&tx, &target.0, &target.1, &target.2, 20014).unwrap();
        tx.commit().unwrap();
        let review = review_items(&db)
            .unwrap()
            .into_iter()
            .find(|item| item.person_id == Some(other))
            .unwrap();
        let tx = db.transaction().unwrap();
        resolve_review(&tx, review.id, Some(segments[2].id), 20015).unwrap();
        tx.commit().unwrap();
        let restored = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert_eq!(restored[2].person_id, Some(other));
        assert_ne!(restored[0].speaker, restored[1].speaker);
    }

    #[test]
    fn one_cloud_request_can_correct_a_strong_speaker_split_without_merging_a_third_voice() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,
             sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
             VALUES ('meeting','session','microphone',1000,15000,0,672000,100,
             'media/microphone/meeting.opus','retain','complete',1)",
            [],
        )
        .unwrap();
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event = events::create_event(
            &mut db,
            1000,
            15000,
            "三段对话",
            EventSources::Microphone,
            13000,
        )
        .unwrap();
        let task = events::take_task(&mut db, CloudProvider::Asr, 13001)
            .unwrap()
            .unwrap();
        let utterances = ["1", "1", "2", "3"]
            .into_iter()
            .enumerate()
            .map(|(index, speaker)| AsrUtterance {
                words: None,
                start_ms: index as i64 * 3000,
                end_ms: index as i64 * 3000 + 3000,
                text: "发言".into(),
                speaker: Some(speaker.into()),
            })
            .collect::<Vec<_>>();
        events::publish_asr(&mut db, &task, &utterances, 13002).unwrap();
        let segments = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        for (index, segment) in segments.iter().enumerate() {
            db.execute(
                "UPDATE voice_observations SET updated_ms=?2 WHERE segment_id=?1",
                params![segment.id.to_string(), index as i64],
            )
            .unwrap();
        }
        let mut first = vec![0.0; VOICE_DIM];
        first[0] = 1.0;
        let mut split = vec![0.0; VOICE_DIM];
        split[0] = 0.8;
        split[1] = 0.6;
        let mut third = vec![0.0; VOICE_DIM];
        third[1] = 1.0;
        for (index, vector) in [first.clone(), first, split, third].iter().enumerate() {
            let work = take_voice(&mut db, 13003 + index as i64).unwrap().unwrap();
            assert_eq!(work.segment_id, segments[index].id);
            publish_voice(&mut db, &work, vector, 13006 + index as i64).unwrap();
        }
        let grouped = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert_eq!(grouped[0].speaker, grouped[1].speaker);
        assert_eq!(grouped[0].speaker, grouped[2].speaker);
        assert_ne!(grouped[0].speaker, grouped[3].speaker);

        db.execute(
            "UPDATE speaker_groups SET group_id=speaker_scope||':'||speaker_label
             WHERE speaker_scope=?1",
            params![task.id.to_string()],
        )
        .unwrap();
        db.execute(
            "UPDATE identity_state SET speaker_policy_version=0 WHERE id=1",
            [],
        )
        .unwrap();
        init(&db).unwrap();
        while match_one_ready(&mut db, 13010).unwrap() {}
        let regrouped = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert_eq!(regrouped[0].speaker, regrouped[1].speaker);
        assert_eq!(regrouped[0].speaker, regrouped[2].speaker);
        assert_ne!(regrouped[0].speaker, regrouped[3].speaker);
    }

    pub(super) fn speaker_merge_fixture(
        utterances: &[(&str, i64, i64)],
    ) -> (Connection, Uuid, Vec<hear_protocol::PreciseSegment>) {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,
             sample_start,frames,bytes,media_path,retention_decision,analysis_state,speech_active)
             VALUES ('speaker-test','session','microphone',1000,60000,0,2832000,100,
             'media/microphone/speaker-test.opus','retain','complete',1)",
            [],
        )
        .unwrap();
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event = events::create_event(
            &mut db,
            1000,
            60000,
            "说话人对照",
            EventSources::Microphone,
            61000,
        )
        .unwrap();
        let task = events::take_task(&mut db, CloudProvider::Asr, 61001)
            .unwrap()
            .unwrap();
        let utterances = utterances
            .iter()
            .map(|(speaker, start, end)| AsrUtterance {
                words: None,
                start_ms: *start,
                end_ms: *end,
                text: "对照语音".into(),
                speaker: Some((*speaker).into()),
            })
            .collect::<Vec<_>>();
        events::publish_asr(&mut db, &task, &utterances, 61002).unwrap();
        let segments = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        for (index, segment) in segments.iter().enumerate() {
            db.execute(
                "UPDATE voice_observations SET updated_ms=?2 WHERE segment_id=?1",
                params![segment.id.to_string(), index as i64],
            )
            .unwrap();
        }
        (db, event, segments)
    }

    #[test]
    fn cloud_label_does_not_spread_an_automatic_person_to_another_voice() {
        let (mut db, event, segments) =
            speaker_merge_fixture(&[("A", 0, 3000), ("A", 3000, 6000), ("A", 6000, 9000)]);
        let tx = db.transaction().unwrap();
        let person = create_person(&tx, "已登记人物", 63000).unwrap();
        bind_speaker(&tx, segments[0].id, person, false, 63001).unwrap();
        tx.commit().unwrap();
        let mut first_voice = vec![0.0; VOICE_DIM];
        first_voice[0] = 1.0;
        let mut other_voice = vec![0.0; VOICE_DIM];
        other_voice[1] = 1.0;
        for (index, vector) in [&first_voice, &other_voice, &first_voice]
            .into_iter()
            .enumerate()
        {
            let work = take_voice(&mut db, 63010 + index as i64).unwrap().unwrap();
            assert_eq!(work.segment_id, segments[index].id);
            publish_voice(&mut db, &work, vector, 63020 + index as i64).unwrap();
        }
        // Model an existing installation where a single matching utterance
        // incorrectly named the entire cloud group, then upgrade its policy.
        let tx = db.transaction().unwrap();
        let group: String = tx
            .query_row(
                "SELECT g.group_id FROM speaker_groups g JOIN precise_segments p
                 ON p.speaker_scope=g.speaker_scope AND p.speaker_label=g.speaker_label
                 WHERE p.id=?1",
                params![segments[0].id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        apply_group_binding(&tx, &group, None, &person.to_string(), "automatic", 63030).unwrap();
        tx.execute(
            "UPDATE identity_state SET speaker_policy_version=2 WHERE id=1",
            [],
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            events::event_detail(&db, event, 0, 10)
                .unwrap()
                .precise_segments[1]
                .person_id,
            Some(person)
        );

        init(&db).unwrap();
        while match_one_ready(&mut db, 63040).unwrap() {}
        let current = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert_eq!(current[0].person_id, Some(person));
        assert_eq!(current[1].person_id, None);
        assert_eq!(current[2].person_id, Some(person));
        assert_eq!(current[2].person_origin.as_deref(), Some("automatic"));

        db.execute(
            "UPDATE voice_samples SET revoked_ms=63041 WHERE person_id=?1 AND revoked_ms IS NULL",
            params![person.to_string()],
        )
        .unwrap();
        db.execute(
            "UPDATE identity_state SET generation=generation+1 WHERE id=1",
            [],
        )
        .unwrap();
        while match_one_ready(&mut db, 63042).unwrap() {}
        let without_sample = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert_eq!(without_sample[0].person_id, Some(person));
        assert_eq!(without_sample[1].person_id, None);
        assert_eq!(without_sample[2].person_id, None);
    }

    #[test]
    fn same_request_merge_requires_each_label_pair_not_a_transitive_chain() {
        let (mut db, event, segments) = speaker_merge_fixture(&[
            ("A", 0, 3000),
            ("A", 3000, 6000),
            ("B", 6000, 9000),
            ("B", 9000, 12000),
            ("C", 12000, 15000),
            ("C", 15000, 18000),
        ]);
        let angle = |degrees: f32| {
            let mut vector = vec![0.0; VOICE_DIM];
            vector[0] = degrees.to_radians().cos();
            vector[1] = degrees.to_radians().sin();
            vector
        };
        for (index, degrees) in [0.0, 0.0, 39.0, 39.0, 78.0, 78.0].iter().enumerate() {
            let work = take_voice(&mut db, 62000 + index as i64).unwrap().unwrap();
            assert_eq!(work.segment_id, segments[index].id);
            publish_voice(&mut db, &work, &angle(*degrees), 62010 + index as i64).unwrap();
        }
        let grouped = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert_eq!(grouped[0].speaker, grouped[2].speaker);
        assert_ne!(grouped[0].speaker, grouped[4].speaker);
    }

    #[test]
    fn overlapping_provider_labels_never_merge_on_solo_voice_similarity() {
        let (mut db, event, segments) = speaker_merge_fixture(&[
            ("A", 0, 3000),
            ("A", 3000, 6000),
            ("B", 6000, 9000),
            ("B", 9000, 12000),
            ("A", 12000, 15000),
            ("B", 12000, 15000),
        ]);
        let mut first = vec![0.0; VOICE_DIM];
        first[0] = 1.0;
        let mut second = vec![0.0; VOICE_DIM];
        second[0] = 0.8;
        second[1] = 0.6;
        for (index, vector) in [first.clone(), first, second.clone(), second]
            .iter()
            .enumerate()
        {
            let work = take_voice(&mut db, 63000 + index as i64).unwrap().unwrap();
            assert_eq!(work.segment_id, segments[index].id);
            publish_voice(&mut db, &work, vector, 63010 + index as i64).unwrap();
        }
        let grouped = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert_ne!(grouped[0].speaker, grouped[2].speaker);
    }

    #[test]
    fn existing_automatic_people_prevent_same_request_identity_takeover() {
        let (mut db, event, segments) = speaker_merge_fixture(&[
            ("A", 0, 3000),
            ("A", 3000, 6000),
            ("B", 6000, 9000),
            ("B", 9000, 12000),
        ]);
        let tx = db.transaction().unwrap();
        let alice = create_person(&tx, "甲", 64000).unwrap();
        let bob = create_person(&tx, "乙", 64001).unwrap();
        for (index, segment) in segments.iter().enumerate() {
            tx.execute(
                "INSERT INTO speaker_bindings
                 (id,original_segment_id,current_segment_id,person_id,source,
                  started_ms,ended_ms,origin,apply_scope,state,created_ms,updated_ms)
                 VALUES (?1,?2,?2,?3,'microphone',?4,?5,
                         'automatic','segment','active',64002,64002)",
                params![
                    Uuid::new_v4().to_string(),
                    segment.id.to_string(),
                    if index < 2 {
                        alice.to_string()
                    } else {
                        bob.to_string()
                    },
                    segment.started_ms,
                    segment.ended_ms
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let mut first = vec![0.0; VOICE_DIM];
        first[0] = 1.0;
        let mut second = vec![0.0; VOICE_DIM];
        second[0] = 0.8;
        second[1] = 0.6;
        for (index, vector) in [first.clone(), first, second.clone(), second]
            .iter()
            .enumerate()
        {
            let work = take_voice(&mut db, 64010 + index as i64).unwrap().unwrap();
            publish_voice(&mut db, &work, vector, 64020 + index as i64).unwrap();
        }
        let grouped = events::event_detail(&db, event, 0, 20)
            .unwrap()
            .precise_segments;
        assert_ne!(grouped[0].speaker, grouped[2].speaker);
        assert_eq!(grouped[0].person_id, Some(alice));
        assert_eq!(grouped[2].person_id, Some(bob));
    }

    #[test]
    fn search_pages_beyond_first_two_hundred_rows() {
        let db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        for index in 0..205 {
            db.execute(
                "INSERT INTO search_records(kind,item_id,source,started_ms,ended_ms,text)
                VALUES('note',?1,NULL,?2,?3,'page entry')",
                params![Uuid::new_v4().to_string(), index * 1000, index * 1000 + 500],
            )
            .unwrap();
        }
        let first = search(&db, None, 0, 300_000, None, 0, 200).unwrap();
        let next = search(&db, None, 0, 300_000, None, 200, 10).unwrap();
        assert_eq!((first.len(), next.len()), (200, 5));
        assert!(
            first
                .iter()
                .all(|item| !next.iter().any(|other| other.item_id == item.item_id))
        );
    }

    #[test]
    fn search_prefers_matching_precise_text_over_overlapping_coarse_text() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
          frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES ('block','session','microphone',1000,6000,0,240000,100,
          'media/microphone/block.opus','retain','complete',1)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO coarse_segments VALUES (?1,'block','session','microphone',
          1000,5000,'需要精确转录记录','test')",
            params![Uuid::new_v4().to_string()],
        )
        .unwrap();
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        events::create_event(&mut db, 1000, 6000, "讨论", EventSources::Microphone, 8000).unwrap();
        let task = events::take_task(&mut db, CloudProvider::Asr, 8001)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &task,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 3000,
                text: "精确转录完成".into(),
                speaker: Some("1".into()),
            }],
            8002,
        )
        .unwrap();
        events::transcribe_range(&mut db, 1000, 6000, EventSources::Microphone, 8003).unwrap();
        let overlapping = events::take_task(&mut db, CloudProvider::Asr, 8004)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &overlapping,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 3000,
                text: "精确转录完成".into(),
                speaker: Some("1".into()),
            }],
            8005,
        )
        .unwrap();
        let refined = search(&db, Some("精确转录"), 0, 10_000, None, 0, 10).unwrap();
        assert_eq!(refined.len(), 1);
        assert_eq!(refined[0].kind, "precise");
        let coarse_only = search(&db, Some("需要"), 0, 10_000, None, 0, 10).unwrap();
        assert_eq!(coarse_only.len(), 1);
        assert_eq!(coarse_only[0].kind, "coarse");
    }

    #[test]
    fn stale_text_revision_cannot_overwrite_newer_correction() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
            frames,bytes,media_path,retention_decision,analysis_state,speech_active)
            VALUES ('block','session','microphone',1000,6000,0,240000,100,
            'media/microphone/block.opus','retain','complete',1)",
            [],
        )
        .unwrap();
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event =
            events::create_event(&mut db, 1000, 6000, "讨论", EventSources::Microphone, 8000)
                .unwrap();
        let task = events::take_task(&mut db, CloudProvider::Asr, 8001)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &task,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 3000,
                text: "原稿".into(),
                speaker: Some("1".into()),
            }],
            8002,
        )
        .unwrap();
        let segment = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments[0]
            .id;
        let seen = object_revision(&db, "segment", segment).unwrap();
        let mut legacy = Sha256::new();
        revision_part(&mut legacy, "segment");
        legacy_segment_revision_part(&db, segment, &mut legacy).unwrap();
        assert_eq!(seen, finish_revision(legacy));
        let tx = db.transaction().unwrap();
        expect_revision(&tx, "segment", segment, seen).unwrap();
        tx.execute(
            "UPDATE named_events SET summary='旧摘要',understanding_json='{}' WHERE id=?1",
            [event.to_string()],
        )
        .unwrap();
        correct_text(&tx, segment, "先改", 8003).unwrap();
        assert!(
            tx.query_row::<Option<String>, _, _>(
                "SELECT understanding_json FROM named_events WHERE id=?1",
                [event.to_string()],
                |r| r.get(0)
            )
            .unwrap()
            .is_none()
        );
        assert!(
            events::event_detail(&tx, event, 0, 10)
                .unwrap()
                .summary
                .is_none()
        );
        tx.commit().unwrap();
        let mut legacy = Sha256::new();
        revision_part(&mut legacy, "segment");
        legacy_segment_revision_part(&db, segment, &mut legacy).unwrap();
        assert_eq!(
            object_revision(&db, "segment", segment).unwrap(),
            finish_revision(legacy)
        );
        let tx = db.transaction().unwrap();
        assert!(
            expect_revision(&tx, "segment", segment, seen)
                .unwrap_err()
                .starts_with("revision_conflict:")
        );
        tx.rollback().unwrap();
        assert_eq!(
            events::event_detail(&db, event, 0, 10)
                .unwrap()
                .precise_segments[0]
                .text,
            "先改"
        );
    }

    #[test]
    fn correction_survives_exact_retranscription_but_not_ambiguous_split() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        db.execute(
            "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
          frames,bytes,media_path,retention_decision,analysis_state,speech_active)
          VALUES ('block','session','microphone',1000,6000,0,240000,100,
          'media/microphone/block.opus','retain','complete',1)",
            [],
        )
        .unwrap();
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let event =
            events::create_event(&mut db, 1000, 6000, "讨论", EventSources::Microphone, 8000)
                .unwrap();
        let first = events::take_task(&mut db, CloudProvider::Asr, 8000)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &first,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 3000,
                text: "识别错字".into(),
                speaker: Some("1".into()),
            }],
            8001,
        )
        .unwrap();
        let segment = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments
            .remove(0);
        let tx = db.transaction().unwrap();
        correct_text(&tx, segment.id, "正确文字", 8002).unwrap();
        tx.commit().unwrap();
        let corrected = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments
            .remove(0);
        assert_eq!(corrected.text, "正确文字");
        assert_eq!(corrected.original_text.as_deref(), Some("识别错字"));
        assert_eq!(
            search(&db, Some("正确文字"), 0, 10_000, None, 0, 10)
                .unwrap()
                .len(),
            1
        );
        assert!(
            search(&db, Some("识别错字"), 0, 10_000, None, 0, 10)
                .unwrap()
                .is_empty()
        );

        let manual_range =
            events::transcribe_range(&mut db, 1000, 6000, EventSources::Microphone, 8002).unwrap();
        let parallel = events::take_task(&mut db, CloudProvider::Asr, 8002)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &parallel,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 3000,
                text: "独立范围识别".into(),
                speaker: Some("1".into()),
            }],
            8002,
        )
        .unwrap();
        assert_eq!(
            events::event_detail(&db, event, 0, 10)
                .unwrap()
                .precise_segments[0]
                .text,
            "正确文字"
        );
        assert_eq!(
            events::range_detail(&db, 1000, 6000, EventSources::Microphone, 0, 10)
                .unwrap()
                .precise_segments
                .iter()
                .find(|item| item.text == "独立范围识别")
                .unwrap()
                .corrected,
            false
        );
        assert_eq!(
            search(&db, Some("正确文字"), 0, 10_000, None, 0, 10)
                .unwrap()
                .len(),
            1
        );
        let _: Uuid = manual_range;

        events::edit_event(&mut db, event, 1, 1000, 7000, "讨论", None, 8003).unwrap();
        {
            let tx = db.transaction().unwrap();
            events::retranscribe_event_in(&tx, event, 2, 8003).unwrap();
            tx.commit().unwrap();
        }
        let next = events::take_task(&mut db, CloudProvider::Asr, 8004)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &next,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 3000,
                text: "新版原识别".into(),
                speaker: Some("1".into()),
            }],
            8005,
        )
        .unwrap();
        let carried = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments
            .remove(0);
        assert_eq!(carried.text, "正确文字");
        assert_eq!(carried.original_text.as_deref(), Some("新版原识别"));
        assert_eq!(
            search(&db, Some("正确文字"), 0, 10_000, None, 0, 10)
                .unwrap()
                .len(),
            1
        );

        events::edit_event(&mut db, event, 2, 1000, 8000, "讨论", None, 8006).unwrap();
        {
            let tx = db.transaction().unwrap();
            events::retranscribe_event_in(&tx, event, 3, 8006).unwrap();
            tx.commit().unwrap();
        }
        let split = events::take_task(&mut db, CloudProvider::Asr, 8007)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &split,
            &[
                AsrUtterance {
                    words: None,
                    start_ms: 500,
                    end_ms: 1750,
                    text: "前半句".into(),
                    speaker: Some("1".into()),
                },
                AsrUtterance {
                    words: None,
                    start_ms: 1750,
                    end_ms: 3000,
                    text: "后半句".into(),
                    speaker: Some("1".into()),
                },
            ],
            8008,
        )
        .unwrap();
        let current = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert!(current.iter().all(|item| !item.corrected));
        let pending: i64 = db
            .query_row(
                "SELECT count(*) FROM text_corrections WHERE state='pending_review'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
        let reviews = review_items(&db).unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].kind, "text");
        let tx = db.transaction().unwrap();
        resolve_review(&tx, reviews[0].id, Some(current[0].id), 8009).unwrap();
        tx.commit().unwrap();
        assert!(review_items(&db).unwrap().is_empty());
        let resolved = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert_eq!(resolved[0].text, "正确文字");
        assert_eq!(resolved[1].text, "后半句");

        events::edit_event(&mut db, event, 3, 1000, 9000, "讨论", None, 8010).unwrap();
        {
            let tx = db.transaction().unwrap();
            events::retranscribe_event_in(&tx, event, 4, 8010).unwrap();
            tx.commit().unwrap();
        }
        let revised = events::take_task(&mut db, CloudProvider::Asr, 8011)
            .unwrap()
            .unwrap();
        events::publish_asr(
            &mut db,
            &revised,
            &[AsrUtterance {
                words: None,
                start_ms: 500,
                end_ms: 1750,
                text: "再一次错误".into(),
                speaker: None,
            }],
            8012,
        )
        .unwrap();
        let latest = events::event_detail(&db, event, 0, 10)
            .unwrap()
            .precise_segments;
        assert_eq!(latest[0].text, "正确文字");
        assert!(review_items(&db).unwrap().is_empty());

        let (left, _right) =
            events::split_event(&mut db, event, 4, 3500, "上半场", "下半场", 8013).unwrap();
        assert!(
            events::take_task(&mut db, CloudProvider::Asr, 8014)
                .unwrap()
                .is_none()
        );
        let reused = events::event_detail(&db, left, 0, 10)
            .unwrap()
            .precise_segments;
        assert_eq!(reused[0].id, latest[0].id);
        assert_eq!(reused[0].text, "正确文字");
        assert!(review_items(&db).unwrap().is_empty());
    }

    #[test]
    fn confirmed_person_enrolls_and_matches_another_event_but_rejection_wins() {
        let mut db = crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db;
        for (id, start, end) in [("first", 1000, 6000), ("second", 11000, 16000)] {
            db.execute(
                "INSERT INTO audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,
              frames,bytes,media_path,retention_decision,analysis_state,speech_active)
              VALUES (?1,'session','microphone',?2,?3,0,240000,100,
              'media/microphone/block.opus','retain','complete',1)",
                params![id, start, end],
            )
            .unwrap();
        }
        events::set_configured(&db, CloudProvider::Asr, "bigmodel").unwrap();
        let first =
            events::create_event(&mut db, 1000, 6000, "首次", EventSources::Microphone, 8000)
                .unwrap();
        let second = events::create_event(
            &mut db,
            11000,
            16000,
            "再次",
            EventSources::Microphone,
            8001,
        )
        .unwrap();
        for when in [8002, 8003] {
            let task = events::take_task(&mut db, CloudProvider::Asr, when)
                .unwrap()
                .unwrap();
            events::publish_asr(
                &mut db,
                &task,
                &[AsrUtterance {
                    words: None,
                    start_ms: 500,
                    end_ms: 4000,
                    text: "一段清楚的话".into(),
                    speaker: Some("1".into()),
                }],
                when + 1,
            )
            .unwrap();
        }
        let first_segment = events::event_detail(&db, first, 0, 10)
            .unwrap()
            .precise_segments[0]
            .id;
        let second_segment = events::event_detail(&db, second, 0, 10)
            .unwrap()
            .precise_segments[0]
            .id;
        let tx = db.transaction().unwrap();
        let person = create_person(&tx, "张三", 9000).unwrap();
        let unseen_binding_revision = object_revision(&tx, "person", person).unwrap();
        bind_speaker(&tx, first_segment, person, true, 9001).unwrap();
        assert!(expect_revision(&tx, "person", person, unseen_binding_revision).is_err());
        tx.commit().unwrap();
        assert_eq!(people(&db).unwrap()[0].sample_count, 0);
        let mut vector = vec![0.0; VOICE_DIM];
        vector[0] = 1.0;
        let first_work = take_voice(&mut db, 9002).unwrap().unwrap();
        assert_eq!(first_work.segment_id, first_segment);
        publish_voice(&mut db, &first_work, &vector, 9003).unwrap();
        assert_eq!(people(&db).unwrap()[0].sample_count, 1);
        let samples = voice_samples(&db, person).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].person_id, person);
        let second_work = take_voice(&mut db, 9004).unwrap().unwrap();
        assert_eq!(second_work.segment_id, second_segment);
        publish_voice(&mut db, &second_work, &vector, 9005).unwrap();
        while match_one_ready(&mut db, 9006).unwrap() {}
        let matched = events::event_detail(&db, second, 0, 10)
            .unwrap()
            .precise_segments
            .remove(0);
        assert_eq!(matched.person_id, Some(person));
        assert_eq!(matched.person_origin.as_deref(), Some("automatic"));
        let tx = db.transaction().unwrap();
        let replacement = create_person(&tx, "李四", 9007).unwrap();
        bind_speaker(&tx, first_segment, replacement, false, 9007).unwrap();
        tx.commit().unwrap();
        enrollment::drain_enrollment(&mut db, 9007).unwrap();
        let listed = people(&db).unwrap();
        assert_eq!(
            listed
                .iter()
                .find(|item| item.id == person)
                .unwrap()
                .sample_count,
            0
        );
        assert_eq!(
            listed
                .iter()
                .find(|item| item.id == replacement)
                .unwrap()
                .sample_count,
            1
        );
        while match_one_ready(&mut db, 9008).unwrap() {}
        assert_eq!(
            events::event_detail(&db, second, 0, 10)
                .unwrap()
                .precise_segments[0]
                .person_id,
            Some(replacement)
        );
        let tx = db.transaction().unwrap();
        unbind_speaker(&tx, second_segment, false, 9009).unwrap();
        tx.execute("UPDATE identity_state SET generation=generation+1", [])
            .unwrap();
        tx.commit().unwrap();
        while match_one_ready(&mut db, 9008).unwrap() {}
        assert!(
            events::event_detail(&db, second, 0, 10)
                .unwrap()
                .precise_segments[0]
                .person_id
                .is_none()
        );
        let tx = db.transaction().unwrap();
        delete_person(&tx, person).unwrap();
        delete_person(&tx, replacement).unwrap();
        tx.commit().unwrap();
        assert!(people(&db).unwrap().is_empty());
        assert!(
            events::event_detail(&db, first, 0, 10)
                .unwrap()
                .precise_segments[0]
                .person_id
                .is_none()
        );
    }
}
