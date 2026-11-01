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

// Windows 11's native caption keeps system window controls, accessibility,
// resizing and Snap behavior while using the web UI's build-time token colors.
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(
        window: *mut std::ffi::c_void,
        attribute: u32,
        value: *const u32,
        size: u32,
    ) -> i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn SystemParametersInfoW(
        action: u32,
        parameter: u32,
        value: *mut std::ffi::c_void,
        flags: u32,
    ) -> i32;
}

pub fn style_main_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[repr(C)]
    struct HighContrast {
        size: u32,
        flags: u32,
        scheme: *mut u16,
    }
    let mut contrast = HighContrast {
        size: std::mem::size_of::<HighContrast>() as u32,
        flags: 0,
        scheme: std::ptr::null_mut(),
    };
    let observed = unsafe {
        SystemParametersInfoW(
            0x42,
            contrast.size,
            (&mut contrast as *mut HighContrast).cast(),
            0,
        )
    };
    if observed == 0 {
        return Err("cannot read Windows contrast setting".into());
    }
    if contrast.flags & 1 != 0 {
        return Ok(());
    }
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
    let canvas: u32 = env!("LOPPI_WINDOW_CANVAS")
        .parse()
        .expect("compiled canvas color");
    let text: u32 = env!("LOPPI_WINDOW_TEXT")
        .parse()
        .expect("compiled text color");
    // DWMWA_BORDER_COLOR/CAPTION_COLOR/TEXT_COLOR; COLOR_NONE hides the rim.
    // https://learn.microsoft.com/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
    for (attribute, color) in [(34, 0xfffffffe), (35, canvas), (36, text)] {
        let result = unsafe { DwmSetWindowAttribute(hwnd, attribute, &color, 4) };
        if result < 0 {
            return Err(format!("cannot style Windows title bar: {result:#x}"));
        }
    }
    Ok(())
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
