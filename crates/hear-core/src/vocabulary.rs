use hear_protocol::vocabulary::{Action, Entry};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;
type Result<T> = std::result::Result<T, String>;

pub fn init(db: &Connection) -> Result<()> {
    crate::storage::migrate(
        db,
        "vocabulary_v1",
        r#"
      CREATE TABLE vocabulary_entries(id TEXT PRIMARY KEY,name TEXT NOT NULL,kind TEXT NOT NULL,aliases TEXT NOT NULL,enabled INTEGER NOT NULL);
      CREATE TABLE vocabulary_settings(id INTEGER PRIMARY KEY CHECK(id=1),llm INTEGER NOT NULL,local_asr INTEGER NOT NULL,cloud_asr INTEGER NOT NULL);
      INSERT INTO vocabulary_settings VALUES(1,1,1,1);
      CREATE TABLE entities(id TEXT PRIMARY KEY,name TEXT NOT NULL,kind TEXT NOT NULL);
      CREATE TABLE asr_vocabulary(task_id TEXT PRIMARY KEY REFERENCES cloud_tasks(id),context TEXT NOT NULL);
      CREATE TRIGGER vocabulary_entity AFTER INSERT ON vocabulary_entries BEGIN
        INSERT INTO entities VALUES(new.id,new.name,new.kind) ON CONFLICT(id) DO UPDATE SET name=new.name,kind=new.kind;
      END;
      CREATE TRIGGER vocabulary_entity_update AFTER UPDATE ON vocabulary_entries BEGIN
        INSERT INTO entities VALUES(new.id,new.name,new.kind) ON CONFLICT(id) DO UPDATE SET name=new.name,kind=new.kind;
      END;
      CREATE TRIGGER understanding_entities AFTER UPDATE OF understanding_json ON named_events WHEN json_valid(new.understanding_json) BEGIN
        INSERT OR IGNORE INTO entities SELECT json_extract(value,'$.entity_id'),json_extract(value,'$.name'),json_extract(value,'$.type') FROM json_each(new.understanding_json,'$.mentioned_objects') WHERE json_valid(value) AND json_type(value,'$.entity_id')='text';
      END;
      CREATE TRIGGER understanding_entities_insert AFTER INSERT ON named_events WHEN json_valid(new.understanding_json) BEGIN
        INSERT OR IGNORE INTO entities SELECT json_extract(value,'$.entity_id'),json_extract(value,'$.name'),json_extract(value,'$.type') FROM json_each(new.understanding_json,'$.mentioned_objects') WHERE json_valid(value) AND json_type(value,'$.entity_id')='text';
      END;
    "#,
    )
}
pub fn list(db: &Connection) -> Result<Value> {
    let mut stmt = db
        .prepare("SELECT id,name,kind,aliases,enabled FROM vocabulary_entries ORDER BY name,id")
        .map_err(|e| e.to_string())?;
    let entries = stmt
        .query_map([], |r| {
            Ok(Entry {
                entity_id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                aliases: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                enabled: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let settings = db.query_row("SELECT llm,local_asr,cloud_asr FROM vocabulary_settings WHERE id=1",[],|r| Ok(json!({"llm":r.get::<_,bool>(0)?,"local_asr":r.get::<_,bool>(1)?,"cloud_asr":r.get::<_,bool>(2)?}))).map_err(|e|e.to_string())?;
    Ok(
        json!({"entries":entries,"settings":settings,"capabilities":{"local_asr":{"engine":"X-ASR transducer","mode":"modified_beam_search","hotwords":true,"constraint":"Only terms representable by the model token inventory are biased; unsupported terms remain LLM vocabulary."},"cloud_asr":{"engine":"Doubao file recognition bigmodel","hotwords":true,"field":"request.corpus.context"}}}),
    )
}
fn key(s: &str) -> String {
    s.trim().to_lowercase()
}
pub fn apply(db: &Connection, action: &Action) -> Result<Value> {
    match action {
        Action::List => {}
        Action::Remove { id } => {
            db.execute(
                "DELETE FROM vocabulary_entries WHERE id=?1",
                [id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        }
        Action::Configure {
            llm,
            local_asr,
            cloud_asr,
        } => {
            db.execute("UPDATE vocabulary_settings SET llm=coalesce(?1,llm),local_asr=coalesce(?2,local_asr),cloud_asr=coalesce(?3,cloud_asr) WHERE id=1",params![llm,local_asr,cloud_asr]).map_err(|e|e.to_string())?;
        }
        Action::Upsert {
            id,
            name,
            kind,
            aliases,
            enabled,
        } => {
            if name.trim().is_empty()
                || name.chars().count() > 100
                || kind.trim().is_empty()
                || kind.chars().count() > 50
                || aliases.len() > 20
                || std::iter::once(name).chain(aliases).any(|s| {
                    s.trim().is_empty()
                        || s.chars().count() > 100
                        || s.chars().any(char::is_control)
                })
            {
                return Err("invalid vocabulary entry: name <=100 characters, kind <=50, at most 20 nonempty aliases, no control characters".into());
            }
            let previous = list(db)?;
            let rows: Vec<Entry> =
                serde_json::from_value(previous["entries"].clone()).map_err(|e| e.to_string())?;
            let id = id
                .map(|id| id.to_string())
                .or_else(|| {
                    rows.iter()
                        .find(|e| key(&e.name) == key(name))
                        .map(|e| e.entity_id.clone())
                })
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            if rows.len() >= 100 && !rows.iter().any(|e| e.entity_id == id) {
                return Err(
                    "vocabulary is limited to 100 entries for bounded model context".into(),
                );
            }
            if *enabled
                && rows
                    .iter()
                    .filter(|e| e.enabled && e.entity_id != id)
                    .any(|e| {
                        std::iter::once(&e.name).chain(&e.aliases).any(|old| {
                            std::iter::once(name)
                                .chain(aliases)
                                .any(|new| key(old) == key(new))
                        })
                    })
            {
                return Err(
                    "vocabulary name or alias is already assigned to another active entity".into(),
                );
            }
            let aliases: Vec<_> = aliases
                .iter()
                .map(|s| s.trim())
                .filter(|s| key(s) != key(name))
                .collect();
            let entry_bytes = json!({"id":id,"name":name,"kind":kind,"aliases":aliases})
                .to_string()
                .len();
            let existing_bytes: usize = rows
                .iter()
                .filter(|e| e.entity_id != id)
                .map(|e| serde_json::to_vec(e).unwrap().len())
                .sum();
            if existing_bytes + entry_bytes > 64 * 1024 {
                return Err("vocabulary exceeds the 64 KiB context budget".into());
            }
            db.execute("INSERT INTO vocabulary_entries VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,kind=excluded.kind,aliases=excluded.aliases,enabled=excluded.enabled",params![id,name.trim(),kind.trim(),json!(aliases).to_string(),enabled]).map_err(|e|e.to_string())?;
        }
    }
    list(db)
}
pub fn llm_context(db: &Connection) -> Result<Value> {
    let v = list(db)?;
    if v["settings"]["llm"] != true {
        return Ok(json!([]));
    }
    Ok(json!(entries(&v["entries"])))
}
fn entries(value: &Value) -> Vec<Entry> {
    serde_json::from_value::<Vec<Entry>>(value.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.enabled)
        .collect()
}
pub fn entity(name: &str, kind: &str, vocabulary: &Value) -> (String, String, String) {
    let mut catalog = serde_json::from_value::<Vec<Entry>>(vocabulary.clone()).unwrap_or_default();
    catalog.sort_by_key(|e| !e.enabled);
    if let Some(e) = catalog
        .into_iter()
        .find(|e| key(&e.name) == key(name) || e.aliases.iter().any(|s| key(s) == key(name)))
    {
        return (e.entity_id, e.name, e.kind);
    }
    (
        format!(
            "entity:{:x}",
            Sha256::digest(format!("{}\0{}", key(name), key(kind)).as_bytes())
        ),
        name.to_owned(),
        kind.to_owned(),
    )
}
/// Longest match, including canonical names, avoids replacing 梦空间 inside 盗梦空间 twice.
pub fn normalize_text(text: &str, vocabulary: &Value) -> String {
    let mut terms = Vec::new();
    for e in entries(vocabulary) {
        terms.push((e.name.clone(), e.name.clone()));
        for alias in e.aliases {
            terms.push((alias, e.name.clone()));
        }
    }
    terms.sort_by_key(|(a, _)| std::cmp::Reverse(a.len()));
    let mut out = String::new();
    let mut pos = 0;
    while pos < text.len() {
        if let Some((alias, name)) = terms.iter().find(|(a, _)| text[pos..].starts_with(a)) {
            out.push_str(name);
            pos += alias.len();
        } else {
            let c = text[pos..].chars().next().unwrap();
            out.push(c);
            pos += c.len_utf8();
        }
    }
    out
}
pub fn normalize_generated(value: &mut Value, vocabulary: &Value) {
    match value {
        Value::String(s) => *s = normalize_text(s, vocabulary),
        Value::Array(a) => {
            for v in a {
                normalize_generated(v, vocabulary);
            }
        }
        Value::Object(o) => {
            for (k, v) in o {
                if matches!(
                    k.as_str(),
                    "title"
                        | "summary"
                        | "text"
                        | "topics"
                        | "name"
                        | "context"
                        | "events"
                        | "understanding"
                        | "key_points"
                        | "mentioned_objects"
                        | "decisions"
                        | "todos"
                        | "summary_update"
                        | "append"
                        | "replace"
                        | "sentences"
                ) {
                    normalize_generated(v, vocabulary);
                }
            }
        }
        _ => {}
    }
}
pub fn submission_context(db: &Connection, task: Uuid) -> Result<Value> {
    let existing: Option<String> = db
        .query_row(
            "SELECT context FROM asr_vocabulary WHERE task_id=?1",
            [task.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(v) = existing {
        return serde_json::from_str(&v).map_err(|e| e.to_string());
    }
    let v = list(db)?;
    let context = if v["settings"]["cloud_asr"] == true {
        json!({"hotwords":entries(&v["entries"]).into_iter().map(|e|json!({"word":e.name})).collect::<Vec<_>>()})
    } else {
        json!({})
    };
    db.execute(
        "INSERT INTO asr_vocabulary VALUES(?1,?2)",
        params![task.to_string(), context.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        crate::Store::new(Connection::open_in_memory().unwrap())
            .unwrap()
            .db
    }
    #[test]
    fn aliases_are_stable_bounded_and_non_recursive() {
        let db = db();
        let v = apply(
            &db,
            &Action::Upsert {
                id: None,
                name: "看山".into(),
                kind: "product".into(),
                aliases: vec!["开山".into()],
                enabled: true,
            },
        )
        .unwrap();
        let id = v["entries"][0]["entity_id"].clone();
        apply(
            &db,
            &Action::Upsert {
                id: None,
                name: "盗梦空间".into(),
                kind: "project".into(),
                aliases: vec!["梦空间".into()],
                enabled: true,
            },
        )
        .unwrap();
        let context = llm_context(&db).unwrap();
        assert_eq!(
            normalize_text("开山和盗梦空间，梦空间", &context),
            "看山和盗梦空间，盗梦空间"
        );
        assert_eq!(entity("开山", "product", &context).0, id.as_str().unwrap());
        assert_eq!(entity("看山", "产品", &context).0, id.as_str().unwrap());
        assert!(
            apply(
                &db,
                &Action::Upsert {
                    id: None,
                    name: "other".into(),
                    kind: "term".into(),
                    aliases: vec!["开山".into()],
                    enabled: true
                }
            )
            .is_err()
        );
        let v = apply(
            &db,
            &Action::Upsert {
                id: None,
                name: "看山".into(),
                kind: "product".into(),
                aliases: vec!["开山".into()],
                enabled: true,
            },
        )
        .unwrap();
        assert!(
            v["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["entity_id"] == id)
        );
        apply(
            &db,
            &Action::Configure {
                llm: Some(false),
                local_asr: Some(false),
                cloud_asr: Some(false),
            },
        )
        .unwrap();
        assert_eq!(llm_context(&db).unwrap(), json!([]));
        assert_eq!(
            entity("全新项目", "project", &json!([])).0,
            entity("全新项目", "project", &json!([])).0
        );
    }
    #[test]
    fn submitted_cloud_vocabulary_does_not_change_on_retry() {
        let db = db();
        let id = Uuid::new_v4();
        db.execute("INSERT INTO cloud_tasks(id,provider,target_kind,target_id,target_version,model_id,processor_version,started_ms,ended_ms,source,state,created_ms,updated_ms) VALUES(?1,'asr','range','x','1','x','x',0,1000,'microphone','submitted',0,0)",[id.to_string()]).unwrap();
        apply(
            &db,
            &Action::Upsert {
                id: None,
                name: "看山".into(),
                kind: "product".into(),
                aliases: vec![],
                enabled: true,
            },
        )
        .unwrap();
        let first = submission_context(&db, id).unwrap();
        assert_eq!(first["hotwords"][0]["word"], "看山");
        apply(
            &db,
            &Action::Configure {
                llm: None,
                local_asr: None,
                cloud_asr: Some(false),
            },
        )
        .unwrap();
        assert_eq!(submission_context(&db, id).unwrap(), first);
    }
}
