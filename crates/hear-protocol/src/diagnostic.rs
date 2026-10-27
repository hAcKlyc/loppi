//! Content-free diagnostics. Free-form provider bodies and user text have no field here.
use crate::{LogRecord, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildInfo {
    pub component: String,
    pub version: String,
    pub protocol_version: u32,
    pub git_sha: String,
    pub dirty: Option<bool>,
    pub built_at_epoch: u64,
}
pub fn build_info(component: &str) -> BuildInfo {
    BuildInfo {
        component: component.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: PROTOCOL_VERSION,
        git_sha: env!("HEAR_GIT_SHA").into(),
        dirty: env!("HEAR_GIT_DIRTY").parse().ok(),
        built_at_epoch: env!("HEAR_BUILD_EPOCH").parse().expect("build timestamp"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
    Other,
    Missing,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<OutputFinishReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_display_asleep: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_frames: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_delta_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_failures: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::AudioSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<crate::CloudProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticFilter {
    pub from_ms: i64,
    pub to_ms: i64,
    pub event_id: Option<Uuid>,
    pub job_id: Option<Uuid>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogWindow {
    pub retained_from_ms: Option<u64>,
    pub retained_to_ms: Option<u64>,
    pub matched: usize,
    pub malformed_lines: usize,
    pub truncated: bool,
    pub failure_counts: std::collections::BTreeMap<String, usize>,
    pub failure_groups_truncated: bool,
    pub entries: Vec<LogRecord>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub schema: u32,
    pub observed_ms: i64,
    pub core_build: BuildInfo,
    pub filter: DiagnosticFilter,
    /// Explicit allowlisted SQL projections; never a table/row dump.
    pub facts: serde_json::Value,
    pub logs: LogWindow,
    pub logs_available: bool,
}
