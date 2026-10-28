//! Explicitly maintained terminology; model-suggested entity IDs are never trusted.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub entity_id: String,
    pub name: String,
    pub kind: String,
    pub aliases: Vec<String>,
    pub enabled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    List,
    Upsert {
        id: Option<uuid::Uuid>,
        name: String,
        kind: String,
        aliases: Vec<String>,
        enabled: bool,
    },
    Remove {
        id: uuid::Uuid,
    },
    Configure {
        llm: Option<bool>,
        local_asr: Option<bool>,
        cloud_asr: Option<bool>,
    },
}
