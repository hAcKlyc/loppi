use tauri::Manager;

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

unsafe extern "C" {
    fn hear_agent_status() -> i32;
    fn hear_agent_register() -> i32;
    fn hear_agent_unregister() -> i32;
    fn hear_microphone_authorization_status() -> i32;
    fn hear_request_microphone_authorization() -> i32;
    fn hear_screen_authorization_status() -> i32;
    fn hear_request_screen_authorization() -> i32;
}

#[derive(serde::Serialize)]
struct PrivacyStatus {
    microphone: i32,
    screen_and_system_audio: i32,
}

#[tauri::command]
fn privacy_status() -> PrivacyStatus {
    PrivacyStatus {
        microphone: unsafe { hear_microphone_authorization_status() },
        screen_and_system_audio: unsafe { hear_screen_authorization_status() },
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
    tauri::async_runtime::spawn_blocking(|| unsafe { hear_request_screen_authorization() })
        .await
        .map_err(|error| error.to_string())
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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            core_status,
            gui_ready,
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
