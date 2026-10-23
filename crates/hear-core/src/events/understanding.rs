//! Request-local evidence indices never escape into persisted event snapshots.
use super::{Evidence, source_bit};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EvidenceRef {
    segment_id: String,
    source: String,
    start_ms: i64,
    end_ms: i64,
}

impl From<&Evidence> for EvidenceRef {
    fn from(item: &Evidence) -> Self {
        Self {
            segment_id: item.id.clone(),
            source: item.source.clone(),
            start_ms: item.start,
            end_ms: item.end,
        }
    }
}

pub(super) fn index(catalog: &mut Vec<EvidenceRef>, reference: EvidenceRef) -> usize {
    if let Some(index) = catalog.iter().position(|item| item == &reference) {
        index
    } else {
        catalog.push(reference);
        catalog.len() - 1
    }
}

#[derive(Serialize, Deserialize)]
struct Understanding {
    schema_version: u32,
    topics: Vec<String>,
    key_points: Vec<Point>,
    mentioned_objects: Vec<Object>,
}

#[derive(Serialize, Deserialize)]
struct Point {
    text: String,
    evidence_refs: Vec<EvidenceRef>,
}

#[derive(Serialize, Deserialize)]
struct Object {
    name: String,
    r#type: String,
    context: String,
    entity_id: Option<String>,
    evidence_refs: Vec<EvidenceRef>,
}

/// Re-number inherited anchors in this request. Missing evidence invalidates
/// the snapshot rather than quietly presenting a partial previous result.
pub(super) fn prepare_context(
    db: &Connection,
    context: &mut Value,
    catalog: &mut Vec<EvidenceRef>,
) -> Result<(), String> {
    for key in ["previous_events", "linked_events"] {
        for event in context[key].as_array_mut().unwrap() {
            let stored = serde_json::from_value::<Understanding>(event["understanding"].clone())
                .ok()
                .filter(|value| value.schema_version == 1);
            let Some(stored) = stored else {
                event["understanding"] = Value::Null;
                continue;
            };
            let mut valid = true;
            for reference in stored
                .key_points
                .iter()
                .flat_map(|p| &p.evidence_refs)
                .chain(
                    stored
                        .mentioned_objects
                        .iter()
                        .flat_map(|o| &o.evidence_refs),
                )
            {
                let exists: bool = db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM coarse_segments WHERE id=?1 AND source=?2
                     AND started_ms=?3 AND ended_ms=?4 AND length(trim(text))>0)",
                        params![
                            reference.segment_id,
                            reference.source,
                            reference.start_ms,
                            reference.end_ms
                        ],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                valid &= exists;
            }
            if !valid {
                event["understanding"] = Value::Null;
                continue;
            }
            let mut value = serde_json::to_value(stored).map_err(|e| e.to_string())?;
            value.as_object_mut().unwrap().remove("schema_version");
            for key in ["key_points", "mentioned_objects"] {
                for item in value[key].as_array_mut().unwrap() {
                    let refs: Vec<EvidenceRef> = serde_json::from_value(
                        item.as_object_mut()
                            .unwrap()
                            .remove("evidence_refs")
                            .unwrap(),
                    )
                    .map_err(|e| e.to_string())?;
                    item["evidence_indices"] = json!(
                        refs.into_iter()
                            .map(|r| index(catalog, r))
                            .collect::<Vec<_>>()
                    );
                }
            }
            event["understanding"] = value;
        }
    }
    Ok(())
}

fn text(value: &Value) -> Option<String> {
    super::fit_summary(value.as_str())
}

/// Model content is optional metadata. Invalid structure cannot discard an
/// otherwise legal event; invalid anchored claims are removed as whole items.
pub(super) fn normalize(
    output: &Value,
    input: &Value,
    prior: Option<&Value>,
    start: i64,
    end: i64,
    mask: i64,
) -> Option<String> {
    let topics = output.get("topics")?.as_array()?;
    let points = output.get("key_points")?.as_array()?;
    let objects = output.get("mentioned_objects")?.as_array()?;
    let catalog: Vec<EvidenceRef> = serde_json::from_value(input["evidence"].clone()).ok()?;
    let mut allowed: HashSet<usize> = input["observations"]
        .as_array()?
        .iter()
        .filter_map(|v| {
            v["evidence_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
        })
        .collect();
    if let Some(prior) = prior {
        for key in ["key_points", "mentioned_objects"] {
            if let Some(items) = prior["understanding"][key].as_array() {
                for item in items {
                    if let Some(indices) = item["evidence_indices"].as_array() {
                        allowed.extend(
                            indices
                                .iter()
                                .filter_map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok())),
                        );
                    }
                }
            }
        }
    }
    let references = |value: &Value| -> Option<Vec<EvidenceRef>> {
        let indices = value["evidence_indices"].as_array()?;
        if indices.is_empty() {
            return None;
        }
        let mut refs = Vec::new();
        for value in indices {
            let i = usize::try_from(value.as_u64()?).ok()?;
            let reference = catalog.get(i)?;
            if !allowed.contains(&i)
                || source_bit(&reference.source) & mask == 0
                || reference.start_ms >= end
                || reference.end_ms <= start
            {
                return None;
            }
            if !refs.contains(reference) {
                refs.push(reference.clone());
            }
        }
        // Canonical ordering makes equivalent repeated responses a no-op.
        refs.sort_by(|a, b| a.segment_id.cmp(&b.segment_id));
        Some(refs)
    };
    let mut result = Understanding {
        schema_version: 1,
        topics: Vec::new(),
        key_points: Vec::new(),
        mentioned_objects: Vec::new(),
    };
    for topic in topics {
        let topic = text(topic)?;
        if !result.topics.contains(&topic) {
            result.topics.push(topic);
        }
    }
    let mut seen = HashSet::new();
    for point in points {
        let (Some(text), Some(evidence_refs)) = (text(&point["text"]), references(point)) else {
            continue;
        };
        let point = Point {
            text,
            evidence_refs,
        };
        if seen.insert(serde_json::to_string(&point).ok()?) {
            result.key_points.push(point);
        }
    }
    seen.clear();
    for object in objects {
        let (Some(name), Some(r#type), Some(context), Some(evidence_refs)) = (
            text(&object["name"]),
            text(&object["type"]),
            text(&object["context"]),
            references(object),
        ) else {
            continue;
        };
        let object = Object {
            name,
            r#type,
            context,
            entity_id: None,
            evidence_refs,
        };
        if seen.insert(serde_json::to_string(&object).ok()?) {
            result.mentioned_objects.push(object);
        }
    }
    serde_json::to_string(&result).ok()
}
