//! Agent consumption projections. All reads use Core's short SQLite snapshots.
mod index;
#[cfg(test)]
mod tests;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use hear_protocol::{
    EventSources,
    context::{self as api, Query, TimeRange},
};
pub use index::init;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;
const BUDGET: usize = 24 * 1024;
const MAX_RANGE: i64 = 31 * 86_400_000;
type Result<T> = std::result::Result<T, String>;
fn sql<T>(r: rusqlite::Result<T>) -> Result<T> {
    r.map_err(|e| e.to_string())
}
fn hash(v: &impl Serialize) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).unwrap()))
}
fn encode(v: &impl Serialize) -> String {
    B64.encode(serde_json::to_vec(v).unwrap())
}
fn decode<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_slice(&B64.decode(s).map_err(|_| "invalid encoded locator")?)
        .map_err(|_| "invalid encoded locator".into())
}
fn mask(s: Option<EventSources>) -> i64 {
    match s.unwrap_or(EventSources::Both) {
        EventSources::Microphone => 1,
        EventSources::System => 2,
        EventSources::Both => 3,
    }
}
fn source_mask(s: &str) -> i64 {
    if s == "microphone" { 1 } else { 2 }
}
fn excerpt(s: &str, limit: usize) -> String {
    s.chars().take(limit).collect()
}

