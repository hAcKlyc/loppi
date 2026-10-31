//! Internal resource admission; never changes user recording intent or retention.
//! OS pressure and measured process footprints complement fixed input bounds.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
/// Fixed lifetime histograms (milliseconds), with no growing sample buffer.
#[derive(Default)]
pub struct Histogram([AtomicU64; 8]);
pub struct Timer<'a>(&'a Histogram, Instant);
impl Histogram {
    pub fn observe(&self, elapsed: Duration) {
        let bucket = [1, 5, 10, 50, 100, 500, 1000]
            .iter()
            .position(|v| elapsed.as_millis() <= *v)
            .unwrap_or(7);
        self.0[bucket].fetch_add(1, Ordering::Relaxed);
    }
    pub fn timer(&self) -> Timer<'_> {
        Timer(self, Instant::now())
    }
    pub fn counts(&self) -> [u64; 8] {
        std::array::from_fn(|i| self.0[i].load(Ordering::Relaxed))
    }
}
impl Drop for Timer<'_> {
    fn drop(&mut self) {
        self.0.observe(self.1.elapsed());
    }
}

const MIB: u64 = 1024 * 1024;
#[repr(C)]
#[derive(Default, Clone, Copy, serde::Serialize)]
pub struct Native {
    pub memory_pressure: u32,
    pub thermal: u32,
    pub low_power: u32,
    pub on_battery: u32,
    pub physical_memory: u64,
}
unsafe extern "C" {
    fn loppi_resource_snapshot(out: *mut Native);
    fn loppi_process_footprint(pid: i32) -> u64;
    fn loppi_background_footprint() -> u64;
}
pub fn native() -> Native {
    let mut out = Native::default();
    unsafe { loppi_resource_snapshot(&mut out) };
    out
}
#[repr(C)]
#[derive(Default, Clone, Copy, serde::Serialize)]
pub struct Power {
    pub available: u32,
    pub sleeping: u32,
    pub generation: u64,
    pub sleep_ms: i64,
}
pub fn power() -> Power {
    unsafe extern "C" {
        fn loppi_power_snapshot(out: *mut Power);
    }
    let mut out = Power::default();
    unsafe {
        loppi_power_snapshot(&mut out);
    }
    out
}
pub fn footprint(pid: u32) -> u64 {
    unsafe { loppi_process_footprint(pid as i32) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Constrained,
    Critical,
    Recovering,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    Coarse = 0,
    Voice = 1,
    Llm = 2,
    Asr = 3,
    Export = 4,
}
impl Work {
    fn peak(self) -> u64 {
        match self {
            Self::Coarse => 640 * MIB,
            Self::Voice => 256 * MIB,
            Self::Llm | Self::Asr => 48 * MIB,
            Self::Export => 32 * MIB,
        }
    }
}
#[derive(Default, Clone, Copy, serde::Serialize)]
pub struct Backlog {
    pub vad_seconds: u64,
    pub coarse_seconds: u64,
    pub oldest_vad_ms: u64,
    pub oldest_coarse_ms: u64,
    pub voice_pending: u64,
    pub oldest_voice_ms: u64,
    pub voice_last_success_ms: u64,
    pub disk_free: u64,
    pub store_wait_ms: u64,
}
#[derive(serde::Serialize)]
struct CaptureView {
    session: uuid::Uuid,
    pid: u32,
    frames: u64,
    active: bool,
    metrics: Option<crate::capture::MetricsView>,
}
struct State {
    mode: Mode,
    since: Instant,
    healthy: Option<Instant>,
    pressured: Option<Instant>,
    total: u64,
    soft: u64,
    hard: u64,
    low_power: bool,
    on_battery: bool,
    busy: [u64; 5],
    models: [Option<(u32, u64)>; 2],
    waiting: [Option<Instant>; 2],
    last_admission: [Instant; 5],
    backlog: Backlog,
    os: Native,
    peak: u64,
    transitions: u64,
    latency_ms: u64,
    capture_lag_ms: u64,
    captures: [Option<CaptureView>; 2],
}
pub struct Governor {
    state: Mutex<State>,
    origin: Instant,
    pub request_latency_ms: AtomicU64,
    pub request_histogram: Histogram,
    capture: [AtomicU64; 2],
}
impl Governor {
    pub fn new() -> Arc<Self> {
        let now = Instant::now();
        let physical = native().physical_memory;
        let small = physical < 16 * 1024 * MIB;
        Arc::new(Self {
            state: Mutex::new(State {
                mode: Mode::Normal,
                since: now,
                healthy: None,
                pressured: None,
                total: 0,
                soft: if small { 768 * MIB } else { 1024 * MIB },
                hard: if small { 1024 * MIB } else { 1536 * MIB },
                low_power: false,
                on_battery: false,
                busy: [0; 5],
                models: [None; 2],
                waiting: [None; 2],
                last_admission: [now - Duration::from_secs(60); 5],
                backlog: Backlog::default(),
                os: Native::default(),
                peak: 0,
                transitions: 0,
                latency_ms: 0,
                capture_lag_ms: 0,
                captures: [None, None],
            }),
            origin: now,
            request_latency_ms: AtomicU64::new(0),
            request_histogram: Histogram::default(),
            capture: [AtomicU64::new(0), AtomicU64::new(0)],
        })
    }
    pub fn capture_progress(&self, source: &str) {
        self.capture[usize::from(source == "system")].store(
            self.origin.elapsed().as_millis() as u64 + 1,
            Ordering::Release,
        );
    }
    pub fn capture_observed(
        &self,
        source: &str,
        session: uuid::Uuid,
        pid: u32,
        progress: &crate::source_process::Progress,
    ) {
        let i = usize::from(source == "system");
        let mut state = self.state.lock().unwrap();
        if state.captures[i]
            .as_ref()
            .is_none_or(|v| v.session != session || progress.frames > v.frames)
            && progress.frames > 0
        {
            self.capture_progress(source);
        }
        state.captures[i] = Some(CaptureView {
            session,
            pid,
            frames: progress.frames,
            active: true,
            metrics: progress.capture.clone().or_else(|| {
                state.captures[i]
                    .as_ref()
                    .filter(|v| v.session == session && v.pid == pid)
                    .and_then(|v| v.metrics.clone())
            }),
        });
    }
    pub fn diagnostics(&self) -> serde_json::Value {
        let s = self.state.lock().unwrap();
        let voice_deferred = if s.mode == Mode::Critical {
            Some("critical")
        } else if !s.heavy_eligible(1) {
            Some("vad_priority")
        } else if s.busy[1] != 0 {
            Some("running")
        } else if s.total.saturating_add(
            Work::Voice
                .peak()
                .saturating_sub(s.models[1].map(|m| m.1).unwrap_or(0)),
        ) > s.soft
        {
            Some("memory_budget")
        } else {
            None
        };
        serde_json::json!({"scope":"core_and_direct_children", "mode":format!("{:?}",s.mode),
            "footprint_bytes":s.total,"peak_footprint_bytes":s.peak,"soft_bytes":s.soft,"hard_bytes":s.hard,
            "request_histogram_ms":self.request_histogram.counts(),"transitions":s.transitions,"request_max_ms":s.latency_ms,"capture_lag_ms":s.capture_lag_ms,
            "reserved_bytes":s.busy,"model_pids":s.models.map(|v|v.map(|m|m.0)),
            "os":s.os,"voice_deferred_reason":voice_deferred,"backlog_lower_bound":s.backlog,"capture":s.captures})
    }
    pub fn capture_metrics(
        &self,
        source: &str,
        session: uuid::Uuid,
    ) -> Option<crate::capture::MetricsView> {
        self.state.lock().unwrap().captures[usize::from(source == "system")]
            .as_ref()
            .filter(|v| v.session == session)
            .and_then(|v| v.metrics.clone())
    }
    pub fn playback_excluded(&self, generation: u64) -> bool {
        self.state.lock().unwrap().captures[1]
            .as_ref()
            .is_some_and(|v| {
                v.active
                    && v.metrics
                        .as_ref()
                        .is_some_and(|m| m.exclusion_generation == generation && generation != 0)
            })
    }
    pub fn capture_stopped(&self, source: &str) {
        self.capture[usize::from(source == "system")].store(0, Ordering::Release);
        if let Some(view) =
            self.state.lock().unwrap().captures[usize::from(source == "system")].as_mut()
        {
            view.active = false;
        }
    }
    pub fn sample(&self, backlog: Backlog) {
        let os = native();
        let total = unsafe { loppi_background_footprint() };
        let mut state = self.state.lock().unwrap();
        for model in state.models.iter_mut().flatten() {
            model.1 = footprint(model.0);
        }
        let elapsed = self.origin.elapsed().as_millis() as u64 + 1;
        let capture_lag = self
            .capture
            .iter()
            .map(|v| v.load(Ordering::Acquire))
            .filter(|v| *v != 0)
            .map(|v| elapsed.saturating_sub(v))
            .max()
            .unwrap_or(0);
        let latency = self.request_latency_ms.swap(0, Ordering::AcqRel);
        let critical = os.memory_pressure >= 2
            || os.thermal >= 2
            || total > state.hard
            || capture_lag > 10_000;
        let pressure = os.memory_pressure >= 1
            || os.thermal >= 1
            || total > state.soft
            || capture_lag > 6500
            || latency > 500
            || backlog.store_wait_ms > 500
            || backlog.disk_free < 2 * 1024 * MIB;
        let previous = state.mode;
        state.advance(Instant::now(), pressure, critical);
        state.transitions += u64::from(previous != state.mode);
        state.peak = state.peak.max(total);
        state.os = os;
        state.latency_ms = latency;
        state.capture_lag_ms = capture_lag;
        state.total = total;
        state.low_power = os.low_power != 0;
        state.on_battery = os.on_battery != 0;
        state.backlog = backlog;
    }
    pub fn mode(&self) -> Mode {
        self.state.lock().unwrap().mode
    }
    pub fn try_admit(self: &Arc<Self>, work: Work) -> Option<Permit> {
        let mut s = self.state.lock().unwrap();
        let i = work as usize;
        if s.busy[i] != 0 || s.mode == Mode::Critical {
            return None;
        }
        if i < 2 && !s.heavy_eligible(i) {
            s.waiting[i] = None;
            return None;
        }
        if s.mode != Mode::Normal && work == Work::Export {
            return None;
        }
        if s.mode != Mode::Normal
            && matches!(work, Work::Llm | Work::Asr)
            && s.busy[if i == 2 { 3 } else { 2 }] != 0
        {
            return None;
        }
        if i < 2 {
            s.waiting[i].get_or_insert_with(Instant::now);
            if s.can_handoff(i)
                && s.waiting[1 - i].is_some_and(|other| {
                    other.elapsed() >= Duration::from_secs(1) && other < s.waiting[i].unwrap()
                })
            {
                return None;
            }
        }
        let interval = if s.mode == Mode::Constrained {
            5
        } else if s.mode == Mode::Recovering || s.low_power || s.on_battery {
            1
        } else {
            0
        };
        if s.last_admission[i].elapsed() < Duration::from_secs(interval) {
            return None;
        }
        // On small machines the two heavy models cannot compute concurrently.
        if (s.soft <= 768 * MIB || s.mode != Mode::Normal) && i < 2 && s.busy[1 - i] != 0 {
            return None;
        }
        let resident = if i < 2 {
            s.models[i].map(|m| m.1).unwrap_or(0)
        } else {
            0
        };
        let reserve = work.peak().saturating_sub(resident).max(1);
        if s.total
            .saturating_add(s.busy.iter().sum::<u64>())
            .saturating_add(reserve)
            > if work == Work::Export {
                s.hard.saturating_sub(128 * MIB)
            } else {
                s.soft
            }
        {
            return None;
        }
        if i < 2 {
            s.waiting[i] = None;
        }
        s.busy[i] = reserve;
        s.last_admission[i] = Instant::now();
        Some(Permit {
            governor: Arc::clone(self),
            work,
        })
    }
    pub fn wait_admit(self: &Arc<Self>, work: Work, timeout: Duration) -> Result<Permit, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(permit) = self.try_admit(work) {
                return Ok(permit);
            }
            if Instant::now() >= deadline {
                return Err("resources are busy; retry when background processing settles".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn register(self: &Arc<Self>, work: Work, pid: u32) -> Resident {
        assert!((work as usize) < 2);
        self.state.lock().unwrap().models[work as usize] = Some((pid, footprint(pid)));
        Resident {
            governor: Arc::clone(self),
            work,
            pid,
        }
    }
    pub fn cancel(&self, work: Work) -> bool {
        let s = self.state.lock().unwrap();
        s.mode == Mode::Critical
            || ((work as usize) < 2
                && s.models[work as usize].is_some_and(|(_, bytes)| {
                    bytes
                        > match work {
                            Work::Coarse => 768 * MIB,
                            _ => 384 * MIB,
                        }
                }))
    }
    pub fn no_work(&self, work: Work) {
        if (work as usize) < 2 {
            self.state.lock().unwrap().waiting[work as usize] = None;
        }
    }
    pub fn unload(&self, work: Work) -> bool {
        let s = self.state.lock().unwrap();
        let i = work as usize;
        if s.mode == Mode::Critical
            || (s.mode != Mode::Normal && s.total > s.soft && s.busy[i] == 0)
        {
            return true;
        }
        i < 2
            && s.busy[i] == 0
            && (s.models[i].is_some_and(|(_, bytes)| {
                bytes
                    > match work {
                        Work::Coarse => 768 * MIB,
                        _ => 384 * MIB,
                    }
            }) || (s.can_handoff(i)
                && s.waiting[1 - i].is_some_and(|at| at.elapsed() >= Duration::from_secs(1))))
    }
}
impl State {
    fn can_handoff(&self, from: usize) -> bool {
        let to = 1 - from;
        if !self.heavy_eligible(to) || self.busy[from] != 0 || self.busy[to] != 0 {
            return false;
        }
        let releasable = self.models[from].map(|m| m.1).unwrap_or(0);
        let resident = self.models[to].map(|m| m.1).unwrap_or(0);
        self.total
            .saturating_sub(releasable)
            .saturating_add(self.busy.iter().sum::<u64>())
            .saturating_add(
                if to == 0 { Work::Coarse } else { Work::Voice }
                    .peak()
                    .saturating_sub(resident)
                    .max(1),
            )
            <= self.soft
    }
    fn heavy_eligible(&self, index: usize) -> bool {
        self.mode != Mode::Critical
            && (index == 0
                || !(self.backlog.vad_seconds > 60 && self.backlog.oldest_vad_ms > 60_000)
                || self.last_admission[index].elapsed() >= Duration::from_secs(60))
    }
    fn advance(&mut self, now: Instant, pressure: bool, critical: bool) {
        if critical {
            self.mode = Mode::Critical;
            self.since = now;
            self.healthy = None;
            self.pressured = None;
        } else if pressure {
            self.healthy = None;
            if now.duration_since(*self.pressured.get_or_insert(now)) >= Duration::from_secs(10)
                && self.mode != Mode::Critical
            {
                self.mode = Mode::Constrained;
                self.since = now;
            }
        } else {
            self.pressured = None;
            if self.mode != Mode::Normal
                && now.duration_since(*self.healthy.get_or_insert(now)) >= Duration::from_secs(60)
            {
                self.mode = if self.mode == Mode::Recovering {
                    Mode::Normal
                } else {
                    Mode::Recovering
                };
                self.since = now;
                self.healthy = Some(now);
            }
        }
    }
}
pub struct Permit {
    governor: Arc<Governor>,
    work: Work,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.governor.state.lock().unwrap().busy[self.work as usize] = 0;
    }
}
pub struct Resident {
    governor: Arc<Governor>,
    work: Work,
    pid: u32,
}
impl Drop for Resident {
    fn drop(&mut self) {
        let mut s = self.governor.state.lock().unwrap();
        if s.models[self.work as usize].is_some_and(|m| m.0 == self.pid) {
            s.models[self.work as usize] = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_observations_cannot_inherit_another_session_or_process_metrics() {
        let governor = Governor::new();
        let session = uuid::Uuid::new_v4();
        let mut progress = crate::source_process::Progress::default();
        progress.capture = Some(crate::capture::MetricsView {
            callback_frames: 48000,
            dropped_frames: 1024,
            ..Default::default()
        });
        governor.capture_observed("microphone", session, 42, &progress);
        progress.capture = None;
        governor.capture_observed("microphone", session, 42, &progress);
        assert_eq!(
            governor.diagnostics()["capture"][0]["metrics"]["dropped_frames"],
            1024
        );
        let next_session = uuid::Uuid::new_v4();
        governor.capture_observed("microphone", next_session, 42, &progress);
        assert!(governor.diagnostics()["capture"][0]["metrics"].is_null());
        progress.capture = Some(crate::capture::MetricsView::default());
        governor.capture_observed("microphone", next_session, 42, &progress);
        progress.capture = None;
        governor.capture_observed("microphone", next_session, 43, &progress);
        governor.capture_stopped("microphone");
        let snapshot = governor.diagnostics();
        assert!(snapshot["capture"][0]["metrics"].is_null());
        assert_eq!(snapshot["capture"][0]["active"], false);
    }
    #[test]
    fn pressure_has_hysteresis_and_critical_cancels_immediately() {
        let governor = Governor::new();
        let now = Instant::now();
        let mut state = governor.state.lock().unwrap();
        state.advance(now, true, false);
        assert_eq!(state.mode, Mode::Normal);
        state.advance(now + Duration::from_secs(9), true, false);
        assert_eq!(state.mode, Mode::Normal);
        state.advance(now + Duration::from_secs(10), true, false);
        assert_eq!(state.mode, Mode::Constrained);
        state.advance(now + Duration::from_secs(11), false, true);
        assert_eq!(state.mode, Mode::Critical);
        state.advance(now + Duration::from_secs(12), false, false);
        state.advance(now + Duration::from_secs(71), false, false);
        assert_eq!(state.mode, Mode::Critical);
        state.advance(now + Duration::from_secs(72), false, false);
        assert_eq!(state.mode, Mode::Recovering);
        state.advance(now + Duration::from_secs(132), false, false);
        assert_eq!(state.mode, Mode::Normal);
    }
    #[test]
    fn budget_reserves_peaks_and_releases_without_recording_mutation() {
        let governor = Governor::new();
        {
            let mut state = governor.state.lock().unwrap();
            state.soft = 768 * MIB;
            state.hard = 1024 * MIB;
            state.total = 100 * MIB;
        }
        let coarse = governor.try_admit(Work::Coarse).unwrap();
        assert!(governor.try_admit(Work::Voice).is_none());
        assert!(governor.try_admit(Work::Coarse).is_none());
        drop(coarse);
        let voice = governor.try_admit(Work::Voice).unwrap();
        drop(voice);
        governor.state.lock().unwrap().mode = Mode::Critical;
        assert!(governor.try_admit(Work::Coarse).is_none());
        assert!(governor.cancel(Work::Coarse));
        assert!(
            governor
                .capture
                .iter()
                .all(|v| v.load(Ordering::Acquire) == 0)
        );
    }
    #[test]
    fn constrained_cloud_admission_preserves_single_slot_and_backlog_prioritizes_vad() {
        let governor = Governor::new();
        governor.state.lock().unwrap().mode = Mode::Constrained;
        let llm = governor.try_admit(Work::Llm).unwrap();
        assert!(governor.try_admit(Work::Asr).is_none());
        drop(llm);
        assert!(governor.try_admit(Work::Asr).is_some());
        let mut state = governor.state.lock().unwrap();
        state.mode = Mode::Normal;
        state.backlog.vad_seconds = 61;
        state.backlog.oldest_vad_ms = 61000;
        state.last_admission[1] = Instant::now();
        drop(state);
        assert!(governor.try_admit(Work::Voice).is_none());
        assert!(governor.try_admit(Work::Coarse).is_some());
    }
    #[test]
    fn voice_makes_bounded_progress_in_safe_degraded_modes_and_yields_to_critical() {
        let g = Governor::new();
        for mode in [Mode::Normal, Mode::Constrained, Mode::Recovering] {
            {
                let mut s = g.state.lock().unwrap();
                s.mode = mode;
                s.total = 64 * MIB;
                s.last_admission[1] = Instant::now() - Duration::from_secs(61);
                s.backlog.vad_seconds = 120;
                s.backlog.oldest_vad_ms = 120_000;
            }
            let permit = g
                .try_admit(Work::Voice)
                .expect("aged voice work gets a bounded opportunity");
            assert!(!g.unload(Work::Voice));
            drop(permit);
            assert!(
                g.try_admit(Work::Voice).is_none(),
                "VAD retains priority between opportunities"
            );
        }
        g.state.lock().unwrap().mode = Mode::Critical;
        assert!(g.try_admit(Work::Voice).is_none());
        assert!(g.unload(Work::Voice));
        {
            let mut s = g.state.lock().unwrap();
            s.mode = Mode::Constrained;
            s.total = s.soft;
            s.last_admission[1] = Instant::now() - Duration::from_secs(61);
        }
        assert!(
            g.try_admit(Work::Voice).is_none(),
            "age cannot bypass the memory budget"
        );
    }

    #[test]
    fn waiting_model_gets_compute_boundary_and_ineligible_work_cannot_block() {
        let g = Governor::new();
        let coarse = g.try_admit(Work::Coarse).unwrap();
        {
            let mut s = g.state.lock().unwrap();
            s.soft = 768 * MIB;
            s.waiting[1] = Some(Instant::now() - Duration::from_secs(2));
        }
        assert!(!g.unload(Work::Coarse)); // Never interrupt active ordinary inference.
        drop(coarse);
        assert!(g.unload(Work::Coarse));
        assert!(g.try_admit(Work::Coarse).is_none());
        let voice = g.try_admit(Work::Voice).unwrap();
        drop(voice);
        assert!(g.try_admit(Work::Coarse).is_some());
        {
            let mut s = g.state.lock().unwrap();
            s.waiting[1] = Some(Instant::now() - Duration::from_secs(2));
            s.backlog.vad_seconds = 61;
            s.backlog.oldest_vad_ms = 61_000;
        }
        assert!(!g.unload(Work::Coarse));
        assert!(g.try_admit(Work::Coarse).is_some());
        g.no_work(Work::Voice);
        assert!(g.state.lock().unwrap().waiting[1].is_none());
        g.state.lock().unwrap().mode = Mode::Constrained;
        assert!(g.try_admit(Work::Voice).is_none());
        assert!(g.state.lock().unwrap().waiting[1].is_none());
    }
    #[test]
    fn oversized_waiter_does_not_starve_smaller_work_but_idle_model_can_make_room() {
        let g = Governor::new();
        {
            let mut s = g.state.lock().unwrap();
            s.soft = 768 * MIB;
            s.total = 200 * MIB;
        }
        assert!(g.try_admit(Work::Coarse).is_none());
        g.state.lock().unwrap().waiting[0] = Some(Instant::now() - Duration::from_secs(2));
        assert!(!g.unload(Work::Voice));
        assert!(g.try_admit(Work::Voice).is_some());
        // If the idle voice model accounts for enough of the footprint, its
        // release makes the older coarse job runnable and must yield to it.
        g.state.lock().unwrap().models[1] = Some((0, 114 * MIB));
        assert!(g.unload(Work::Voice));
        assert!(g.try_admit(Work::Voice).is_none());
    }
    #[test]
    fn native_resource_observation_reports_this_process() {
        let observed = native();
        assert!(observed.physical_memory > 0);
        assert!(observed.thermal <= 3);
        assert!(observed.memory_pressure <= 2);
        assert!(footprint(std::process::id()) > 0);
    }
}
