//! The desktop host owns checks, the verified package and installation.
mod schedule;

use schedule::{DOWNLOAD_RETRY, Deadline, QUICK_RETRY, Schedule, Time};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::async_runtime::{Mutex as AsyncMutex, spawn, spawn_blocking};
use tauri::{Emitter, Manager, State};
use tauri_plugin_updater::{Error as PluginError, Update, UpdaterExt};
use tokio::{sync::Notify, time::timeout};

const STATUS_EVENT: &str = "loppi-update-status";

pub struct Updates {
    work: AsyncMutex<Work>,
    status: Mutex<UpdateStatus>,
    wake: Notify,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            work: AsyncMutex::new(Work {
                schedule: Schedule::new(Time::now()),
                ready: None,
                failed_downloads: HashMap::new(),
            }),
            status: Mutex::default(),
            wake: Notify::new(),
        }
    }
}

struct Work {
    schedule: Schedule,
    ready: Option<ReadyUpdate>,
    failed_downloads: HashMap<(String, String), FailedDownload>,
}

struct ReadyUpdate {
    update: Update,
    file: Arc<std::fs::File>,
}

struct FailedDownload {
    retry: DownloadRetry,
    message: String,
}

#[derive(Clone, Copy)]
enum DownloadRetry {
    Soon(Deadline),
    Later(Deadline),
    Manual,
}

impl FailedDownload {
    fn record(error: DownloadError, now: Time, previous: Option<&Self>) -> Self {
        let signature_rejected =
            previous.is_some_and(|failure| matches!(failure.retry, DownloadRetry::Manual));
        let retry = match error {
            _ if signature_rejected => DownloadRetry::Manual,
            DownloadError::Signature => DownloadRetry::Manual,
            DownloadError::Network if previous.is_none() => {
                DownloadRetry::Soon(Deadline::after(now, QUICK_RETRY))
            }
            _ => DownloadRetry::Later(Deadline::after(now, DOWNLOAD_RETRY)),
        };
        Self {
            retry,
            message: error.message(),
        }
    }

    fn can_retry(&self, now: Time) -> bool {
        match self.retry {
            DownloadRetry::Soon(deadline) | DownloadRetry::Later(deadline) => {
                deadline.remaining(now).is_zero()
            }
            DownloadRetry::Manual => false,
        }
    }

    fn schedule_retry(&self, schedule: &mut Schedule) {
        if let DownloadRetry::Soon(deadline) = self.retry {
            schedule.retry_download_at(deadline);
        }
    }
}

enum DownloadError {
    Network,
    Signature,
    Storage(String),
}