#[derive(Default, Serialize, Deserialize)]
struct Cursor {
    query: String,
    expires: i64,
    range: Option<TimeRange>,
    time: i64,
    key: String,
    fragment: usize,
    version: Option<String>,
    #[serde(default)]
    fragment_version: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Ref {
    kind: String,
    id: String,
    version: String,
}
struct Item {
    time: i64,
    key: String,
    value: Value,
}
struct Page {
    cursor: Cursor,
    limit: usize,
    descending: bool,
}
impl Page {
    fn new(q: &Query, token: Option<&str>, limit: usize, descending: bool) -> Result<Self> {
        let mut identity = serde_json::to_value(q).unwrap();
        identity["arguments"]
            .as_object_mut()
            .unwrap()
            .remove("cursor");
        identity["arguments"]
            .as_object_mut()
            .unwrap()
            .remove("limit");
        let fingerprint = hash(&identity);
        let cursor = if let Some(t) = token {
            let c: Cursor = decode(t)?;
            if c.query != fingerprint
                || c.expires < api::now_ms()
                || c.expires > api::now_ms() + 3_600_000
                || c.fragment > 100_000_000
            {
                return Err("cursor_stale: restart this query".into());
            }
            c
        } else {
            Cursor {
                query: fingerprint,
                expires: api::now_ms() + 1_800_000,
                time: if descending { i64::MAX } else { -1 },
                ..Default::default()
            }
        };
        Ok(Self {
            cursor,
            limit,
            descending,
        })
    }
    fn range(
        &mut self,
        input: Option<&api::TimeInput>,
        today: bool,
        bounded: bool,
    ) -> Result<TimeRange> {
        let range = match &self.cursor.range {
            Some(r) => r.clone(),
            None => api::resolve_time(input, today, api::now_ms())?,
        };
        if range.from_ms < 0
            || range.to_ms <= range.from_ms
            || (bounded && range.to_ms - range.from_ms > MAX_RANGE)
        {
            return Err("time range must be at most 31 days; use search for all history".into());
        }
        self.cursor.range = Some(range.clone());
        Ok(range)
    }
    fn accepts(&self, time: i64, key: &str) -> bool {
        let at = (time, key);
        let previous = (self.cursor.time, self.cursor.key.as_str());
        if self.cursor.fragment > 0 && at == previous {
            return true;
        }
        if self.descending {
            at < previous
        } else {
            at > previous
        }
    }
    fn finish(
        mut self,
        id: Uuid,
        mut data: Value,
        mut items: Vec<Item>,
        quality: Value,
    ) -> Result<Value> {
        if self.cursor.fragment > 0
            && !items
                .iter()
                .any(|i| i.time == self.cursor.time && i.key == self.cursor.key)
        {
            return Err("cursor_stale: the continued record is no longer available".into());
        }
        items.sort_by(|a, b| (a.time, &a.key).cmp(&(b.time, &b.key)));
        if self.descending {
            items.reverse();
        }
        items.retain(|i| self.accepts(i.time, &i.key));
        let mut output = Vec::new();
        let mut bytes = 0;
        let mut more = false;
        for item in items {
            if output.len() >= self.limit {
                more = true;
                break;
            }
            let mut value = item.value;
            let offset = if (item.time, &item.key) == (self.cursor.time, &self.cursor.key) {
                self.cursor.fragment
            } else {
                0
            };
            if offset > 0
                && self
                    .cursor
                    .fragment_version
                    .as_ref()
                    .is_some_and(|v| v != &hash(&value))
            {
                return Err("cursor_stale: source text changed".into());
            }
            let fragment_version = hash(&value);
            let mut next_fragment = 0;
            if let Some(text) = value["text"].as_str() {
                let chars: Vec<char> = text.chars().collect();
                if offset > chars.len() {
                    return Err("cursor_stale: text changed".into());
                }
                let end = (offset + 2000).min(chars.len());
                let part: String = chars[offset..end].iter().collect();
                if end < chars.len() {
                    next_fragment = end;
                }
                value["text"] = json!(part);
                value["fragment"] = json!({"start":offset,"end":end,"has_more":next_fragment>0});
            }
            let size = serde_json::to_vec(&value).unwrap().len();
            if size > BUDGET - 8192 {
                return Err("record_too_large: metadata exceeds context page budget".into());
            }
            if bytes + size > BUDGET - 8192 {
                more = true;
                break;
            }
            bytes += size;
            output.push(value);
            self.cursor.time = item.time;
            self.cursor.key = item.key;
            self.cursor.fragment = next_fragment;
            self.cursor.fragment_version = (next_fragment > 0).then_some(fragment_version);
            if next_fragment > 0 {
                more = true;
                break;
            }
        }
        data["items"] = json!(output);
        let mut response = api::success(id, data);
        response["page"] = json!({"has_more":more,"next_cursor":if more{Some(encode(&self.cursor))}else{None},"consistency":"live","text_chunk_characters":2000});
        response["quality"] = quality;
        if let Some(r) = &self.cursor.range {
            response["query"] = json!({"from":api::timestamp(r.from_ms),"to":api::timestamp(r.to_ms),"timezone":r.timezone});
        }
        if serde_json::to_vec(&response).unwrap().len() > BUDGET {
            return Err("record_too_large: narrow the range".into());
        }
        Ok(response)
    }
}

pub fn query(db: &Connection, q: &Query, id: Uuid) -> Value {
    let result = (|| {
        q.validate()?;
        match q {
            Query::Status(_) => Ok(api::success(
                id,
                json!({"online":true,"api_version":api::API_VERSION,"timezone":api::timezone(),"server_time":api::timestamp(api::now_ms()),"capabilities":api::TOOLS.iter().map(|(_,n,_)|*n).collect::<Vec<_>>(),"processing":crate::events::queue_states(db)?}),
            )),
            Query::List(a) => list(db, q, a, id),
            Query::Search(a) => search(db, q, a, id),
            Query::Event(a) => event(db, q, a, id),
            Query::Read(a) => read(db, q, a, id),
            Query::People(a) => people(db, q, a, id),
        }
    })();
    match result {
        Ok(v) => v,
        Err(e) => {
            let code = if e.starts_with("cursor_stale") {
                "cursor_stale"
            } else if e.starts_with("ref_unavailable") {
                "ref_unavailable"
            } else if e.contains("interrupted") {
                "query_timeout"
            } else if e.contains("capacity busy") {
                "read_busy"
            } else if e.starts_with("record_too_large") {
                "record_too_large"
            } else {
                "invalid_argument"
            };
            api::error(id, code, e)
        }
    }
}

fn coverage(db: &Connection, r: &TimeRange, m: i64) -> Result<Value> {
    let mut issues = Vec::new();
    let reason =
        crate::knowledge::search_empty_reason_for_sources(db, r.from_ms, r.to_ms, None, m)?;
    if reason != "no_match" {
        issues.push(json!({"code":reason}));
    }
    // Report gaps separately from text availability. Bound metadata like content.
    let mut gaps_stmt=sql(db.prepare("SELECT source,max(started_ms,?1),min(coalesce(ended_ms,?4),?2),reason FROM media_gaps WHERE started_ms<?2 AND coalesce(ended_ms,?4)>?1 AND (source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0) ORDER BY started_ms,source LIMIT 13"))?;
    let relevant=sql(gaps_stmt.query_map(params![r.from_ms,r.to_ms,m,api::now_ms()],|row|Ok(json!({"source":row.get::<_,String>(0)?,"from":api::timestamp(row.get(1)?),"to":api::timestamp(row.get(2)?),"reason":row.get::<_,String>(3)?}))))?.collect::<rusqlite::Result<Vec<_>>>();
    let relevant = sql(relevant)?;
    let pending:i64=sql(db.query_row("SELECT count(*) FROM coarse_jobs WHERE state IN ('pending','processing') AND started_ms<?2 AND ended_ms>?1 AND (source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0)",params![r.from_ms,r.to_ms,m],|row|row.get(0)))?;
    if pending > 0 {
        issues.push(json!({"code":"coarse_pending","count":pending}));
    }
    let counts:(i64,i64,i64)=sql(db.query_row("SELECT (SELECT count(*) FROM context_visible WHERE kind='coarse' AND started_ms<?2 AND ended_ms>?1 AND (source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0)),(SELECT count(*) FROM context_visible WHERE kind='precise' AND started_ms<?2 AND ended_ms>?1 AND (source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0)),(SELECT count(*) FROM audio_blocks WHERE started_ms<?2 AND ended_ms>?1 AND media_path IS NOT NULL AND (source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0))",params![r.from_ms,r.to_ms,m],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))))?;
    Ok(
        json!({"coverage_state":"observed_only","issues":issues,"gaps":relevant.iter().take(12).collect::<Vec<_>>(),"gaps_truncated":relevant.len()>12,"coarse_segments":counts.0,"precise_segments":counts.1,"retained_audio_blocks":counts.2,"note":"Coverage describes recorded material, not all real-world activity. Text and retained audio are independent."}),
    )
}

