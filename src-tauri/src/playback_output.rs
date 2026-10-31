//! GUI audio renderer only. Core still owns media, cleanup and recording facts.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::CStr;
use std::sync::OnceLock;
use tauri::Emitter;
static HOST: OnceLock<tauri::AppHandle> = OnceLock::new();
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
unsafe extern "C" {
    fn loppi_output_command(
        bytes: *const u8,
        count: usize,
        ended: extern "C" fn(u64, u64, u64, i32),
    ) -> *mut std::ffi::c_char;
    fn loppi_output_shutdown();
}
extern "C" fn ended(context: u64, buffer: u64, epoch: u64, status: i32) {
    if let Some(app) = HOST.get() {
        let _ = app.emit_to(
            "main",
            "loppi-playback-ended",
            serde_json::json!({"context":context,"buffer":buffer,"epoch":epoch,"status":status}),
        );
    }
}
#[tauri::command]
pub async fn playback_output(app: tauri::AppHandle, command: Command) -> Result<Value, String> {
    let _ = HOST.set(app.clone());
    let bytes = serde_json::to_vec(&command).map_err(|_| "playback_invalid_command")?;
    if bytes.len() > 1_600_000 {
        return Err("playback_buffer_limit".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            let result = unsafe {
                let pointer = loppi_output_command(bytes.as_ptr(), bytes.len(), ended);
                if pointer.is_null() {
                    Err("playback_device".into())
                } else {
                    let result =
                        serde_json::from_slice::<Value>(CStr::from_ptr(pointer).to_bytes())
                            .map_err(|_| "playback_device".to_owned());
                    libc::free(pointer.cast());
                    result.and_then(|value| {
                        if value["ok"] == true {
                            Ok(value["data"].clone())
                        } else {
                            Err(value["error"]
                                .as_str()
                                .unwrap_or("playback_device")
                                .to_owned())
                        }
                    })
                }
            };
            let _ = sender.send(result);
        })
        .map_err(|_| "playback_device".to_owned())?;
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "playback_device".to_owned())?
    })
    .await
    .map_err(|_| "playback_device".to_owned())?
}
pub fn shutdown() {
    unsafe {
        loppi_output_shutdown();
    }
}
