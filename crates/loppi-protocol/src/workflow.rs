use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::EventSources;

pub const OTHER_TAG: Uuid = Uuid::from_u128(1);

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TagMode {
    #[default]
    Single,
    Multiple,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    Coarse,
    Precise,
}

impl Input {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coarse => "coarse",
            Self::Precise => "precise",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualInput {
    #[default]
    Configured,
    Coarse,
    PreparePrecise,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Tag {
    #[serde(default)]
    pub id: Uuid,
    pub name: String,
    pub criteria: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Strategy {
    #[serde(default)]
    pub id: Uuid,
    pub name: String,
    pub prompt: String,
    pub input: Input,
    #[serde(default)]
    pub tag_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AutoAsr {
    #[default]
    All,
    Tags {
        tag_ids: Vec<Uuid>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub mode: TagMode,
    pub tags: Vec<Tag>,
    pub auto_asr: AutoAsr,
    pub strategies: Vec<Strategy>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Label {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventStatus {
    pub phase: String,
    pub tag_state: String,
    pub labels: Vec<Label>,
    pub manual: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    ConfigShow,
    ConfigSet {
        config: Config,
        expected_revision: i64,
        #[serde(default)]
        preview: bool,
        #[serde(default)]
        add_required_asr_tags: bool,
    },
    Strategies,
    Complete {
        event_id: Uuid,
        expected_revision: i64,
        started_ms: Option<i64>,
        ended_ms: Option<i64>,
    },
    ConfirmRange {
        started_ms: i64,
        ended_ms: i64,
        sources: EventSources,
        title: Option<String>,
    },
    Tags {
        event_id: Uuid,
        expected_revision: i64,
        tag_ids: Vec<Uuid>,
        plan: Option<String>,
    },
    Retag {
        event_id: Uuid,
        expected_revision: i64,
    },
    Run {
        event_id: Uuid,
        strategy_id: Uuid,
        expected_revision: i64,
        config_revision: i64,
        #[serde(default)]
        input: ManualInput,
    },
    Runs {
        event_id: Uuid,
        run_id: Option<Uuid>,
        strategy_id: Option<Uuid>,
        cursor: Option<String>,
        #[serde(default)]
        latest_only: bool,
    },
    Result {
        event_id: Uuid,
        strategy_id: Uuid,
        version_id: Option<Uuid>,
        cursor: Option<String>,
    },
}

impl Action {
    pub fn read_only(&self) -> bool {
        matches!(
            self,
            Self::ConfigShow
                | Self::ConfigSet { preview: true, .. }
                | Self::Strategies
                | Self::Tags { plan: None, .. }
                | Self::Runs { .. }
                | Self::Result { .. }
        )
    }
}
