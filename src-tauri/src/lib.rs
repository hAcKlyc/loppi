use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::Manager;
mod entry;
mod onboarding;
use entry::{
    EntryRuntime, entry_action, entry_drag, entry_focus, entry_layout, entry_mode, entry_snapshot,
};
use std::sync::{Arc, Mutex};
static PRIVACY_REQUEST: Mutex<()> = Mutex::new(());

#[tauri::command]
async fn core_request(
    app: tauri::AppHandle,
    mut request: hear_protocol::Request,
    request_id: Option<uuid::Uuid>,
) -> Result<hear_protocol::Response, String> {
    if let hear_protocol::Request::CaptureScreenshot {
        excluded_window_ids,
        ..
    } = &mut request
    {
        *excluded_window_ids = vec![entry::screenshot_window(app.clone()).await?];
    }
    if matches!(request, hear_protocol::Request::ExecuteCleanup { .. }) {
        return Err("use the cleanup confirmation flow".into());
    }
    let runtime = Arc::clone(&app.state::<Arc<EntryRuntime>>());
    let status = matches!(request, hear_protocol::Request::Status);
    tauri::async_runtime::spawn_blocking(move || {
        if status {
            runtime.refresh(&app);
            let snapshot = runtime.snapshot();
            return snapshot
                .state
                .map(|state| hear_protocol::Response::Status {
                    id: uuid::Uuid::new_v4(),
                    state,
                })
                .ok_or_else(|| snapshot.error.unwrap_or("status unavailable".into()));
        }
        let result = hear_protocol::send_from_with_id(
            hear_protocol::ClientKind::Gui,
            request,
            request_id.unwrap_or_else(uuid::Uuid::new_v4),
        );
        runtime.request_refresh();
        result
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
async fn agent_access_info() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let cli = executable
            .parent()
            .ok_or("missing app directory")?
            .join("hear");
        let mut info = hear_protocol::agent_assets::info(&cli);
        info["status"] = hear_protocol::context::execute(hear_protocol::context::Query::Status(
            hear_protocol::context::StatusArgs::default(),
        ));
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn copy_agent_text(text: String) -> Result<(), String> {
    if text.len() > 32768 {
        return Err("copy text too large".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::Write;
        let mut child = Command::new("/usr/bin/pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child
            .stdin
            .take()
            .ok_or("clipboard unavailable")?
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        if child.wait().map_err(|e| e.to_string())?.success() {
            Ok(())
        } else {
            Err("clipboard unavailable".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn build_info() -> hear_protocol::BuildInfo {
    hear_protocol::build_info("gui")
}

#[tauri::command]
fn gui_ready() -> Result<(), String> {
    let mut event = hear_protocol::LogEvent::simple("gui", hear_protocol::LogLevel::Info, "opened");
    event.build = Some(hear_protocol::build_info("gui"));
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
    fn hear_show_exit_error(message: *const std::ffi::c_char);
    fn hear_microphone_authorization_status() -> i32;
    fn hear_request_microphone_authorization() -> i32;
    fn hear_screen_authorization_status() -> i32;
    fn hear_request_screen_authorization() -> i32;
    fn hear_tap_required() -> i32;
    fn hear_microphone_available() -> i32;
}

#[derive(serde::Serialize)]
struct PrivacyStatus {
    microphone: i32,
    microphone_available: bool,
    screen_and_system_audio: i32,
    tap_required: bool,
    tap_ready: bool,
}

#[tauri::command]
fn privacy_status() -> PrivacyStatus {
    let tap_required = unsafe { hear_tap_required() != 0 };
    PrivacyStatus {
        microphone: unsafe { hear_microphone_authorization_status() },
        microphone_available: unsafe { hear_microphone_available() != 0 },
        screen_and_system_audio: unsafe { hear_screen_authorization_status() },
        tap_required,
        tap_ready: tap_required
            && hear_protocol::data_dir()
                .is_ok_and(|directory| directory.join("tap-ready-v1").is_file()),
    }
}

#[tauri::command]
async fn request_microphone_permission() -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = PRIVACY_REQUEST
            .try_lock()
            .map_err(|_| "另一项授权正在进行，请完成后重试")?;
        Ok(unsafe { hear_request_microphone_authorization() })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn request_system_audio_permission() -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = PRIVACY_REQUEST
            .try_lock()
            .map_err(|_| "另一项授权正在进行，请完成后重试")?;
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
async fn request_screenshot_permission() -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = PRIVACY_REQUEST
            .try_lock()
            .map_err(|_| "另一项授权正在进行，请完成后重试")?;
        Ok(unsafe { hear_request_screen_authorization() })
    })
    .await
    .map_err(|e| e.to_string())?
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

fn show_exit_error(error: &str, native_exit: bool) {
    let instruction = if native_exit {
        "后台尚未退出。请稍后重新打开 Hear，再选择退出。"
    } else {
        "后台尚未退出。请稍后再次选择退出 Hear。"
    };
    // Error text is display data, never an AppleScript or shell program.
    let text = format!("{instruction}\n\n{error}").replace('\0', "�");
    if let Ok(text) = std::ffi::CString::new(text) {
        unsafe { hear_show_exit_error(text.as_ptr()) };
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(onboarding::Preferences::new(
                std::env::var_os("HEAR_DATA_DIR")
                    .map(|p| std::path::PathBuf::from(p).join("gui"))
                    .unwrap_or(app.path().app_config_dir()?)
                    .join("onboarding.json"),
            ));
            let runtime = EntryRuntime::new(
                std::env::var_os("HEAR_DATA_DIR")
                    .map(|p| std::path::PathBuf::from(p).join("gui"))
                    .unwrap_or(app.path().app_config_dir()?)
                    .join("entry-preferences.json"),
            );
            app.manage(Arc::clone(&runtime));
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "float",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("hear · 快速操作")
            .inner_size(68.0, 68.0)
            .decorations(false)
            .resizable(false)
            .transparent(true)
            .shadow(false)
            .focused(false)
            .visible(false)
            .build()?;
            entry::install(app.handle(), &window, &runtime)?;
            runtime.start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.label() == "main" {
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            build_info,
            agent_access_info,
            copy_agent_text,
            onboarding::onboarding_status,
            onboarding::complete_onboarding,
            onboarding::open_privacy_settings,
            onboarding::open_setup_help,
            core_request,
            cleanup_request,
            gui_ready,
            show_main,
            login_item_status,
            register_login_item,
            unregister_login_item,
            privacy_status,
            request_microphone_permission,
            request_system_audio_permission,
            request_screenshot_permission,
            entry_snapshot,
            entry_mode,
            entry_layout,
            entry_focus,
            entry_drag,
            entry_action
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // macOS's native terminate: (default menu, Cmd-Q and Dock Quit)
            // delivers Exit directly, without ExitRequested. Always drain here
            // too. quit_gui_core is idempotent and never starts a missing Core.
            tauri::RunEvent::Exit => {
                app.state::<Arc<EntryRuntime>>().stop();
                if let Err(error) = hear_protocol::quit_gui_core() {
                    eprintln!("Hear native exit incomplete: {error}");
                    show_exit_error(&error, true);
                }
                entry::dispose(app);
            }
            tauri::RunEvent::ExitRequested { api, .. } => {
                app.state::<Arc<EntryRuntime>>().stop();
                match hear_protocol::quit_gui_core() {
                    Ok(()) => entry::dispose(app),
                    Err(error) => {
                        api.prevent_exit();
                        eprintln!("Hear exit incomplete: {error}");
                        // Keep the GUI alive rather than silently abandoning a
                        // recording Core; the user can retry the Quit action.
                        show_exit_error(&error, false);
                    }
                }
            }
            tauri::RunEvent::Reopen { .. } => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        });
}