fn list(db: &Connection, q: &Query, a: &api::ListArgs, id: Uuid) -> Result<Value> {
    let mut page = Page::new(
        q,
        a.cursor.as_deref(),
        a.limit.unwrap_or(20) as usize,
        false,
    )?;
    let range = page.range(a.time.as_ref(), true, true)?;
    let m = mask(a.sources);
    let mut items = Vec::new();
    let mut start = range.from_ms;
    let mut seen = std::collections::HashSet::new();
    while start < range.to_ms {
        let end = (start + 86_400_000).min(range.to_ms);
        for t in crate::events::timeline(db, start, end)? {
            if mask(Some(t.sources)) & m == 0 {
                continue;
            }
            let (key, from, to) = if let Some(event) = t.id {
                if !seen.insert(event) {
                    continue;
                }
                let (from, to) = sql(db.query_row(
                    "SELECT started_ms,ended_ms FROM named_events WHERE id=?1",
                    [event.to_string()],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                ))?;
                (format!("event:{event}"), from, to)
            } else {
                (
                    format!(
                        "range:{}:{}:{}",
                        t.started_ms,
                        t.ended_ms,
                        mask(Some(t.sources))
                    ),
                    t.started_ms,
                    t.ended_ms,
                )
            };
            let time = from.max(range.from_ms);
            if !page.accepts(time, &key) {
                continue;
            }
            items.push(Item{time,key,value:json!({"kind":if t.id.is_some(){"event"}else{"unorganized_range"},"event_id":t.id,"from":api::timestamp(from),"to":api::timestamp(to),"matched_from":api::timestamp(from.max(range.from_ms)),"matched_to":api::timestamp(to.min(range.to_ms)),"sources":t.sources,"title":t.title,"summary_excerpt":t.summary.as_deref().map(|s|excerpt(s,240)),"summary_truncated":t.summary.as_ref().is_some_and(|s|s.chars().count()>240),"processing_state":t.processing_state,"selector":{"time":{"from":api::timestamp(from.max(range.from_ms)),"to":api::timestamp(to.min(range.to_ms))},"sources":t.sources}})});
        }
        start = end;
    }
    // Moments without an event remain discoverable, including marks with no text.
    let mut stmt=sql(db.prepare("SELECT kind,item_id,started_ms FROM context_visible c WHERE kind IN ('note','ocr','mark') AND started_ms>=?1 AND started_ms<?2 AND NOT EXISTS(SELECT 1 FROM named_events e WHERE e.deleted_ms IS NULL AND e.started_ms<=c.started_ms AND e.ended_ms>c.started_ms) AND (started_ms>?3 OR started_ms=?3 AND kind||':'||item_id>?4) ORDER BY started_ms,kind||':'||item_id LIMIT ?5"))?;
    let rows = sql(stmt.query_map(
        params![
            range.from_ms,
            range.to_ms,
            page.cursor.time,
            page.cursor.key,
            page.limit as i64 + 1
        ],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        },
    ))?;
    for row in rows {
        let (kind, item_id, time) = sql(row)?;
        if let Some(mut v) = record(db, &kind, &item_id)? {
            v.as_object_mut().unwrap().remove("text");
            items.push(Item {
                time,
                key: format!("{kind}:{item_id}"),
                value: v,
            });
        }
    }
    page.finish(id, json!({}), items, coverage(db, &range, m)?)
}

