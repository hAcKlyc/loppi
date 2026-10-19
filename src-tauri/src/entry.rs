use hear_protocol::{
    CaptureBlocker as Blocker, ClientKind, MomentKind, Request, Response, ServiceState,
    SourcePhase as Phase,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use uuid::Uuid;

static HOST: OnceLock<tauri::AppHandle> = OnceLock::new();
unsafe extern "C" {
    fn hear_entry_active() -> i32;
    fn hear_entry_geometry(window: *mut std::ffi::c_void) -> Geometry;
    fn hear_entry_install(window: *mut std::ffi::c_void, callback: extern "C" fn(i32, i32));
    fn hear_entry_layout(
        window: *mut std::ffi::c_void,
        mode: i32,
        view: i32,
        feedback: i32,
        detail: i32,
        suppress: i32,
    ) -> Geometry;
    fn hear_entry_focus(window: *mut std::ffi::c_void, acquire: i32, restore: i32) -> i32;
    fn hear_entry_drag(window: *mut std::ffi::c_void, dragging: i32);
    fn hear_entry_position(window: *mut std::ffi::c_void) -> *mut libc::c_char;
    fn hear_entry_restore(window: *mut std::ffi::c_void, uuid: *const libc::c_char, x: f64, y: f64);
    fn hear_entry_window_id(window: *mut std::ffi::c_void) -> u32;
    fn hear_entry_dispose(window: *mut std::ffi::c_void);
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Top,
    Floating,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Position {
    display_uuid: String,
    x: f64,
    y: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Preferences {
    schema_version: u8,
    mode: Mode,
    floating: Option<Position>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mode: Mode::Top,
            floating: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Listening,
    Sleeping,
    Waiting,
    Partial,
    Interrupted,
}
#[derive(Clone, Debug, Serialize)]
pub struct Notice {
    id: u64,
    message: String,
}
#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub sequence: u64,
    pub state: Option<ServiceState>,
    pub error: Option<String>,
    pub mode: Mode,
    pub appearance: Appearance,
    pub detail: Option<String>,
    pub control: Option<String>,
    pub notice: Option<Notice>,
}
#[derive(Clone)]
struct Pending {
    id: Uuid,
    request: Request,
    in_flight: bool,
    response: Option<Response>,
}
struct Inner {
    snapshot: Snapshot,
    prefs: Preferences,
    pending: BTreeMap<String, Pending>,
    observed: bool,
    armed: bool,
    layout_generation: u64,
    canceled_generation: u64,
    view: View,
}
impl Inner {
    fn focus_is_current(&self, generation: u64) -> bool {
        generation <= self.layout_generation
            && generation > self.canceled_generation
            && self.view == View::Comment
    }
    fn layout_is_current(&self, generation: u64, mode: Mode, view: View) -> bool {
        generation == self.layout_generation
            && mode == self.snapshot.mode
            && (generation > self.canceled_generation || view == View::Collapsed)
    }
    fn observe(&mut self, result: Result<ServiceState, String>) -> Snapshot {
        self.snapshot.sequence += 1;
        self.snapshot.notice = None;
        self.snapshot.state = result.as_ref().ok().cloned();
        self.snapshot.error = result.err();
        let (appearance, detail, control) = presentation(self.snapshot.state.as_ref());
        // Unknown/transition/manual pause do not rearm an unresolved fault episode.
        if appearance == Appearance::Listening {
            self.armed = true;
        }
        if appearance == Appearance::Interrupted {
            if self.observed && self.armed {
                self.snapshot.notice = Some(Notice {
                    id: self.snapshot.sequence,
                    message: detail.clone().unwrap_or_default(),
                });
            }
            self.armed = false;
        }
        if self.snapshot.state.is_some() {
            self.observed = true;
        }
        self.snapshot.appearance = appearance;
        self.snapshot.detail = detail;
        self.snapshot.control = control;
        self.snapshot.clone()
    }
}
pub struct EntryRuntime {
    inner: Mutex<Inner>,
    query: Mutex<()>,
    wake: (Mutex<bool>, Condvar),
    stopped: AtomicBool,
    path: PathBuf,
}

fn presentation(s: Option<&ServiceState>) -> (Appearance, Option<String>, Option<String>) {
    let Some(s) = s else {
        return (
            Appearance::Waiting,
            Some("暂时无法确认状态，请稍后重试".into()),
            None,
        );
    };
    let any = s.microphone_recording || s.system_recording;
    let stopping = s.microphone_phase == Phase::Stopping || s.system_phase == Phase::Stopping;
    let partial = (s.microphone_enabled && s.microphone_phase == Phase::Unavailable)
        || (s.system_enabled && s.system_phase == Phase::Unavailable);
    let starting = (s.microphone_enabled && s.microphone_phase == Phase::Starting)
        || (s.system_enabled && s.system_phase == Phase::Starting);
    let control = if stopping {
        None
    } else {
        Some(if any || (s.enabled_intent && !s.paused_intent && s.capture_blocker==Blocker::None) { "pause" } else if s.enabled_intent { "resume" } else { "start" }.into())
    };
    let warning = s
        .storage_warning
        .then(|| "音频空间即将不足，可在设置中清理".into());
    if stopping {
        return (
            Appearance::Waiting,
            Some("正在切换，请稍候".into()),
            control,
        );
    }
    if any {
        return if partial {
            (
                Appearance::Partial,
                Some(
                    if s.microphone_enabled && !s.microphone_recording {
                        "麦克风暂不可用，可在设置中检查"
                    } else {
                        "电脑声音暂不可用，可在设置中检查"
                    }
                    .into(),
                ),
                control,
            )
        } else if starting {
            (
                Appearance::Waiting,
                Some("正在等待声音来源".into()),
                control,
            )
        } else {
            (Appearance::Listening, warning, control)
        };
    }
    if s.storage_paused {
        return (
            Appearance::Interrupted,
            Some("音频空间已满，请清理后继续".into()),
            control,
        );
    }
    if s.paused_intent || !s.enabled_intent {
        return (
            Appearance::Sleeping,
            (s.paused_intent && s.recording_schedule.paused_by_schedule)
                .then(|| "定时暂停，可随时手动继续录制".into()),
            control,
        );
    }
    let detail = match s.capture_blocker {
        Blocker::SourceUnavailable => "声音来源暂不可用，请检查权限或设备",
        Blocker::SourcesDisabled => "声音来源未开启，请在设置中选择",
        Blocker::ModelNotReady => "本地模型尚未准备好，请查看设置",
        Blocker::SetupIncomplete => "请先在设置中完成云端服务配置",
        Blocker::QueryOnly | Blocker::SessionStopped => "本次尚未开始，点击继续或查看设置",
        Blocker::Maintenance => "正在整理数据，请稍候",
        _ => "正在等待声音来源",
    };
    let appearance = if s.capture_blocker == Blocker::SourceUnavailable {
        Appearance::Interrupted
    } else {
        Appearance::Waiting
    };
    (
        appearance,
        Some(detail.into()),
        if s.capture_blocker == Blocker::SourcesDisabled {
            None
        } else {
            control
        },
    )
}
impl EntryRuntime {
    pub fn new(path: PathBuf) -> Arc<Self> {
        let prefs = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Preferences>(&b).ok())
            .filter(|p| {
                p.schema_version == 1
                    && p.floating.as_ref().is_none_or(|f| {
                        Uuid::parse_str(&f.display_uuid).is_ok()
                            && f.x.is_finite()
                            && f.y.is_finite()
                            && (0.0..=1.0).contains(&f.x)
                            && (0.0..=1.0).contains(&f.y)
                    })
            })
            .unwrap_or_default();
        Arc::new(Self {
            inner: Mutex::new(Inner {
                snapshot: Snapshot {
                    sequence: 0,
                    state: None,
                    error: None,
                    mode: prefs.mode,
                    appearance: Appearance::Waiting,
                    detail: None,
                    control: None,
                    notice: None,
                },
                prefs,
                pending: BTreeMap::new(),
                observed: false,
                armed: true,
                layout_generation: 0,
                canceled_generation: 0,
                view: View::Collapsed,
            }),
            query: Mutex::new(()),
            wake: (Mutex::new(false), Condvar::new()),
            stopped: AtomicBool::new(false),
            path,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }
    pub fn request_refresh(&self) {
        *self.wake.0.lock().unwrap() = true;
        self.wake.1.notify_one();
    }
    pub fn refresh(&self, app: &tauri::AppHandle) {
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let _query = self.query.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let result =
            hear_protocol::send_from(ClientKind::Gui, Request::AppOpened).and_then(|r| match r {
                Response::Status { state, .. } => Ok(state),
                Response::Error { code, message, .. } => Err(format!("{code}: {message}")),
                _ => Err("unexpected status response".into()),
            });
        let snapshot = self.inner.lock().unwrap().observe(result);
        let _ = app.emit("hear-entry-snapshot", snapshot);
    }
    pub fn start(self: &Arc<Self>, app: tauri::AppHandle) {
        let runtime = Arc::clone(self);
        std::thread::spawn(move || {
            while !runtime.stopped.load(Ordering::Acquire) {
                runtime.refresh(&app);
                let pending = runtime.wake.0.lock().unwrap();
                let (mut pending, _) = runtime
                    .wake
                    .1
                    .wait_timeout_while(pending, Duration::from_secs(5), |pending| {
                        !*pending && !runtime.stopped.load(Ordering::Acquire)
                    })
                    .unwrap();
                *pending = false;
            }
        });
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.wake.1.notify_one();
        // Drain a running query before Quit can stop the on-demand owner.
        let _query = self.query.lock().unwrap();
    }
    fn save(&self, prefs: &Preferences) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let parent = self.path.parent().ok_or("missing preference directory")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let pending = self.path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(prefs).map_err(|e| e.to_string())?)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(pending, &self.path).map_err(|e| e.to_string())
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Geometry {
    width: f64,
    height: f64,
    band: f64,
    rabbit_x: f64,
    rabbit_y: f64,
}

async fn native<T: Send + 'static>(
    app: tauri::AppHandle,
    operation: impl FnOnce(&tauri::WebviewWindow, *mut std::ffi::c_void) -> Result<T, String>
    + Send
    + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = handle
            .get_webview_window("float")
            .ok_or("entry unavailable".into())
            .and_then(|w| {
                w.ns_window()
                    .map_err(|e| e.to_string())
                    .and_then(|n| operation(&w, n))
            });
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}
extern "C" fn native_event(kind: i32, value: i32) {
    let Some(app) = HOST.get() else { return };
    if kind == 2 || kind == 3 {
        let runtime = app.state::<Arc<EntryRuntime>>();
        let mut inner = runtime.inner.lock().unwrap();
        inner.canceled_generation = inner.layout_generation;
        inner.view = View::Collapsed;
    }
    if kind == 4 {
        if let Some(w) = app.get_webview_window("float") {
            let _ = w.set_focusable(value != 0);
        }
    }
    let geometry = if kind == 3 || kind == 5 {
        app.get_webview_window("float")
            .and_then(|w| w.ns_window().ok())
            .map(|n| unsafe { hear_entry_geometry(n) })
    } else {
        None
    };
    let _ = app.emit_to(
        "float",
        "hear-entry-native",
        serde_json::json!({"kind":kind,"value":value,"geometry":geometry}),
    );
}
pub fn install(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    runtime: &EntryRuntime,
) -> Result<(), String> {
    let _ = HOST.set(app.clone());
    window
        .set_focusable(unsafe { hear_entry_active() != 0 })
        .map_err(|e| e.to_string())?;
    let pointer = window.ns_window().map_err(|e| e.to_string())?;
    unsafe {
        hear_entry_install(pointer, native_event);
    }
    let inner = runtime.inner.lock().unwrap();
    if let Some(p) = &inner.prefs.floating {
        let uuid = CString::new(p.display_uuid.as_str()).map_err(|e| e.to_string())?;
        unsafe {
            hear_entry_restore(pointer, uuid.as_ptr(), p.x, p.y);
        }
    }
    unsafe {
        hear_entry_layout(
            pointer,
            if inner.prefs.mode == Mode::Top { 0 } else { 1 },
            0,
            0,
            0,
            0,
        );
    }
    Ok(())
}
pub fn dispose(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("float") {
        if let Ok(n) = w.ns_window() {
            unsafe {
                hear_entry_dispose(n);
            }
        }
    }
}

#[tauri::command]
pub fn entry_snapshot(app: tauri::AppHandle, refresh: Option<bool>) -> Snapshot {
    let runtime = app.state::<Arc<EntryRuntime>>();
    if refresh == Some(true) {
        runtime.request_refresh();
    }
    runtime.snapshot()
}
#[tauri::command]
pub async fn entry_mode(app: tauri::AppHandle, mode: Mode) -> Result<(), String> {
    let runtime = app.state::<Arc<EntryRuntime>>();
    {
        let mut inner = runtime.inner.lock().unwrap();
        let mut prefs = inner.prefs.clone();
        prefs.mode = mode;
        runtime.save(&prefs)?;
        inner.prefs = prefs;
        inner.snapshot.mode = mode;
        inner.snapshot.sequence += 1;
        inner.canceled_generation = inner.layout_generation;
        inner.view = View::Collapsed;
    }
    let mode_runtime = Arc::clone(&runtime);
    native(app.clone(), move |w, n| {
        if mode_runtime.inner.lock().unwrap().snapshot.mode != mode {
            return Ok(());
        }
        unsafe {
            hear_entry_focus(n, 0, 1);
            hear_entry_layout(n, if mode == Mode::Top { 0 } else { 1 }, 0, 0, 0, 0);
        }
        w.set_focusable(unsafe { hear_entry_active() != 0 })
            .map_err(|e| e.to_string())
    })
    .await?;
    app.emit("hear-entry-snapshot", runtime.snapshot())
        .map_err(|e| e.to_string())
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Collapsed,
    Panel,
    Comment,
}
#[tauri::command]
pub async fn entry_layout(
    app: tauri::AppHandle,
    view: View,
    feedback: bool,
    detail: bool,
    suppress: bool,
    generation: u64,
) -> Result<Option<Geometry>, String> {
    let runtime = Arc::clone(&app.state::<Arc<EntryRuntime>>());
    let mode = {
        let mut inner = runtime.inner.lock().unwrap();
        if generation < inner.layout_generation
            || (generation <= inner.canceled_generation && view != View::Collapsed)
        {
            return Ok(None);
        }
        inner.layout_generation = generation;
        inner.view = view;
        inner.snapshot.mode
    };
    native(app, move |w, n| {
        let valid = {
            let inner = runtime.inner.lock().unwrap();
            inner.layout_is_current(generation, mode, view)
        };
        if !valid {
            return Ok(None);
        }
        let geometry = unsafe {
            hear_entry_layout(
                n,
                if mode == Mode::Top { 0 } else { 1 },
                match view {
                    View::Collapsed => 0,
                    View::Panel => 1,
                    View::Comment => 2,
                },
                feedback as i32,
                detail as i32,
                suppress as i32,
            )
        };
        if !w.is_visible().map_err(|e| e.to_string())? {
            w.show().map_err(|e| e.to_string())?;
        }
        Ok(Some(geometry))
    })
    .await
}
#[tauri::command]
pub async fn entry_focus(
    app: tauri::AppHandle,
    acquire: bool,
    restore: bool,
    generation: u64,
) -> Result<bool, String> {
    let runtime = Arc::clone(&app.state::<Arc<EntryRuntime>>());
    if !acquire {
        let mut inner = runtime.inner.lock().unwrap();
        if generation < inner.layout_generation {
            return Ok(false);
        }
        inner.layout_generation = generation;
        if inner.view == View::Comment {
            inner.canceled_generation = generation;
        }
        inner.view = View::Collapsed;
    }
    native(app, move |w, n| {
        let valid = {
            let inner = runtime.inner.lock().unwrap();
            if acquire {
                inner.focus_is_current(generation)
            } else {
                generation == inner.layout_generation
            }
        };
        if !valid {
            return Ok(false);
        }
        if acquire {
            w.set_focusable(true).map_err(|e| e.to_string())?;
        }
        let okay = unsafe { hear_entry_focus(n, acquire as i32, restore as i32) };
        if !acquire {
            w.set_focusable(unsafe { hear_entry_active() != 0 })
                .map_err(|e| e.to_string())?;
        }
        Ok(okay != 0)
    })
    .await
}
#[tauri::command]
pub async fn entry_drag(app: tauri::AppHandle, dragging: bool) -> Result<(), String> {
    let saved = native(app.clone(), move |_, n| {
        unsafe {
            hear_entry_drag(n, dragging as i32);
        }
        if dragging {
            Ok(None)
        } else {
            let p = unsafe { hear_entry_position(n) };
            if p.is_null() {
                return Err("cannot read entry position".into());
            }
            let result =
                serde_json::from_slice::<Position>(unsafe { CStr::from_ptr(p) }.to_bytes())
                    .map_err(|e| e.to_string());
            unsafe {
                libc::free(p.cast());
            }
            result.map(Some)
        }
    })
    .await?;
    if let Some(position) = saved {
        let runtime = app.state::<Arc<EntryRuntime>>();
        let mut inner = runtime.inner.lock().unwrap();
        let mut prefs = inner.prefs.clone();
        prefs.floating = Some(position);
        runtime.save(&prefs)?;
        inner.prefs = prefs;
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Mark,
    Comment,
    Screenshot,
    Control,
}
#[derive(Deserialize)]
pub struct Action {
    id: Uuid,
    kind: ActionKind,
    anchor_ms: i64,
    text: Option<String>,
}
#[derive(Serialize)]
pub struct ActionResult {
    outcome: &'static str,
    message: Option<String>,
    response: Option<Response>,
}
fn action_result(response: Response) -> ActionResult {
    match response {
        Response::Error { code, message, .. } => ActionResult {
            outcome: "rejected",
            message: Some(format!("{code}: {message}")),
            response: None,
        },
        response => ActionResult {
            outcome: "confirmed",
            message: None,
            response: Some(response),
        },
    }
}
enum PreparedAction {
    Send(Request),
    Known(Response),
}
impl Inner {
    fn prepare(
        &mut self,
        key: &str,
        action: &Action,
        window_id: Option<u32>,
    ) -> Result<PreparedAction, String> {
        if let Some(p) = self.pending.get(key) {
            if p.in_flight || (p.id != action.id && p.response.is_none()) {
                return Err("action_pending: 请先确认上一次操作的结果".into());
            }
            if p.id == action.id {
                if let Some(response) = &p.response {
                    // Keep the last resolved slot until a new ID replaces it.
                    // The WebView can lose its reply after core has committed;
                    // rebuilding a control request here would change its revision.
                    return Ok(PreparedAction::Known(response.clone()));
                }
            } else {
                self.pending.remove(key);
            }
        }
        if let Some(p) = self.pending.get_mut(key) {
            p.in_flight = true;
            Ok(PreparedAction::Send(p.request.clone()))
        } else {
            let request = match action.kind {
                ActionKind::Mark => Request::AddMoment {
                    kind: MomentKind::Mark,
                    anchor_ms: action.anchor_ms,
                    text: None,
                },
                ActionKind::Comment => Request::AddMoment {
                    kind: MomentKind::Comment,
                    anchor_ms: action.anchor_ms,
                    text: action.text.clone(),
                },
                ActionKind::Screenshot => Request::CaptureScreenshot {
                    excluded_window_ids: vec![window_id.ok_or("入口窗口不可用")?],
                    anchor_ms: Some(action.anchor_ms),
                },
                ActionKind::Control => {
                    let state = self
                        .snapshot
                        .state
                        .as_ref()
                        .ok_or("状态暂不可得，请稍后重试")?;
                    match self.snapshot.control.as_deref() {
                        Some("pause") => Request::Pause {
                            expected_revision: state.revision,
                        },
                        Some("resume") => Request::Resume {
                            expected_revision: state.revision,
                        },
                        Some("start") => Request::Start {
                            expected_revision: state.revision,
                        },
                        _ => return Err("正在切换，请稍后重试".into()),
                    }
                }
            };
            self.pending.insert(
                key.to_owned(),
                Pending {
                    id: action.id,
                    request: request.clone(),
                    in_flight: true,
                    response: None,
                },
            );
            Ok(PreparedAction::Send(request))
        }
    }
}
pub async fn screenshot_window(app: tauri::AppHandle) -> Result<u32, String> {
    native(app, |_, n| Ok(unsafe { hear_entry_window_id(n) })).await
}
#[tauri::command]
pub async fn entry_action(app: tauri::AppHandle, action: Action) -> Result<ActionResult, String> {
    let window_id = if matches!(action.kind, ActionKind::Screenshot) {
        Some(native(app.clone(), |_, n| Ok(unsafe { hear_entry_window_id(n) })).await?)
    } else {
        None
    };
    let runtime = Arc::clone(&app.state::<Arc<EntryRuntime>>());
    let key = match action.kind {
        ActionKind::Mark => "mark",
        ActionKind::Comment => "comment",
        ActionKind::Screenshot => "screenshot",
        ActionKind::Control => "control",
    }
    .to_owned();
    let (prepared, accepted) = {
        let mut inner = runtime.inner.lock().unwrap();
        let prepared = inner.prepare(&key, &action, window_id);
        let accepted = inner.pending.get(&key).is_some_and(|p| p.id == action.id);
        (prepared, accepted)
    };
    let request = match prepared {
        // A competing, unaccepted ID is definitively refused. The same accepted
        // ID still in flight must remain uncertain so its client can confirm it.
        // Neither case may turn a confirmation into a fresh inverse control.
        Err(message) => {
            return Ok(ActionResult {
                outcome: if accepted { "uncertain" } else { "rejected" },
                message: Some(message),
                response: None,
            });
        }
        Ok(PreparedAction::Known(response)) => return Ok(action_result(response)),
        Ok(PreparedAction::Send(request)) => request,
    };
    let id = action.id;
    // All handling, including resetting the slot, belongs to this blocking job.
    // A disconnected WebView cannot leave an action permanently in flight.
    let result = tauri::async_runtime::spawn_blocking(move || {
        let response = hear_protocol::send_from_with_id(ClientKind::Gui, request, id);
        {
            let mut inner = runtime.inner.lock().unwrap();
            if let Some(p) = inner.pending.get_mut(&key) {
                p.in_flight = false;
                p.response = response.as_ref().ok().cloned();
            }
        }
        runtime.request_refresh();
        match response {
            Ok(response) => {
                if matches!(response, Response::MomentSaved { .. }) {
                    let _ = app.emit("hear-moment-changed", ());
                }
                action_result(response)
            }
            Err(message) => ActionResult {
                outcome: "uncertain",
                message: Some(format!("结果尚未确认；再次点击可确认原操作（{message}）")),
                response: None,
            },
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> ServiceState {
        serde_json::from_value(serde_json::json!({"revision":1,"enabled_intent":true,"paused_intent":false,"storage_paused":false,"microphone_enabled":true,"system_enabled":true,"capture_state":"recording","reason":null,"microphone_recording":true,"system_recording":true,"microphone_reason":null,"system_reason":null,"microphone_phase":"recording","system_phase":"recording","capture_blocker":"none"})).unwrap()
    }
    #[test]
    fn mode_change_invalidates_even_an_old_collapsed_layout() {
        let runtime = EntryRuntime::new(
            std::env::temp_dir().join(format!("hear-entry-{}.json", Uuid::new_v4())),
        );
        let mut inner = runtime.inner.lock().unwrap();
        inner.layout_generation = 3;
        assert!(inner.layout_is_current(3, Mode::Top, View::Collapsed));
        inner.snapshot.mode = Mode::Floating;
        inner.canceled_generation = 3;
        assert!(!inner.layout_is_current(3, Mode::Top, View::Collapsed));
        assert!(!inner.layout_is_current(3, Mode::Floating, View::Comment));
        inner.layout_generation = 4;
        assert!(inner.layout_is_current(4, Mode::Floating, View::Comment));
    }
    #[test]
    fn comment_focus_intent_survives_layout_but_never_a_cancellation() {
        let runtime = EntryRuntime::new(
            std::env::temp_dir().join(format!("hear-entry-{}.json", Uuid::new_v4())),
        );
        let mut inner = runtime.inner.lock().unwrap();
        inner.layout_generation = 4;
        inner.view = View::Comment;
        assert!(inner.focus_is_current(3));
        inner.canceled_generation = 4;
        // Even if a delayed background render has registered a newer layout,
        // its original user-focus intent remains canceled.
        inner.layout_generation = 5;
        assert!(!inner.focus_is_current(3));
        assert!(!inner.focus_is_current(4));
        assert!(inner.focus_is_current(5));
        inner.view = View::Collapsed;
        assert!(!inner.focus_is_current(5));
    }
    #[test]
    fn disabled_missing_source_is_healthy_but_enabled_missing_is_partial() {
        let mut s = state();
        s.system_recording = false;
        s.system_phase = Phase::Unavailable;
        assert_eq!(presentation(Some(&s)).0, Appearance::Partial);
        s.system_enabled = false;
        s.system_phase = Phase::Disabled;
        assert_eq!(presentation(Some(&s)).0, Appearance::Listening);
    }
    #[test]
    fn source_startup_waits_and_does_not_claim_a_failure_or_rearm_alerts() {
        let mut s = state();
        s.system_recording = false;
        s.system_phase = Phase::Starting;
        assert_eq!(presentation(Some(&s)).0, Appearance::Waiting);
        assert_eq!(presentation(Some(&s)).2.as_deref(), Some("pause"));
    }
    #[test]
    fn lost_core_or_webview_reply_keeps_original_control_revision() {
        let runtime = EntryRuntime::new(
            std::env::temp_dir().join(format!("hear-entry-{}.json", Uuid::new_v4())),
        );
        let mut inner = runtime.inner.lock().unwrap();
        inner.observe(Ok(state()));
        let action = Action {
            id: Uuid::new_v4(),
            kind: ActionKind::Control,
            anchor_ms: 100,
            text: None,
        };
        assert!(matches!(
            inner.prepare("control", &action, None).unwrap(),
            PreparedAction::Send(Request::Pause {
                expected_revision: 1
            })
        ));
        assert!(inner.prepare("control", &action, None).is_err());
        // A transport failure permits retry but changes neither request nor ID.
        inner.pending.get_mut("control").unwrap().in_flight = false;
        inner.snapshot.state.as_mut().unwrap().revision = 9;
        assert!(matches!(
            inner.prepare("control", &action, None).unwrap(),
            PreparedAction::Send(Request::Pause {
                expected_revision: 1
            })
        ));
        let confirmed = Response::Status {
            state: state(),
            id: action.id,
        };
        let slot = inner.pending.get_mut("control").unwrap();
        slot.in_flight = false;
        slot.response = Some(confirmed);
        assert!(matches!(
            inner.prepare("control", &action, None).unwrap(),
            PreparedAction::Known(_)
        ));
        let next = Action {
            id: Uuid::new_v4(),
            ..action
        };
        assert!(matches!(
            inner.prepare("control", &next, None).unwrap(),
            PreparedAction::Send(Request::Pause {
                expected_revision: 9
            })
        ));
    }
    #[test]
    fn uncertain_save_blocks_new_identity_but_does_not_block_pause() {
        let runtime = EntryRuntime::new(
            std::env::temp_dir().join(format!("hear-entry-{}.json", Uuid::new_v4())),
        );
        let mut inner = runtime.inner.lock().unwrap();
        inner.observe(Ok(state()));
        let first = Action {
            id: Uuid::new_v4(),
            kind: ActionKind::Comment,
            anchor_ms: 100,
            text: Some("original".into()),
        };
        inner.prepare("comment", &first, None).unwrap();
        inner.pending.get_mut("comment").unwrap().in_flight = false;
        let changed = Action {
            id: first.id,
            kind: ActionKind::Comment,
            anchor_ms: 200,
            text: Some("changed".into()),
        };
        assert!(
            matches!(inner.prepare("comment", &changed, None).unwrap(), PreparedAction::Send(Request::AddMoment { anchor_ms: 100, text: Some(t), .. }) if t == "original")
        );
        inner.pending.get_mut("comment").unwrap().in_flight = false;
        let new = Action {
            id: Uuid::new_v4(),
            ..changed
        };
        assert!(inner.prepare("comment", &new, None).is_err());
        let pause = Action {
            id: Uuid::new_v4(),
            kind: ActionKind::Control,
            anchor_ms: 200,
            text: None,
        };
        assert!(matches!(
            inner.prepare("control", &pause, None).unwrap(),
            PreparedAction::Send(Request::Pause { .. })
        ));
    }
    #[test]
    fn only_actual_manual_pause_sleeps_and_unknown_never_stays_healthy() {
        let mut s = state();
        s.paused_intent = true;
        s.microphone_phase = Phase::Stopping;
        s.system_phase = Phase::Stopping;
        assert_eq!(presentation(Some(&s)).0, Appearance::Waiting);
        s.microphone_recording = false;
        s.system_recording = false;
        s.microphone_phase = Phase::Idle;
        s.system_phase = Phase::Idle;
        assert_eq!(presentation(Some(&s)).0, Appearance::Sleeping);
        s.storage_paused = true;
        assert_eq!(presentation(Some(&s)).0, Appearance::Interrupted);
        assert_eq!(presentation(None).0, Appearance::Waiting);
    }
    #[test]
    fn fault_reminds_once_and_only_normal_recovery_rearms() {
        let runtime = EntryRuntime::new(
            std::env::temp_dir().join(format!("hear-entry-{}.json", Uuid::new_v4())),
        );
        let mut inner = runtime.inner.lock().unwrap();
        let mut fault = state();
        fault.microphone_recording = false;
        fault.system_recording = false;
        fault.microphone_phase = Phase::Unavailable;
        fault.system_phase = Phase::Unavailable;
        fault.capture_blocker = Blocker::SourceUnavailable;
        assert!(inner.observe(Ok(fault.clone())).notice.is_none());
        assert!(inner.observe(Ok(state())).notice.is_none());
        assert!(inner.observe(Ok(fault.clone())).notice.is_some());
        assert!(inner.observe(Ok(fault.clone())).notice.is_none());
        assert!(inner.observe(Err("unreachable".into())).notice.is_none());
        let mut partial = state();
        partial.system_recording = false;
        partial.system_phase = Phase::Unavailable;
        assert!(inner.observe(Ok(partial)).notice.is_none());
        assert!(inner.observe(Ok(fault.clone())).notice.is_none());
        inner.observe(Ok(state()));
        assert!(inner.observe(Ok(fault)).notice.is_some());
    }
    #[test]
    fn unknown_initial_state_does_not_make_startup_fault_a_new_alert() {
        let runtime = EntryRuntime::new(
            std::env::temp_dir().join(format!("hear-entry-{}.json", Uuid::new_v4())),
        );
        let mut inner = runtime.inner.lock().unwrap();
        inner.observe(Err("unreachable".into()));
        let mut s = state();
        s.storage_paused = true;
        s.microphone_recording = false;
        s.system_recording = false;
        assert!(inner.observe(Ok(s)).notice.is_none());
    }
}
