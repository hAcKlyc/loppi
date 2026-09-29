use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::Manager;

#[tauri::command]
async fn core_request(request: hear_protocol::Request) -> Result<hear_protocol::Response, String> {
    if matches!(request, hear_protocol::Request::ExecuteCleanup { .. }) {
        return Err("use the cleanup confirmation flow".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        hear_protocol::send_from(hear_protocol::ClientKind::Gui, request)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn cleanup_request(
    request: hear_protocol::Request,
) -> Result<hear_protocol::Response, String> {
    let hear_protocol::Request::ExecuteCleanup {
        confirmed_all_data, ..
    } = &request
    else {
        return Err("cleanup request required".into());
    };
    if *confirmed_all_data {
        let confirmed = tauri::async_runtime::spawn_blocking(|| {
            std::process::Command::new("/usr/bin/osascript")
                .args(["-e", "display dialog \"将永久清除选中范围的录音、文字、笔记、截图和相关声纹样例。已经导出或发送给服务商的副本无法从这里撤回。\" with title \"清除全部数据\" buttons {\"取消\", \"清除全部数据\"} default button \"取消\" cancel button \"取消\" with icon caution"])
                .output()
                .is_ok_and(|result| result.status.success() && String::from_utf8_lossy(&result.stdout).contains("button returned:清除全部数据"))
        })
        .await
        .map_err(|error| error.to_string())?;
        if !confirmed {
            return Err("已取消清除全部数据".into());
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        hear_protocol::send_from(hear_protocol::ClientKind::Gui, request)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn core_status() -> Result<hear_protocol::ServiceState, String> {
    match hear_protocol::send_from(
        hear_protocol::ClientKind::Gui,
        hear_protocol::Request::Status,
    )? {
        hear_protocol::Response::Status { state, .. } => Ok(state),
        hear_protocol::Response::Models { .. } => Err("unexpected model response".to_owned()),
        hear_protocol::Response::ProbeResult { .. } => Err("unexpected probe response".to_owned()),
        hear_protocol::Response::RecordTestResult { .. } => {
            Err("unexpected recording test response".to_owned())
        }
        hear_protocol::Response::Logs { .. } => Err("unexpected log response".to_owned()),
        hear_protocol::Response::Error { code, message, .. } => Err(format!("{code}: {message}")),
        _ => Err("unexpected core response".to_owned()),
    }
}

#[tauri::command]
fn gui_ready() -> Result<(), String> {
    let event = hear_protocol::LogEvent::simple("gui", hear_protocol::LogLevel::Info, "opened");
    hear_protocol::send_from(
        hear_protocol::ClientKind::Gui,
        hear_protocol::Request::Log { event },
    )?;
    Ok(())
}

#[tauri::command]
fn show_main(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window unavailable")?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

unsafe extern "C" {
    fn hear_agent_status() -> i32;
    fn hear_agent_register() -> i32;
    fn hear_agent_unregister() -> i32;
    fn hear_microphone_authorization_status() -> i32;
    fn hear_request_microphone_authorization() -> i32;
    fn hear_screen_authorization_status() -> i32;
    fn hear_request_screen_authorization() -> i32;
    fn hear_tap_required() -> i32;
}

#[derive(serde::Serialize)]
struct PrivacyStatus {
    microphone: i32,
    screen_and_system_audio: i32,
    tap_required: bool,
    tap_ready: bool,
}

#[tauri::command]
fn privacy_status() -> PrivacyStatus {
    let tap_required = unsafe { hear_tap_required() != 0 };
    PrivacyStatus {
        microphone: unsafe { hear_microphone_authorization_status() },
        screen_and_system_audio: unsafe { hear_screen_authorization_status() },
        tap_required,
        tap_ready: tap_required
            && hear_protocol::data_dir()
                .is_ok_and(|directory| directory.join("tap-ready-v1").is_file()),
    }
}

#[tauri::command]
async fn request_microphone_permission() -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(|| unsafe { hear_request_microphone_authorization() })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn request_system_audio_permission() -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(|| {
        if unsafe { hear_tap_required() } == 0 {
            return Ok(unsafe { hear_request_screen_authorization() });
        }
        let directory = hear_protocol::data_dir()?;
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let marker = directory.join("tap-ready-v1");
        // A previous successful setup is historical evidence, not a live TCC
        // grant. Revalidation must clear it before making another attempt.
        match std::fs::remove_file(&marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        // Apple's first Tap read prompts for AudioCapture permission. Run it
        // from the visible app, not the login item, and never persist readiness
        // until the signed helper has actually returned audio frames.
        let binary = std::env::current_exe().map_err(|error| error.to_string())?;
        let contents = binary
            .parent()
            .and_then(|path| path.parent())
            .ok_or("cannot locate hear application contents")?;
        let helper = contents.join("Library/LoginItems/HearCore.app/Contents/MacOS/hear-core");
        let mut child = Command::new(&helper)
            .args([
                "probe-system",
                "1",
                "--parent-pid",
                &std::process::id().to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start system audio authorization: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(75);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    return Err(format!("电脑声音授权未完成（{status}）"));
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("电脑声音授权等待超时；请在系统设置中允许后重试".to_owned());
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("电脑声音授权检查失败：{error}"));
                }
            }
        }
        let pending = directory.join(format!("tap-ready-v1.{}.tmp", std::process::id()));
        let result = (|| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&pending)
                .map_err(|error| error.to_string())?;
            file.write_all(b"foreground Tap probe succeeded\n")
                .and_then(|_| file.sync_all())
                .map_err(|error| error.to_string())?;
            std::fs::rename(&pending, &marker).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(pending);
        }
        result.map(|()| 1)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn login_item_status() -> i32 {
    unsafe { hear_agent_status() }
}

#[tauri::command]
fn register_login_item() -> Result<i32, String> {
    let result = unsafe { hear_agent_register() };
    if result == 0 {
        Ok(unsafe { hear_agent_status() })
    } else {
        Err(format!(
            "login item registration failed with macOS code {result}"
        ))
    }
}

#[tauri::command]
fn unregister_login_item() -> Result<i32, String> {
    let result = unsafe { hear_agent_unregister() };
    if result == 0 {
        Ok(unsafe { hear_agent_status() })
    } else {
        Err(format!(
            "login item removal failed with macOS code {result}"
        ))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let monitor = app
                .primary_monitor()?
                .ok_or("primary display unavailable")?;
            let scale = monitor.scale_factor();
            let width = 68.0;
            let height = 68.0;
            let x =
                (monitor.position().x as f64 + monitor.size().width as f64) / scale - width - 20.0;
            let y = (monitor.position().y as f64 + monitor.size().height as f64) / scale
                - height
                - 20.0;
            tauri::WebviewWindowBuilder::new(
                app,
                "float",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("hear")
            .inner_size(width, height)
            .position(x, y)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .transparent(true)
            .shadow(false)
            .build()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            core_request,
            cleanup_request,
            core_status,
            gui_ready,
            show_main,
            login_item_status,
            register_login_item,
            unregister_login_item,
            privacy_status,
            request_microphone_permission,
            request_system_audio_permission
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { .. } => {
                let _ = hear_protocol::send_from(
                    hear_protocol::ClientKind::Gui,
                    hear_protocol::Request::Quit,
                );
            }
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        });
}