#[derive(Clone)]
struct Event {
    start: i64,
    end: i64,
    title: String,
    revision: i64,
    mask: i64,
    summary: Option<String>,
    understanding: Option<String>,
}
fn load_event(db: &Connection, id: &str) -> Result<Event> {
    sql(db.query_row("SELECT started_ms,ended_ms,title,revision,source_mask,summary,understanding_json FROM named_events WHERE id=?1 AND deleted_ms IS NULL",[id],|r|Ok(Event{start:r.get(0)?,end:r.get(1)?,title:r.get(2)?,revision:r.get(3)?,mask:r.get(4)?,summary:r.get(5)?,understanding:r.get(6)?})).optional())?.ok_or_else(||"ref_unavailable: event no longer available".into())
}
fn understanding(db: &Connection, e: &Event) -> Result<Option<Value>> {
    let Some(raw) = &e.understanding else {
        return Ok(None);
    };
    let Ok(mut v) = serde_json::from_str::<Value>(raw) else {
        return Ok(None);
    };
    if v["schema_version"] != 1
        || ["topics", "key_points", "mentioned_objects"]
            .iter()
            .any(|k| !v[k].is_array())
    {
        return Ok(None);
    }
    for section in ["key_points", "mentioned_objects"] {
        for item in v[section].as_array_mut().unwrap() {
            let Some(refs) = item["evidence_refs"].as_array() else {
                return Ok(None);
            };
            if refs.is_empty() {
                return Ok(None);
            }
            let mut resolved = Vec::new();
            for r in refs {
                let Some(id) = r["segment_id"].as_str() else {
                    return Ok(None);
                };
                let Some(record) = record(db, "coarse", id)? else {
                    return Ok(None);
                };
                if record["started_ms"] != r["start_ms"]
                    || record["ended_ms"] != r["end_ms"]
                    || record["source"] != r["source"]
                    || r["start_ms"].as_i64().is_none_or(|n| n >= e.end)
                    || r["end_ms"].as_i64().is_none_or(|n| n <= e.start)
                    || r["source"]
                        .as_str()
                        .is_none_or(|s| source_mask(s) & e.mask == 0)
                {
                    return Ok(None);
                }
                resolved.push(record["ref"].clone());
            }
            item["evidence_refs"] = json!(resolved);
            item["content_kind"] = json!("derived");
        }
    }
    Ok(Some(v))
}
fn event_version(db: &Connection, id: &str, e: &Event) -> Result<String> {
    // Include content and identity, not just the editable event-boundary revision.
    let mut hasher = Sha256::new();
    hasher.update(
        serde_json::to_vec(&json!([
            e.start,
            e.end,
            e.title,
            e.revision,
            e.mask,
            e.summary,
            e.understanding
        ]))
        .unwrap(),
    );
    let mut s=sql(db.prepare("SELECT kind,item_id,text,person_id,person_name,person_origin FROM context_visible WHERE started_ms<?2 AND ended_ms>?1 AND (source IS NULL OR source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0) ORDER BY kind,item_id"))?;
    let rows = sql(s.query_map(params![e.start, e.end, e.mask], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    }))?;
    for row in rows {
        hasher.update(serde_json::to_vec(&sql(row)?).unwrap());
    }
    hasher.update(id.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}
