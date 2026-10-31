//! GUI audio renderer; Core owns media and capture/exclusion facts.
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Open,
    Resume {
        context: u64,
        epoch: u64,
    },
    Suspend {
        context: u64,
        epoch: u64,
    },
    Close {
        context: u64,
        epoch: u64,
    },
    Load {
        context: u64,
        epoch: u64,
        wav: String,
    },
    Schedule {
        context: u64,
        epoch: u64,
        buffer: u64,
        when: f64,
    },
    Remove {
        context: u64,
        epoch: u64,
        buffer: u64,
    },
}

#[cfg(target_os = "macos")]
#[path = "playback_macos.rs"]
mod backend;
#[cfg(windows)]
#[path = "playback_windows.rs"]
mod backend;
pub use backend::shutdown;
#[tauri::command]
pub async fn playback_output(
    app: tauri::AppHandle,
    command: Command,
) -> Result<serde_json::Value, String> {
    backend::playback_output(app, command).await
}
