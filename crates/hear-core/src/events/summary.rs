//! Request-local sentence edits over the existing summary text, not a second
//! persisted summary store. Untouched sentences survive every window.
use super::{fit_summary, source_bit};
use serde_json::{Value, json};
use std::collections::HashSet;

fn sentences(text: &str) -> Vec<String> {
    text.split_inclusive(['。', '！', '？'])
        .filter_map(|s| fit_summary(Some(s)))
        .collect()
}

pub(super) fn indexed(text: Option<&str>) -> Value {
    json!(
        text.map(sentences)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(item_index, text)| json!({"item_index":item_index,"text":text}))
            .collect::<Vec<_>>()
    )
}

pub(super) fn apply(
    output: &Value,
    prior: Option<&Value>,
    input: &Value,
    start: i64,
    end: i64,
    mask: i64,
) -> Option<String> {
    let old_text = prior.and_then(|p| p["summary"].as_str());
    // Legacy full text can describe a fully visible event, but must never
    // overwrite inherited history with a new window's partial description.
    let history_outside = prior.is_some_and(|p| {
        p["start_ms"]
            .as_i64()
            .is_some_and(|n| n < input["candidate"]["start_ms"].as_i64().unwrap_or(start))
            || p["end_ms"]
                .as_i64()
                .is_some_and(|n| n > input["candidate"]["end_ms"].as_i64().unwrap_or(end))
    });
    if history_outside && old_text.is_none() {
        return None;
    }
    let Some(updates) = output.as_array() else {
        return if !history_outside && output.is_string() {
            fit_summary(output.as_str()).or_else(|| old_text.map(str::to_owned))
        } else {
            old_text.map(str::to_owned)
        };
    };
    let old = old_text.map(sentences).unwrap_or_default();
    let mut replacements: Vec<Option<String>> = vec![None; old.len()];
    let mut consumed = HashSet::new();
    let mut added = Vec::new();
    let current: HashSet<u64> = input["observations"]
        .as_array()?
        .iter()
        .filter_map(|o| o["evidence_index"].as_u64())
        .collect();
    let mut allowed = current.clone();
    if let Some(prior) = prior {
        for kind in ["key_points", "mentioned_objects"] {
            if let Some(items) = prior["understanding"][kind].as_array() {
                for item in items {
                    if let Some(refs) = item["evidence_indices"].as_array() {
                        allowed.extend(refs.iter().filter_map(Value::as_u64));
                    }
                }
            }
        }
    }
    for update in updates {
        let Some(text) = fit_summary(update["text"].as_str()) else {
            continue;
        };
        let Some(indices) = update["replaces"].as_array().and_then(|v| {
            v.iter()
                .map(|i| i.as_u64().and_then(|n| usize::try_from(n).ok()))
                .collect::<Option<Vec<_>>>()
        }) else {
            continue;
        };
        let unique: HashSet<usize> = indices.iter().copied().collect();
        if unique.len() != indices.len()
            || indices
                .iter()
                .any(|i| *i >= old.len() || consumed.contains(i))
        {
            continue;
        }
        // An omitted earlier point may be added using its inherited evidence;
        // replacing a saved sentence also requires evidence from this window.
        let supported = update["evidence_indices"].as_array().is_some_and(|refs| {
            !refs.is_empty()
                && refs.iter().all(|i| {
                    i.as_u64().is_some_and(|i| {
                        let r = &input["evidence"][i as usize];
                        allowed.contains(&i)
                            && r["source"]
                                .as_str()
                                .is_some_and(|s| source_bit(s) & mask != 0)
                            && r["start_ms"].as_i64().is_some_and(|n| n < end)
                            && r["end_ms"].as_i64().is_some_and(|n| n > start)
                    })
                })
        });
        let fresh = update["evidence_indices"].as_array().is_some_and(|refs| {
            refs.iter()
                .any(|i| i.as_u64().is_some_and(|i| current.contains(&i)))
        });
        if !supported || (!indices.is_empty() && !fresh) {
            continue;
        }
        if let Some(first) = indices.iter().min() {
            replacements[*first] = Some(text);
            consumed.extend(indices);
        } else {
            added.push(text);
        }
    }
    if consumed.is_empty() && added.is_empty() {
        return old_text.map(str::to_owned);
    }
    let mut result = Vec::new();
    for (i, text) in old.into_iter().enumerate() {
        if let Some(text) = replacements[i].take() {
            result.extend(sentences(&text));
        } else if !consumed.contains(&i) {
            result.push(text);
        }
    }
    for text in added {
        for sentence in sentences(&text) {
            if !result.contains(&sentence) {
                result.push(sentence);
            }
        }
    }
    fit_summary(Some(&result.join(" ")))
}