impl DownloadError {
    fn message(self) -> String {
        match self {
            Self::Network => "更新包下载失败，请检查网络后重试".into(),
            Self::Signature => "更新包签名验证失败，请稍后检查新版".into(),
            Self::Storage(message) => message,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AvailableUpdate {
    current_version: String,
    version: String,
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Idle,
    Checking,
    Downloading,
    Stopping,
    Installing,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    revision: u64,
    phase: Phase,
    available: Option<AvailableUpdate>,
    checked: bool,
    error: String,
}

impl Updates {
    fn publish(&self, app: &tauri::AppHandle, change: impl FnOnce(&mut UpdateStatus)) {
        let snapshot = {
            let mut status = self.status.lock().unwrap();
            change(&mut status);
            status.revision += 1;
            status.clone()
        };
        let _ = app.emit_to("main", STATUS_EVENT, snapshot);
    }

    async fn check(&self, app: &tauri::AppHandle, work: &mut Work, manual: bool) {
        self.publish(app, |status| {
            status.phase = Phase::Checking;
            status.error.clear();
        });
        let result = self.prepare(app, work, manual).await;
        self.publish(app, |status| {
            status.phase = Phase::Idle;
            status.available = work.ready.as_ref().map(|ready| AvailableUpdate {
                current_version: app.package_info().version.to_string(),
                version: ready.update.version.clone(),
            });
            status.checked |= result.is_ok();
            status.error = result.err().unwrap_or_default();
        });
    }

    async fn prepare(
        &self,
        app: &tauri::AppHandle,
        work: &mut Work,
        manual: bool,
    ) -> Result<(), String> {
        work.schedule.checking(Time::now());
        let result = lookup(app).await;
        work.schedule.checked(Time::now(), result.is_ok());
        let Some(mut update) = result? else {
            work.ready = None;
            return Ok(());
        };
        if work.ready.as_ref().is_some_and(|ready| {
            ready.update.version == update.version && ready.update.signature == update.signature
        }) {
            return Ok(());
        }
        let package = (update.version.clone(), update.signature.clone());
        let failure = work.failed_downloads.get(&package);
        if let Some(failure) = failure
            && !manual
            && !failure.can_retry(Time::now())
        {
            failure.schedule_retry(&mut work.schedule);
            return Err(failure.message.clone());
        }
        self.publish(app, |status| status.phase = Phase::Downloading);
        match download(&mut update).await {
            Ok(file) => {
                work.ready = Some(ReadyUpdate { update, file });
                work.failed_downloads.remove(&package);
                Ok(())
            }
            Err(error) => {
                let failure = FailedDownload::record(error, Time::now(), failure);
                failure.schedule_retry(&mut work.schedule);
                let message = failure.message.clone();
                work.failed_downloads.insert(package, failure);
                Err(message)
            }
        }
    }
}

pub fn start(app: tauri::AppHandle) {
    spawn(async move {
        let updates = app.state::<Updates>();
        loop {
            let delay = {
                let mut work = updates.work.lock().await;
                if work.schedule.remaining(Time::now()).is_zero() {
                    updates.check(&app, &mut work, false).await;
                }
                work.schedule.remaining(Time::now())
            };
            let _ = timeout(delay, updates.wake.notified()).await;
        }
    });
}

pub fn reconsider_schedule(app: &tauri::AppHandle) {
    if let Some(updates) = app.try_state::<Updates>() {
        updates.wake.notify_one();
    }
}

fn require_main(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("请在主窗口中检查和安装更新".into())
    }
}

#[tauri::command]
pub fn update_status(
    window: tauri::WebviewWindow,
    updates: State<'_, Updates>,
) -> Result<UpdateStatus, String> {
    require_main(&window)?;
    Ok(updates.status.lock().unwrap().clone())
}

#[tauri::command]
pub async fn check_update(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    updates: State<'_, Updates>,
) -> Result<(), String> {
    require_main(&window)?;
    let revision = updates.status.lock().unwrap().revision;
    let mut work = updates.work.lock().await;
    // A check that finished while this request waited already served it.
    // Taking the lock also works when the scheduler was only reading its deadline.
    let current_revision = updates.status.lock().unwrap().revision;
    if current_revision == revision {
        updates.check(&app, &mut work, true).await;
        updates.wake.notify_one();
    }
    Ok(())
}

async fn lookup(app: &tauri::AppHandle) -> Result<Option<Update>, String> {
    app.updater_builder()
        .timeout(Duration::from_secs(30))
        // Keep Windows usable if the installer fails to launch.
        .on_before_exit(|| {})
        .build()
        .map_err(|e| format!("无法准备更新检查：{e}"))?
        .check()
        .await
        .map_err(|_| "无法检查更新，请检查网络后重试".to_string())
}

async fn download(update: &mut Update) -> Result<Arc<std::fs::File>, DownloadError> {
    update.timeout = Some(Duration::from_secs(15 * 60));
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| match error {
            PluginError::Reqwest(_) | PluginError::Network(_) => DownloadError::Network,
            _ => DownloadError::Signature,
        })?;
    // The anonymous file is scoped to the host session. Only verified bytes
    // can become ready, and neither installation nor restart needs the network.
    let file = spawn_blocking(move || -> Result<_, String> {
        let mut file = tempfile::tempfile().map_err(|e| format!("无法暂存更新包：{e}"))?;
        file.write_all(&bytes)
            .map_err(|e| format!("无法暂存更新包：{e}"))?;
        Ok(file)
    })
    .await
    .map_err(|e| DownloadError::Storage(e.to_string()))?
    .map_err(DownloadError::Storage)?;
    Ok(Arc::new(file))
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
) -> Result<(), String> {
    require_main(&window)?;
    let work = updates
        .work
        .try_lock()
        .map_err(|_| "更新操作正在进行，请稍后重试")?;
    let ready = work
        .ready
        .as_ref()
        .ok_or("更新尚未下载完成，请先检查更新")?;
    if ready.update.version != version {
        return Err("可用版本已变化，请重新检查并确认更新".into());
    }
    let update = ready.update.clone();
    let file = Arc::clone(&ready.file);
    let host = app.clone();
    updates.publish(&app, |status| {
        status.phase = Phase::Stopping;
        status.error.clear();
    });
    let result = spawn_blocking(move || {
        // Read the verified session file before asking the Core to leave. A
        // disk/read failure must not interrupt recording or consume the update.
        let mut file = file
            .try_clone()
            .map_err(|e| format!("无法读取更新包：{e}"))?;
        file.rewind().map_err(|e| format!("无法读取更新包：{e}"))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| format!("无法读取更新包：{e}"))?;
        install_when_stopped(
            || {
                loppi_protocol::quit_gui_core()
                    .map_err(|e| format!("后台未能正常退出，尚未安装更新：{e}"))
            },
            || {
                crate::playback_output::shutdown();
                host.state::<Updates>()
                    .publish(&host, |status| status.phase = Phase::Installing);
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
    .map_err(|e| format!("更新任务未完成，请重新打开 loppi：{e}"))
    .and_then(|result| result);
    if let Err(message) = result {
        updates.publish(&app, |status| {
            status.phase = Phase::Idle;
            status.error = message;
        });
        return Ok(());
    }
    // Windows' installer exits and relaunches us itself; macOS returns here.
    drop(work);
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn manual_network_failure_keeps_an_earlier_signature_rejection() {
        let now = Time::now();
        let rejected = FailedDownload::record(DownloadError::Signature, now, None);
        let retry = FailedDownload::record(DownloadError::Network, now, Some(&rejected));
        assert!(matches!(retry.retry, DownloadRetry::Manual));
        assert!(!retry.can_retry(now));
    }

    #[test]
    fn first_network_failure_retries_quickly_and_later_failures_wait_longer() {
        let now = Time::now();
        let first = FailedDownload::record(DownloadError::Network, now, None);
        assert!(
            matches!(first.retry, DownloadRetry::Soon(deadline) if deadline.remaining(now) == QUICK_RETRY)
        );
        let second = FailedDownload::record(DownloadError::Network, now, Some(&first));
        assert!(
            matches!(second.retry, DownloadRetry::Later(deadline) if deadline.remaining(now) == DOWNLOAD_RETRY)
        );
    }

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
