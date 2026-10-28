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
                for (item_index, item) in value[key].as_array_mut().unwrap().iter_mut().enumerate()
                {
                    item["item_index"] = json!(item_index);
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
    let prior_value = prior.map(|p| &p["understanding"]);
    let valid = |v: &Value| {
        ["topics", "key_points", "mentioned_objects"]
            .iter()
            .all(|key| v[*key].is_array())
    };
    let prior_value = prior_value.filter(|v| valid(v));
    let empty = json!({"topics":[],"key_points":[],"mentioned_objects":[]});
    // A missing/invalid patch is not a deletion of an existing valid snapshot.
    let output = if valid(output) {
        output
    } else if prior_value.is_some() {
        &empty
    } else {
        return None;
    };
    let old = prior_value.unwrap_or(&empty);
    let catalog: Vec<EvidenceRef> = serde_json::from_value(input["evidence"].clone()).ok()?;
    let current: HashSet<usize> = input["observations"]
        .as_array()?
        .iter()
        .filter_map(|v| {
            v["evidence_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
        })
        .collect();
    let mut allowed = current.clone();
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
    let point = |value: &Value| {
        Some(Point {
            text: text(&value["text"])?,
            evidence_refs: references(value)?,
        })
    };
    let object = |value: &Value| {
        let (entity_id, name, kind) = crate::vocabulary::entity(
            &text(&value["name"])?,
            &text(&value["type"])?,
            &input["vocabulary"],
        );
        Some(Object {
            name,
            r#type: kind,
            context: text(&value["context"])?,
            entity_id: Some(entity_id),
            evidence_refs: references(value)?,
        })
    };
    let mut result = Understanding {
        schema_version: 1,
        topics: Vec::new(),
        key_points: Vec::new(),
        mentioned_objects: Vec::new(),
    };
    for topic in old["topics"]
        .as_array()?
        .iter()
        .chain(output["topics"].as_array()?)
    {
        if let Some(topic) = text(topic) {
            if !result.topics.contains(&topic) {
                result.topics.push(topic);
            }
        }
    }
    for point in update_items(
        old["key_points"].as_array()?,
        output["key_points"].as_array()?,
        &current,
        point,
    ) {
        if let Some(existing) = result.key_points.iter_mut().find(|p| p.text == point.text) {
            merge_references(&mut existing.evidence_refs, point.evidence_refs);
        } else {
            result.key_points.push(point);
        }
    }
    for object in update_items(
        old["mentioned_objects"].as_array()?,
        output["mentioned_objects"].as_array()?,
        &current,
        object,
    ) {
        if let Some(existing) = result.mentioned_objects.iter_mut().find(|o| {
            o.name == object.name && o.r#type == object.r#type && o.context == object.context
        }) {
            merge_references(&mut existing.evidence_refs, object.evidence_refs);
        } else {
            result.mentioned_objects.push(object);
        }
    }
    serde_json::to_string(&result).ok()
}

/// Indices address the sent target snapshot, never an array being mutated.
/// Revisions/actual inputs are checked by the publication transaction first.
fn update_items<T>(
    old: &[Value],
    updates: &[Value],
    current: &HashSet<usize>,
    decode: impl Fn(&Value) -> Option<T>,
) -> Vec<T> {
    let mut retained: Vec<Option<T>> = old.iter().map(&decode).collect();
    let mut added = Vec::new();
    for update in updates {
        let Some(item) = decode(update) else {
            continue;
        };
        let replaces = match update.get("replaces") {
            None => Some(Vec::new()),
            Some(v) => v.as_array().and_then(|items| {
                items
                    .iter()
                    .map(|i| i.as_u64().and_then(|n| usize::try_from(n).ok()))
                    .collect::<Option<Vec<_>>>()
            }),
        };
        let Some(replaces) = replaces else {
            continue;
        };
        if !replaces.is_empty() {
            // Changed/retracted claims need evidence in this window. Old anchors
            // alone cannot authorize deleting earlier facts. Decode also checks
            // every anchor's target scope, source and ownership.
            let fresh = update["evidence_indices"]
                .as_array()
                .is_some_and(|indices| {
                    indices.iter().any(|i| {
                        i.as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .is_some_and(|n| current.contains(&n))
                    })
                });
            if !fresh
                || replaces
                    .iter()
                    .any(|&i| retained.get(i).is_none_or(Option::is_none))
            {
                continue;
            }
            for i in replaces {
                retained[i] = None;
            }
        }
        added.push(item);
    }
    retained.into_iter().flatten().chain(added).collect()
}

fn merge_references(existing: &mut Vec<EvidenceRef>, additional: Vec<EvidenceRef>) {
    for reference in additional {
        if !existing.contains(&reference) {
            existing.push(reference);
        }
    }
    existing.sort_by(|a, b| a.segment_id.cmp(&b.segment_id));
}