fn event(db: &Connection, q: &Query, a: &api::EventArgs, id: Uuid) -> Result<Value> {
    let eid = a.event_id.to_string();
    let e = load_event(db, &eid)?;
    let mut page = Page::new(
        q,
        a.cursor.as_deref(),
        a.limit.unwrap_or(20) as usize,
        false,
    )?;
    let version = event_version(db, &eid, &e)?;
    if page.cursor.version.as_ref().is_some_and(|v| v != &version) {
        return Err("cursor_stale: event content changed".into());
    }
    page.cursor.version = Some(version.clone());
    let understanding = understanding(db, &e)?;
    let section = a.section.as_deref().unwrap_or("overview");
    let mut items = Vec::new();
    let mut data = json!({"event_id":eid,"title":e.title,"from":api::timestamp(e.start),"to":api::timestamp(e.end),"content_version":version,"section":section,"understanding_available":understanding.is_some(),"understanding_source":"coarse_transcript"});
    match section {
        "overview" => {
            data["summary_excerpt"] = json!(e.summary.as_deref().map(|s| excerpt(s, 1000)));
            data["summary_truncated"] =
                json!(e.summary.as_ref().is_some_and(|s| s.chars().count() > 1000));
            data["topics_excerpt"] = json!(
                understanding
                    .as_ref()
                    .and_then(|v| v["topics"].as_array())
                    .map(|a| a
                        .iter()
                        .take(5)
                        .map(|x| excerpt(x.as_str().unwrap_or(""), 120))
                        .collect::<Vec<_>>())
            );
            data["sections"] = json!([
                "summary",
                "topics",
                "key_points",
                "mentioned_objects",
                "moments"
            ]);
        }
        "summary" => {
            if let Some(text) = e.summary.clone() {
                items.push(Item {
                    time: 0,
                    key: "summary".into(),
                    value: json!({"kind":"event_summary","text":text,"content_kind":"derived"}),
                });
            }
        }
        "moments" => {
            let mut stmt=sql(db.prepare("SELECT kind,item_id,started_ms FROM context_visible WHERE kind IN ('note','ocr','mark') AND started_ms>=?1 AND started_ms<?2 AND (started_ms>?3 OR started_ms=?3 AND kind||':'||item_id>=?4) ORDER BY started_ms,kind||':'||item_id LIMIT ?5"))?;
            for row in sql(stmt.query_map(
                params![
                    e.start,
                    e.end,
                    page.cursor.time,
                    page.cursor.key,
                    page.limit as i64 + 2
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            ))? {
                let (k, i, t) = sql(row)?;
                if let Some(v) = record(db, &k, &i)? {
                    items.push(Item {
                        time: t,
                        key: format!("{k}:{i}"),
                        value: v,
                    });
                }
            }
        }
        _ => {
            if let Some(u) = understanding {
                for (i, v) in u[section].as_array().unwrap().iter().enumerate() {
                    let value = if section == "topics" {
                        json!({"text":v,"content_kind":"derived"})
                    } else {
                        let mut v = v.clone();
                        if section == "mentioned_objects" {
                            v["text"] = v["context"].take();
                        }
                        v
                    };
                    items.push(Item {
                        time: i as i64,
                        key: format!("{section}:{i}"),
                        value,
                    });
                }
            }
        }
    }
    page.finish(
        id,
        data,
        items,
        coverage(
            db,
            &TimeRange {
                from_ms: e.start,
                to_ms: e.end,
                timezone: api::timezone(),
            },
            e.mask,
        )?,
    )
}

fn record(db: &Connection, kind: &str, id: &str) -> Result<Option<Value>> {
    let row:Option<(Option<String>,i64,i64,String,Option<String>,Option<String>,Option<String>,bool)>=sql(db.query_row("SELECT source,started_ms,ended_ms,text,person_id,person_name,person_origin,corrected FROM context_visible WHERE kind=?1 AND item_id=?2",params![kind,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional())?;
    let Some((source, start, end, text, person_id, person_name, origin, corrected)) = row else {
        return Ok(None);
    };
    let screenshot = if kind == "ocr" {
        sql(db.query_row("SELECT ocr_state,image_summary,image_transcript,ocr_text FROM moments WHERE id=?1",[id],|r| {
            let summary: Option<String> = r.get(1)?;
            let transcript: Option<String> = r.get(2)?;
            let ocr: Option<String> = r.get(3)?;
            Ok(json!({"ocr_state":r.get::<_,Option<String>>(0)?,"has_summary":summary.is_some(),"text_origin":if transcript.is_some(){"image_understanding"}else{"ocr"},"summary_characters":summary.as_ref().map(|s|s.chars().count()).unwrap_or(0),"transcript_start":if summary.is_some() || transcript.is_some(){summary.as_ref().map(|s|s.chars().count()).unwrap_or(0)+1}else{0},"text_available":summary.as_ref().is_some_and(|s|!s.is_empty()) || transcript.as_ref().or(ocr.as_ref()).is_some_and(|s|!s.is_empty())}))
        }).optional())?
    } else {
        None
    };
    let version = hash(&(
        kind,
        id,
        &source,
        start,
        end,
        &text,
        &person_id,
        &person_name,
        &origin,
        corrected,
        &screenshot,
    ));
    let reference = encode(&Ref {
        kind: kind.into(),
        id: id.into(),
        version: version.clone(),
    });
    let event_ids = event_ids(db, start, end, source.as_deref())?;
    let audio = if let Some(source) = &source {
        sql(db.query_row("SELECT EXISTS(SELECT 1 FROM audio_blocks WHERE source=?1 AND started_ms<?3 AND ended_ms>?2 AND media_path IS NOT NULL)",params![source,start,end],|r|r.get::<_,bool>(0)))?
    } else {
        false
    };
    Ok(Some(
        json!({"ref":reference,"kind":kind,"item_id":id,"source":source,"started_ms":start,"ended_ms":end,"from":api::timestamp(start),"to":api::timestamp(end),"text":text,"content_version":version,"person_id":person_id,"person_name":person_name,"identity_origin":origin,"corrected":corrected,"audio_available":audio,"text_available":!text.is_empty(),"screenshot":screenshot,"event_ids":event_ids,"citation_label":format!("{} · {}",api::timestamp(start),kind),"content_kind":if kind=="ocr"{"screenshot_text"}else{"source"}}),
    ))
}
fn event_ids(db: &Connection, start: i64, end: i64, source: Option<&str>) -> Result<Vec<String>> {
    let mut stmt=sql(db.prepare("SELECT id FROM named_events WHERE deleted_ms IS NULL AND started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0 ORDER BY started_ms,id LIMIT 101"))?;
    let ids = sql(stmt.query_map(
        params![start, end, source.map(source_mask).unwrap_or(3)],
        |r| r.get(0),
    ))?
    .collect::<rusqlite::Result<Vec<String>>>();
    let ids = sql(ids)?;
    if ids.len() > 100 {
        return Err("record_too_large: too many event associations".into());
    }
    Ok(ids)
}

fn read(db: &Connection, q: &Query, a: &api::ReadArgs, id: Uuid) -> Result<Value> {
    let mut page = Page::new(
        q,
        a.cursor.as_deref(),
        a.limit.unwrap_or(50) as usize,
        false,
    )?;
    let mut items = Vec::new();
    if let Some(refs) = &a.selector.refs {
        for (i, r) in refs.iter().enumerate() {
            let reference: Ref = decode(r).map_err(|_| "invalid evidence ref")?;
            let value = match record(db, &reference.kind, &reference.id)? {
                Some(v) if v["content_version"] == reference.version => v,
                Some(_) => json!({"ref":r,"available":false,"reason":"stale"}),
                None => json!({"ref":r,"available":false,"reason":"unavailable"}),
            };
            items.push(Item {
                time: i as i64,
                key: i.to_string(),
                value,
            });
        }
        return page.finish(
            id,
            json!({}),
            items,
            json!({"coverage_state":"selected_refs"}),
        );
    }
    let mut m = mask(a.selector.sources);
    let range = if let Some(eid) = a.selector.event_id {
        let e = load_event(db, &eid.to_string())?;
        m &= e.mask;
        let version = event_version(db, &eid.to_string(), &e)?;
        if page.cursor.version.as_ref().is_some_and(|v| v != &version) {
            return Err("cursor_stale: event content changed".into());
        }
        page.cursor.version = Some(version);
        let r = TimeRange {
            from_ms: e.start,
            to_ms: e.end,
            timezone: api::timezone(),
        };
        page.cursor.range = Some(r.clone());
        r
    } else {
        page.range(a.selector.time.as_ref(), false, true)?
    };
    let mode = a.transcript.as_deref().unwrap_or("best_available");
    let mut stmt=sql(db.prepare("SELECT kind,item_id,started_ms FROM context_preferred c WHERE started_ms<?2 AND ended_ms>?1 AND (source IS NULL OR source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0) AND ((kind IN ('note','ocr','mark') AND ?4) OR (kind IN ('coarse','precise') AND (?5='best_available' OR kind=?5))) AND (?5!='best_available' OR kind!='coarse' OR NOT EXISTS(SELECT 1 FROM context_visible p WHERE p.kind='precise' AND p.source=c.source AND p.started_ms<=c.started_ms AND p.ended_ms>=c.ended_ms)) AND (started_ms>?6 OR started_ms=?6 AND kind||':'||item_id>=?7) ORDER BY started_ms,kind||':'||item_id LIMIT ?8"))?;
    for row in sql(stmt.query_map(
        params![
            range.from_ms,
            range.to_ms,
            m,
            a.include_moments.unwrap_or(true),
            mode,
            page.cursor.time,
            page.cursor.key,
            page.limit as i64 + 2
        ],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        },
    ))? {
        let (kind, item_id, time) = sql(row)?;
        if let Some(mut value) = record(db, &kind, &item_id)? {
            if kind == "coarse" && mode == "best_available" {
                value["overlaps_precise"]=json!(sql(db.query_row("SELECT EXISTS(SELECT 1 FROM context_visible p WHERE p.kind='precise' AND p.source=?3 AND p.started_ms<?2 AND p.ended_ms>?1)",params![value["started_ms"].as_i64(),value["ended_ms"].as_i64(),value["source"].as_str()],|r|r.get::<_,bool>(0)))?);
            }
            items.push(Item {
                time,
                key: format!("{kind}:{item_id}"),
                value,
            });
        }
    }
    page.finish(
        id,
        json!({"transcript":mode}),
        items,
        coverage(db, &range, m)?,
    )
}

fn search(db: &Connection, q: &Query, a: &api::SearchArgs, id: Uuid) -> Result<Value> {
    let mut page = Page::new(q, a.cursor.as_deref(), a.limit.unwrap_or(20) as usize, true)?;
    let range = page.range(a.time.as_ref(), false, false)?;
    let m = mask(a.sources);
    let term = a.query.as_deref().unwrap_or("").trim();
    let fts = term.chars().count() >= 3;
    let text = if fts {
        format!("\"{}\"", term.replace('"', "\"\""))
    } else {
        term.to_owned()
    };
    let condition = if term.is_empty() {
        "1"
    } else if fts {
        "rowid IN (SELECT rowid FROM search_fts WHERE search_fts MATCH ?4)"
    } else {
        "instr(lower(text),lower(?4))>0"
    };
    let derived_condition = if term.is_empty() {
        "1"
    } else if fts {
        "rowid IN (SELECT rowid FROM context_event_fts WHERE context_event_fts MATCH ?4)"
    } else {
        "instr(lower(text),lower(?4))>0"
    };
    let statement=format!("WITH hits AS (
        SELECT kind,item_id,started_ms,kind||':'||item_id AS key,text FROM context_preferred WHERE kind!='mark' AND started_ms<?2 AND ended_ms>?1 AND (source IS NULL OR source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0) AND (?5 IS NULL OR person_id=?5) AND {condition}
        UNION ALL SELECT 'event_field',event_id,started_ms,'event_field:'||event_id||':'||field,text FROM context_event_search WHERE started_ms<?2 AND ended_ms>?1 AND source_mask&?3!=0 AND {derived_condition} AND (?5 IS NULL OR EXISTS(SELECT 1 FROM context_visible v WHERE v.kind='precise' AND v.person_id=?5 AND v.started_ms<context_event_search.ended_ms AND v.ended_ms>context_event_search.started_ms AND (v.source='microphone' AND context_event_search.source_mask&?3&1!=0 OR v.source='system' AND context_event_search.source_mask&?3&2!=0))))
        SELECT kind,item_id,started_ms,key,text FROM hits WHERE started_ms<?6 OR started_ms=?6 AND key<?7 ORDER BY started_ms DESC,key DESC LIMIT ?8");
    let mut stmt = sql(db.prepare(&statement))?;
    let mut items = Vec::new();
    for row in sql(stmt.query_map(
        params![
            range.from_ms,
            range.to_ms,
            m,
            text,
            a.person_id.map(|p| p.to_string()),
            page.cursor.time,
            page.cursor.key,
            page.limit as i64 + 1
        ],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        },
    ))? {
        let (kind, item_id, time, key, text) = sql(row)?;
        let mut value = if kind == "event_field" {
            let e = load_event(db, &item_id)?;
            let field = key.splitn(3, ':').nth(2).unwrap_or("");
            let mut hit = json!({"event_id":item_id,"event_ids":[item_id],"title":e.title,"matched_field":field,"content_kind":if field=="title" {"event_metadata"} else {"derived"},"from":api::timestamp(e.start),"to":api::timestamp(e.end)});
            if let Some((section, index)) = field.split_once(':') {
                let u = understanding(db, &e)?;
                let section = match section {
                    "topic" => "topics",
                    "key_point" => "key_points",
                    _ => "mentioned_objects",
                };
                let index: usize = index.parse().unwrap_or(usize::MAX);
                // Stale understanding remains a discoverable locator, never quoted as evidence.
                if let Some(value) = u.as_ref().and_then(|v| v[section].get(index)) {
                    hit["evidence_refs"] = value.get("evidence_refs").cloned().unwrap_or(json!([]));
                } else {
                    hit["available"] = json!(false);
                    hit["reason"] = json!("understanding_unavailable");
                }
            }
            hit
        } else {
            let Some(v) = record(db, &kind, &item_id)? else {
                continue;
            };
            v
        };
        value.as_object_mut().unwrap().remove("text");
        value["excerpt"] = if value["available"] == false {
            Value::Null
        } else {
            json!(matching_excerpt(&text, term))
        };
        value["excerpt_truncated"] = json!(text.chars().count() > 300);
        items.push(Item { time, key, value });
    }
    let empty_reason = if items.is_empty() {
        Some(crate::knowledge::search_empty_reason_for_sources(
            db,
            range.from_ms,
            range.to_ms,
            a.person_id,
            m,
        )?)
    } else {
        None
    };
    page.finish(
        id,
        json!({"match_mode":"literal_phrase","empty_reason":empty_reason}),
        items,
        coverage(db, &range, m)?,
    )
}
fn matching_excerpt(text: &str, term: &str) -> String {
    let lower = text.to_lowercase();
    let index = lower.find(&term.to_lowercase()).unwrap_or(0);
    // Lowercasing can change byte lengths; use character positions for safe previews.
    let before = lower[..index].chars().count().saturating_sub(60);
    text.chars().skip(before).take(300).collect()
}
fn people(db: &Connection, q: &Query, a: &api::PeopleArgs, id: Uuid) -> Result<Value> {
    let page = Page::new(
        q,
        a.cursor.as_deref(),
        a.limit.unwrap_or(20) as usize,
        false,
    )?;
    let mut stmt=sql(db.prepare("SELECT id,display_name,0 FROM people WHERE (?1 IS NULL OR instr(lower(display_name),lower(?1))>0) AND id>?2 ORDER BY id LIMIT ?3"))?;
    let mut items = Vec::new();
    for row in sql(stmt.query_map(
        params![a.name, page.cursor.key, page.limit as i64 + 1],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        },
    ))? {
        let (pid, name, _revision) = sql(row)?;
        let revision = crate::knowledge::object_revision(
            db,
            "person",
            Uuid::parse_str(&pid).map_err(|e| e.to_string())?,
        )?;
        items.push(Item {
            time: 0,
            key: pid.clone(),
            value: json!({"person_id":pid,"display_name":name,"revision":revision}),
        });
    }
    page.finish(
        id,
        json!({}),
        items,
        json!({"identity_scope":"known_speakers"}),
    )
}

#[cfg(test)]
pub(crate) fn remove_index_for_legacy_fixture(db: &Connection) {
    // A legacy fixture predates this projection as well as the columns it drops.
    db.execute_batch(
        "DROP TRIGGER context_event_insert; DROP TRIGGER context_event_update;
        DROP TRIGGER context_event_delete; DROP VIEW context_event_fields;
        DROP TABLE context_event_search; DROP TABLE context_event_fts;
        DELETE FROM core_migrations WHERE name='agent_context_event_search_v1';",
    )
    .unwrap();
}
