use rusqlite::Connection;

pub fn init(db: &Connection) -> Result<(), String> {
    // Existing search_records constrains its kinds. Keep event metadata in a
    // separate rebuildable index, with transaction-local invalidation triggers.
    crate::storage::migrate(
        db,
        "agent_context_event_search_v1",
        r#"
        CREATE TABLE context_event_search (
          rowid INTEGER PRIMARY KEY,event_id TEXT NOT NULL,field TEXT NOT NULL,
          started_ms INTEGER NOT NULL,ended_ms INTEGER NOT NULL,source_mask INTEGER NOT NULL,text TEXT NOT NULL,
          UNIQUE(event_id,field));
        CREATE INDEX context_event_search_time ON context_event_search(started_ms,event_id);
        CREATE VIRTUAL TABLE context_event_fts USING fts5(text,content='context_event_search',content_rowid='rowid',tokenize='trigram');
        CREATE TRIGGER context_event_index_insert AFTER INSERT ON context_event_search BEGIN
          INSERT INTO context_event_fts(rowid,text) VALUES(new.rowid,new.text);
        END;
        CREATE TRIGGER context_event_index_delete AFTER DELETE ON context_event_search BEGIN
          INSERT INTO context_event_fts(context_event_fts,rowid,text) VALUES('delete',old.rowid,old.text);
        END;
        CREATE VIEW context_event_fields AS
          SELECT id event_id,'title' field,started_ms,ended_ms,source_mask,title text FROM named_events WHERE deleted_ms IS NULL
          UNION ALL SELECT id,'summary',started_ms,ended_ms,source_mask,summary FROM named_events WHERE deleted_ms IS NULL AND summary IS NOT NULL
          UNION ALL SELECT e.id,'topic:'||j.key,e.started_ms,e.ended_ms,e.source_mask,cast(j.value AS TEXT) FROM named_events e,json_each(CASE WHEN json_valid(e.understanding_json) THEN e.understanding_json ELSE '{}' END,'$.topics') j WHERE e.deleted_ms IS NULL
          UNION ALL SELECT e.id,'key_point:'||j.key,e.started_ms,e.ended_ms,e.source_mask,json_extract(j.value,'$.text') FROM named_events e,json_each(CASE WHEN json_valid(e.understanding_json) THEN e.understanding_json ELSE '{}' END,'$.key_points') j WHERE e.deleted_ms IS NULL AND json_valid(j.value)
          UNION ALL SELECT e.id,'mentioned_object:'||j.key,e.started_ms,e.ended_ms,e.source_mask,coalesce(json_extract(j.value,'$.name'),'')||char(10)||coalesce(json_extract(j.value,'$.context'),'') FROM named_events e,json_each(CASE WHEN json_valid(e.understanding_json) THEN e.understanding_json ELSE '{}' END,'$.mentioned_objects') j WHERE e.deleted_ms IS NULL AND json_valid(j.value);
        CREATE TRIGGER context_event_insert AFTER INSERT ON named_events BEGIN
          INSERT INTO context_event_search(event_id,field,started_ms,ended_ms,source_mask,text) SELECT * FROM context_event_fields WHERE event_id=new.id AND text IS NOT NULL;
        END;
        CREATE TRIGGER context_event_update AFTER UPDATE OF title,summary,understanding_json,started_ms,ended_ms,source_mask,deleted_ms ON named_events BEGIN
          DELETE FROM context_event_search WHERE event_id=old.id;
          INSERT INTO context_event_search(event_id,field,started_ms,ended_ms,source_mask,text) SELECT * FROM context_event_fields WHERE event_id=new.id AND text IS NOT NULL;
        END;
        CREATE TRIGGER context_event_delete AFTER DELETE ON named_events BEGIN
          DELETE FROM context_event_search WHERE event_id=old.id;
        END;
        INSERT INTO context_event_search(event_id,field,started_ms,ended_ms,source_mask,text) SELECT * FROM context_event_fields WHERE text IS NOT NULL;
    "#,
    )?;
    // A view, not another copy of transcript text. Reuse the legacy search's
    // publication predicate and its correction-maintained text projection.
    db.execute_batch(&format!(r#"
        DROP VIEW IF EXISTS context_preferred;
        DROP VIEW IF EXISTS context_visible;
        CREATE VIEW context_visible AS
        SELECT sr.rowid,sr.kind,sr.item_id,sr.source,sr.started_ms,sr.ended_ms,sr.text,
          identity.person_id,identity.person_name,identity.person_origin,t.target_kind,
          EXISTS(SELECT 1 FROM text_corrections c WHERE c.current_segment_id=p.id AND c.state='active') corrected
        FROM search_records sr
        LEFT JOIN precise_segments p ON sr.kind='precise' AND p.id=sr.item_id
        LEFT JOIN transcript_tasks t ON t.id=p.task_id AND p.started_ms>=t.started_ms AND p.ended_ms<=t.ended_ms
        LEFT JOIN precise_identity_projection identity ON identity.segment_id=p.id
        WHERE sr.kind!='precise' OR ({})
        UNION ALL SELECT NULL,'mark',id,NULL,anchor_ms,anchor_ms+1,'',NULL,NULL,NULL,NULL,0 FROM moments WHERE kind='mark'
        UNION ALL SELECT NULL,'ocr',m.id,NULL,m.anchor_ms,m.anchor_ms+1,'',NULL,NULL,NULL,NULL,0
        FROM moments m WHERE m.kind='screenshot' AND NOT EXISTS(
          SELECT 1 FROM search_records sr WHERE sr.kind='ocr' AND sr.item_id=m.id);
        CREATE VIEW context_preferred AS SELECT hit.* FROM context_visible hit
        WHERE hit.kind!='precise' OR NOT EXISTS(SELECT 1 FROM context_visible twin
          WHERE twin.kind='precise' AND twin.source=hit.source
            AND twin.started_ms=hit.started_ms AND twin.ended_ms=hit.ended_ms AND twin.text=hit.text
            AND twin.person_id IS hit.person_id AND twin.person_origin IS hit.person_origin
            AND twin.corrected=hit.corrected AND twin.rowid!=hit.rowid
            AND (twin.target_kind='event' AND hit.target_kind!='event'
              OR twin.target_kind=hit.target_kind AND twin.rowid<hit.rowid));
    "#,crate::knowledge::PRECISE_VISIBLE)).map_err(|e|e.to_string())
}
