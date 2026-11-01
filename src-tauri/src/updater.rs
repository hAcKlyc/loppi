//! The desktop host owns updates; renderers cannot invoke the plugin installer.
//! Downloads (including signature verification) finish before touching the Core.
use serde::Serialize;
use std::io::{Read, Seek, Write};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, State, ipc::Channel};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct Updates(tauri::async_runtime::Mutex<Option<ReadyUpdate>>);

struct ReadyUpdate {
    update: Update,
    file: Arc<std::fs::File>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    current_version: String,
    version: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    phase: &'static str,
}

fn require_main(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("请在主窗口中检查和安装更新".into())
    }
}

#[tauri::command]
pub async fn check_update(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    updates: State<'_, Updates>,
    progress: Channel<Progress>,
) -> Result<Option<AvailableUpdate>, String> {
    require_main(&window)?;
    let mut slot = updates
        .0
        .try_lock()
        .map_err(|_| "更新操作正在进行，请稍后重试")?;
    let update = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        // The plugin's default hook clears WebView resources and hides Windows
        // before ShellExecute can report failure. Keep the host usable if the
        // installer cannot launch; successful process exit releases OS resources.
        .on_before_exit(|| {})
        .build()
        .map_err(|e| format!("无法准备更新检查：{e}"))?
        .check()
        .await
        .map_err(|_| "无法检查更新，请检查网络后重试".to_string())?;
    let Some(mut update) = update else {
        *slot = None;
        return Ok(None);
    };
    let available = AvailableUpdate {
        current_version: app.package_info().version.to_string(),
        version: update.version.clone(),
    };
    if slot
        .as_ref()
        .is_some_and(|ready| ready.update.version == update.version)
    {
        return Ok(Some(available));
    }
    update.timeout = Some(Duration::from_secs(15 * 60));
    let _ = progress.send(Progress {
        phase: "downloading",
    });
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|_| {
        "更新包下载或签名验证失败，录音未因更新而停止；请检查网络后重试".to_string()
    })?;
    // An anonymous session file avoids retaining the full installer in RAM.
    // The OS removes it when the handle closes, even on Windows process exit.
    let file = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        let mut file = tempfile::tempfile().map_err(|e| format!("无法暂存更新包：{e}"))?;
        file.write_all(&bytes)
            .map_err(|e| format!("无法暂存更新包：{e}"))?;
        Ok(file)
    })
    .await
    .map_err(|e| e.to_string())??;
    // Only verified bytes may make the restart button available. No network
    // request is needed on the user-triggered installation path.
    *slot = Some(ReadyUpdate {
        update,
        file: Arc::new(file),
    });
    Ok(Some(available))
}

// Shared ordering makes both failure boundaries explicit. Reopening IPC does
// not change recording intent; AppOpened follows the existing activation path.
fn install_when_stopped(
    stop: impl FnOnce() -> Result<(), String>,
    install: impl FnOnce() -> Result<(), String>,
    restore: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let result = stop().and_then(|()| install());
    if let Err(error) = result {
        return match restore() {
            Ok(()) => Err(error),
            Err(recovery) => Err(format!(
                "{error}；无法恢复应用连接：{recovery}，请重新打开 loppi"
            )),
        };
    }
    Ok(())
}

#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    updates: State<'_, Updates>,
    version: String,
    progress: Channel<Progress>,
) -> Result<(), String> {
    require_main(&window)?;
    let slot = updates
        .0
        .try_lock()
        .map_err(|_| "更新操作正在进行，请稍后重试")?;
    let ready = slot.as_ref().ok_or("更新尚未下载完成，请先检查更新")?;
    if ready.update.version != version {
        return Err("可用版本已变化，请重新检查并确认更新".into());
    }
    let update = ready.update.clone();
    let file = Arc::clone(&ready.file);
    let host = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Read the verified session file before asking the Core to leave. A
        // disk/read failure must not interrupt recording or consume the update.
        let mut file = file
            .try_clone()
            .map_err(|e| format!("无法读取更新包：{e}"))?;
        file.rewind().map_err(|e| format!("无法读取更新包：{e}"))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| format!("无法读取更新包：{e}"))?;
        let _ = progress.send(Progress { phase: "stopping" });
        install_when_stopped(
            || {
                loppi_protocol::quit_gui_core()
                    .map_err(|e| format!("后台未能正常退出，尚未安装更新：{e}"))
            },
            || {
                crate::playback_output::shutdown();
                let _ = progress.send(Progress {
                    phase: "installing",
                });
                update
                    .install(bytes.as_slice())
                    .map_err(|e| format!("安装更新失败：{e}"))
            },
            || {
                loppi_protocol::resume_gui_core()?;
                host.state::<std::sync::Arc<crate::entry::EntryRuntime>>()
                    .request_refresh();
                Ok(())
            },
        )
    })
    .await
    .map_err(|e| format!("更新任务未完成，请重新打开 loppi：{e}"))??;
    // Windows' installer exits and relaunches us itself; macOS returns here.
    drop(slot);
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn shutdown_failure_never_installs_and_reopens_requests() {
        let calls = RefCell::new(vec![]);
        let result = install_when_stopped(
            || {
                calls.borrow_mut().push("stop");
                Err("busy".into())
            },
            || {
                calls.borrow_mut().push("install");
                Ok(())
            },
            || {
                calls.borrow_mut().push("restore");
                Ok(())
            },
        );
        assert_eq!(result, Err("busy".into()));
        assert_eq!(*calls.borrow(), ["stop", "restore"]);
    }

    #[test]
    fn install_failure_restores_but_success_stays_closed_until_restart() {
        for failed in [true, false] {
            let calls = RefCell::new(vec![]);
            let result = install_when_stopped(
                || {
                    calls.borrow_mut().push("stop");
                    Ok(())
                },
                || {
                    calls.borrow_mut().push("install");
                    if failed { Err("disk".into()) } else { Ok(()) }
                },
                || {
                    calls.borrow_mut().push("restore");
                    Ok(())
                },
            );
            assert_eq!(result.is_err(), failed);
            assert_eq!(
                *calls.borrow(),
                if failed {
                    vec!["stop", "install", "restore"]
                } else {
                    vec!["stop", "install"]
                }
            );
        }
    }
}
