use std::sync::atomic::{AtomicI32, Ordering};
use std::{ffi::CString, process::Stdio};
// Ephemeral native probe fact for this GUI lifetime; Core remains capture owner.
static SYSTEM_AUDIO_PROBED: AtomicI32 = AtomicI32::new(0);
pub fn system_audio_status() -> i32 {
    if let Ok(loppi_protocol::Response::Status { state, .. }) = loppi_protocol::send_existing(
        loppi_protocol::ClientKind::Gui,
        loppi_protocol::Request::Status,
    ) {
        match state.system_phase {
            loppi_protocol::SourcePhase::Recording => return 1,
            loppi_protocol::SourcePhase::Unavailable => {
                SYSTEM_AUDIO_PROBED.store(0, Ordering::Relaxed)
            }
            _ => {}
        }
    }
    SYSTEM_AUDIO_PROBED.load(Ordering::Relaxed)
}
pub fn initialize() -> Result<(), String> {
    loppi_protocol::platform::require_supported().map_err(|e| e.to_string())?;
    if std::env::args().any(|arg| arg == "--login") {
        let core = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("loppi-core.exe");
        loppi_protocol::platform::command(core)
            .args(["serve", "--login"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
unsafe extern "C" {
    fn loppi_open_url(url: *const std::ffi::c_char) -> i32;
    fn loppi_copy_text(text: *const std::ffi::c_char) -> i32;
}
pub fn open_url(url: &str) -> Result<(), String> {
    let url = CString::new(url).map_err(|_| "invalid URL")?;
    if unsafe { loppi_open_url(url.as_ptr()) } == 0 {
        return Err("cannot open Windows settings or browser".into());
    }
    Ok(())
}
pub fn copy_text(text: &str) -> Result<(), String> {
    if text.len() > 32768 {
        return Err("copy text too large".into());
    }
    let text = CString::new(text).map_err(|_| "invalid clipboard text")?;
    if unsafe { loppi_copy_text(text.as_ptr()) } == 0 {
        return Err("clipboard unavailable".into());
    }
    Ok(())
}
pub async fn probe_system() -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(|| {
        SYSTEM_AUDIO_PROBED.store(0, Ordering::Relaxed);
        match loppi_protocol::send_from(
            loppi_protocol::ClientKind::Gui,
            loppi_protocol::Request::Probe {
                source: loppi_protocol::AudioSource::System,
                seconds: 1,
            },
        )? {
            loppi_protocol::Response::ProbeResult { stats, .. } if stats.frames > 0 => {
                SYSTEM_AUDIO_PROBED.store(1, Ordering::Relaxed);
                Ok(1)
            }
            loppi_protocol::Response::Error { message, .. } => Err(message),
            _ => Err("unexpected audio probe response".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
