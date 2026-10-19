mod diagnostic;
pub use diagnostic::*;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 18;
const MAX_RESPONSE_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    pub id: Uuid,
    pub client: ClientKind,
    pub request: Request,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Core,
    Cli,
    Gui,
    Worker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Status,
    Diagnose {
        filter: DiagnosticFilter,
    },
    Models,
    PrepareModels,
    Probe {
        source: AudioSource,
        seconds: u8,
    },
    RecordTest {
        source: AudioSource,
        seconds: u8,
    },
    Start {
        expected_revision: i64,
    },
    Pause {
        expected_revision: i64,
    },
    Resume {
        expected_revision: i64,
    },
    SetSource {
        source: AudioSource,
        enabled: bool,
        expected_revision: i64,
    },
    SetAudioLimit {
        limit_bytes: u64,
        expected_revision: i64,
    },
    CloudStatus,
    UsageStatistics {
        boundaries: Vec<i64>,
    },
    CloudTasks {
        provider: CloudProvider,
        limit: u16,
    },
    ConfigureCloud {
        provider: CloudProvider,
        api_key: String,
        model: Option<String>,
    },
    SetCloudPaused {
        provider: CloudProvider,
        paused: bool,
    },
    RetryCloudTask {
        task_id: Uuid,
    },
    ProcessNow,
    Timeline {
        from_ms: i64,
        to_ms: i64,
    },
    CalendarDays {
        boundaries: Vec<i64>,
    },
    EventDetail {
        event_id: Uuid,
        offset: u32,
        limit: u16,
    },
    RangeDetail {
        started_ms: i64,
        ended_ms: i64,
        sources: EventSources,
        offset: u32,
        limit: u16,
    },
    PlaybackChunk {
        from_ms: i64,
        to_ms: i64,
        sources: EventSources,
    },
    PlaybackValid {
        from_ms: i64,
        to_ms: i64,
        sources: EventSources,
        generation: i64,
    },
    CreateEvent {
        started_ms: i64,
        ended_ms: i64,
        title: String,
        sources: EventSources,
    },
    EditEvent {
        event_id: Uuid,
        expected_revision: i64,
        started_ms: i64,
        ended_ms: i64,
        title: String,
        sources: Option<EventSources>,
    },
    RetranscribeEvent {
        event_id: Uuid,
        expected_revision: i64,
    },
    DeleteEvent {
        event_id: Uuid,
        expected_revision: i64,
    },
    SplitEvent {
        event_id: Uuid,
        expected_revision: i64,
        at_ms: i64,
        left_title: String,
        right_title: String,
    },
    MergeEvents {
        left_id: Uuid,
        left_revision: i64,
        right_id: Uuid,
        right_revision: i64,
        title: String,
    },
    TranscribeRange {
        started_ms: i64,
        ended_ms: i64,
        sources: EventSources,
    },
    CorrectText {
        segment_id: Uuid,
        text: String,
        expected_revision: i64,
    },
    RevertText {
        segment_id: Uuid,
        expected_revision: i64,
    },
    ObjectRevision {
        kind: String,
        object_id: Uuid,
    },
    SessionSpeakers {
        segment_id: Uuid,
    },
    AssignLocalSpeaker {
        segment_id: Uuid,
        target_segment_id: Uuid,
        entire_local_speaker: bool,
        expected_revision: i64,
        target_revision: i64,
    },
    People,
    CreatePerson {
        display_name: String,
    },
    CreateAndBindSpeaker {
        segment_id: Uuid,
        display_name: String,
        entire_local_speaker: bool,
        expected_revision: i64,
    },
    RenamePerson {
        person_id: Uuid,
        display_name: String,
        expected_revision: i64,
    },
    DeletePerson {
        person_id: Uuid,
        expected_revision: i64,
    },
    BindSpeaker {
        segment_id: Uuid,
        person_id: Uuid,
        entire_local_speaker: bool,
        expected_revision: i64,
    },
    UnbindSpeaker {
        segment_id: Uuid,
        entire_local_speaker: bool,
        expected_revision: i64,
    },
    AddVoiceSample {
        segment_id: Uuid,
        expected_revision: i64,
    },
    RemoveVoiceSample {
        sample_id: Uuid,
        expected_revision: i64,
    },
    VoiceSamples {
        person_id: Uuid,
    },
    ReviewItems,
    ResolveReview {
        item_id: Uuid,
        target_segment_id: Option<Uuid>,
        expected_revision: i64,
    },
    Search {
        query: Option<String>,
        from_ms: i64,
        to_ms: i64,
        person_id: Option<Uuid>,
        offset: u32,
        limit: u16,
    },
    Moments {
        from_ms: i64,
        to_ms: i64,
        offset: u32,
        limit: u16,
    },
    AddMoment {
        kind: MomentKind,
        anchor_ms: i64,
        text: Option<String>,
    },
    CaptureScreenshot {
        #[serde(default)]
        excluded_window_ids: Vec<u32>,
        #[serde(default)]
        anchor_ms: Option<i64>,
    },
    ScreenshotData {
        moment_id: Uuid,
    },
    PreviewCleanup {
        from_ms: i64,
        to_ms: i64,
        sources: EventSources,
        mode: CleanupMode,
    },
    ExecuteCleanup {
        preview_token: Uuid,
        confirmed_all_data: bool,
    },
    ExportRange {
        from_ms: i64,
        to_ms: i64,
        sources: EventSources,
        include_audio: bool,
        include_screenshots: bool,
        destination: PathBuf,
    },
    Log {
        event: LogEvent,
    },
    Logs {
        filter: LogFilter,
    },
    YieldToLogin,
    Quit,
}

impl Request {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Diagnose { .. } => "diagnose",
            Self::Models => "models",
            Self::PrepareModels => "prepare_models",
            Self::Probe { .. } => "probe",
            Self::RecordTest { .. } => "record_test",
            Self::Start { .. } => "start",
            Self::Pause { .. } => "pause",
            Self::Resume { .. } => "resume",
            Self::SetSource { .. } => "set_source",
            Self::SetAudioLimit { .. } => "set_audio_limit",
            Self::CloudStatus => "cloud_status",
            Self::UsageStatistics { .. } => "usage_statistics",
            Self::CloudTasks { .. } => "cloud_tasks",
            Self::ConfigureCloud { .. } => "configure_cloud",
            Self::SetCloudPaused { .. } => "set_cloud_paused",
            Self::RetryCloudTask { .. } => "retry_cloud_task",
            Self::ProcessNow => "process_now",
            Self::Timeline { .. } => "timeline",
            Self::CalendarDays { .. } => "calendar_days",
            Self::EventDetail { .. } => "event_detail",
            Self::RangeDetail { .. } => "range_detail",
            Self::PlaybackChunk { .. } => "playback_chunk",
            Self::PlaybackValid { .. } => "playback_valid",
            Self::CreateEvent { .. } => "create_event",
            Self::EditEvent { .. } => "edit_event",
            Self::RetranscribeEvent { .. } => "retranscribe_event",
            Self::DeleteEvent { .. } => "delete_event",
            Self::SplitEvent { .. } => "split_event",
            Self::MergeEvents { .. } => "merge_events",
            Self::TranscribeRange { .. } => "transcribe_range",
            Self::CorrectText { .. } => "correct_text",
            Self::RevertText { .. } => "revert_text",
            Self::ObjectRevision { .. } => "object_revision",
            Self::SessionSpeakers { .. } => "session_speakers",
            Self::AssignLocalSpeaker { .. } => "assign_local_speaker",
            Self::People => "people",
            Self::CreatePerson { .. } => "create_person",
            Self::CreateAndBindSpeaker { .. } => "create_and_bind_speaker",
            Self::RenamePerson { .. } => "rename_person",
            Self::DeletePerson { .. } => "delete_person",
            Self::BindSpeaker { .. } => "bind_speaker",
            Self::UnbindSpeaker { .. } => "unbind_speaker",
            Self::AddVoiceSample { .. } => "add_voice_sample",
            Self::RemoveVoiceSample { .. } => "remove_voice_sample",
            Self::VoiceSamples { .. } => "voice_samples",
            Self::ReviewItems => "review_items",
            Self::ResolveReview { .. } => "resolve_review",
            Self::Search { .. } => "search",
            Self::Moments { .. } => "moments",
            Self::AddMoment { .. } => "add_moment",
            Self::CaptureScreenshot { .. } => "capture_screenshot",
            Self::ScreenshotData { .. } => "screenshot_data",
            Self::PreviewCleanup { .. } => "preview_cleanup",
            Self::ExecuteCleanup { .. } => "execute_cleanup",
            Self::ExportRange { .. } => "export_range",
            Self::Log { .. } => "log",
            Self::Logs { .. } => "logs",
            Self::YieldToLogin => "yield_to_login",
            Self::Quit => "quit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<LogContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildInfo>,
    pub component: String,
    pub level: LogLevel,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_span_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogFilter {
    /// Internal diagnostic event dependency expansion. Intersected with job_id if present.
    #[serde(skip)]
    pub related_job_ids: Option<Vec<Uuid>>,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub job_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub capture_span_id: Option<Uuid>,
    pub component: Option<String>,
    pub level: Option<LogLevel>,
    pub correlation_id: Option<Uuid>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub timestamp_ms: u64,
    pub run_id: Uuid,
    pub elapsed_ms: u64,
    pub client: ClientKind,
    pub request_id: Uuid,
    #[serde(flatten)]
    pub event: LogEvent,
}

impl LogEvent {
    pub fn simple(component: &str, level: LogLevel, event: &str) -> Self {
        Self {
            context: None,
            build: None,
            component: component.to_owned(),
            level,
            event: event.to_owned(),
            correlation_id: None,
            source_id: None,
            capture_span_id: None,
            event_id: None,
            job_id: None,
            revision: None,
            duration_ms: None,
            dropped_count: None,
            error_code: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSource {
    Microphone,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSources {
    Both,
    Microphone,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    Llm,
    Asr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupMode {
    RawAudio,
    AllData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MomentKind {
    Mark,
    Comment,
    Screenshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomentView {
    pub id: Uuid,
    pub kind: MomentKind,
    pub anchor_ms: i64,
    pub created_ms: i64,
    pub captured_ms: Option<i64>,
    pub text: Option<String>,
    pub ocr_text: Option<String>,
    pub ocr_state: Option<String>,
    #[serde(default)]
    pub image_summary: Option<String>,
    #[serde(default)]
    pub image_transcript: Option<String>,
    #[serde(default)]
    pub image_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPreview {
    pub token: Uuid,
    pub mode: CleanupMode,
    pub from_ms: i64,
    pub to_ms: i64,
    pub sources: EventSources,
    pub audio_blocks: u64,
    pub audio_bytes: u64,
    pub coarse_segments: u64,
    pub precise_segments: u64,
    pub voice_samples: u64,
    pub asr_tasks_awaiting_upload: u64,
    pub voice_extractions_awaiting_audio: u64,
    pub affected_event_count: u64,
    pub affected_events: Vec<Uuid>,
    pub moment_count: u64,
    pub screenshot_count: u64,
    pub overlap_may_remove_whole_sentences: bool,
    pub voice_features_retained: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudQueueState {
    pub provider: CloudProvider,
    pub configured: bool,
    pub paused: bool,
    pub pending: u64,
    pub processing: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudTaskView {
    pub id: Uuid,
    pub provider: CloudProvider,
    pub target_kind: String,
    pub target_id: String,
    pub source: Option<AudioSource>,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub state: String,
    pub error_code: Option<String>,
    pub manual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelinePreview {
    pub text: String,
    pub source: AudioSource,
    pub is_precise: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineItem {
    pub id: Option<Uuid>,
    pub manual_range_id: Option<Uuid>,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub title: Option<String>,
    pub sources: EventSources,
    pub revision: Option<i64>,
    pub fit_state: Option<String>,
    pub processing_state: String,
    pub manual_asr_state: Option<String>,
    pub manual_asr_started_ms: Option<i64>,
    pub manual_asr_ended_ms: Option<i64>,
    pub manual_asr_source: Option<AudioSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<TimelinePreview>,
    /// LLM event summary; independent of transcript precision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recording_sources: Vec<AudioSource>,
    pub coarse_segments: Vec<CoarseSegment>,
    pub precise_segments: Vec<PreciseSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoarseSegment {
    pub id: Uuid,
    pub source: AudioSource,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreciseSegment {
    pub id: Uuid,
    pub source: AudioSource,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub text: String,
    pub speaker: Option<String>,
    pub original_text: Option<String>,
    pub corrected: bool,
    pub person_id: Option<Uuid>,
    pub person_name: Option<String>,
    pub person_origin: Option<String>,
    pub revision: i64,
    pub speaker_revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSpeaker {
    pub group_id: String,
    pub segment_id: Uuid,
    pub name: String,
    pub anonymous: bool,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonView {
    pub id: Uuid,
    pub display_name: String,
    pub sample_count: u32,
    pub created_ms: i64,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSamplePart {
    pub started_ms: i64,
    pub ended_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSampleView {
    pub id: Uuid,
    pub person_id: Uuid,
    pub source: AudioSource,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub model_version: String,
    pub created_ms: i64,
    pub revision: i64,
    #[serde(default)]
    pub parts: Vec<VoiceSamplePart>,
    #[serde(default)]
    pub audio_available: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewItem {
    pub id: Uuid,
    pub kind: String,
    pub original_segment_id: Uuid,
    pub source: AudioSource,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub text: Option<String>,
    pub person_id: Option<Uuid>,
    pub person_name: Option<String>,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: String,
    pub item_id: Uuid,
    pub source: Option<AudioSource>,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub text: String,
    pub person_id: Option<Uuid>,
    pub person_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Diagnostic {
        id: Uuid,
        report: DiagnosticReport,
    },
    Status {
        id: Uuid,
        state: ServiceState,
    },
    Models {
        id: Uuid,
        state: ModelState,
    },
    ProbeResult {
        id: Uuid,
        stats: ProbeStats,
    },
    RecordTestResult {
        id: Uuid,
        source: AudioSource,
        blocks: usize,
        frames: u64,
        dropped_packets: u64,
    },
    Logs {
        id: Uuid,
        entries: Vec<LogRecord>,
    },
    UsageStatistics {
        id: Uuid,
        statistics: UsageStatistics,
    },
    CloudStatus {
        id: Uuid,
        queues: Vec<CloudQueueState>,
    },
    CloudTasks {
        id: Uuid,
        tasks: Vec<CloudTaskView>,
    },
    Timeline {
        id: Uuid,
        items: Vec<TimelineItem>,
        gaps: Vec<TimeGap>,
    },
    CalendarDays {
        id: Uuid,
        days: Vec<i64>,
    },
    Event {
        id: Uuid,
        event: TimelineItem,
    },
    Accepted {
        id: Uuid,
        object_id: Uuid,
    },
    TranscriptionQueued {
        id: Uuid,
        range_id: Uuid,
        tasks: Vec<CloudTaskView>,
    },
    EventsChanged {
        id: Uuid,
        event_ids: Vec<Uuid>,
    },
    TextChanged {
        id: Uuid,
        segment_id: Uuid,
        revision: i64,
    },
    SessionSpeakers {
        id: Uuid,
        speakers: Vec<LocalSpeaker>,
    },
    People {
        id: Uuid,
        people: Vec<PersonView>,
    },
    VoiceSamples {
        id: Uuid,
        samples: Vec<VoiceSampleView>,
    },
    ReviewItems {
        id: Uuid,
        items: Vec<ReviewItem>,
    },
    ReviewResolved {
        id: Uuid,
        item_id: Uuid,
        target_revision: Option<i64>,
    },
    ObjectRevision {
        id: Uuid,
        revision: i64,
    },
    PersonChanged {
        id: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        sample_count: Option<u32>,
        person_id: Uuid,
        revision: Option<i64>,
        deleted: bool,
    },
    Search {
        id: Uuid,
        hits: Vec<SearchHit>,
        empty_reason: Option<String>,
    },
    Moments {
        id: Uuid,
        moments: Vec<MomentView>,
    },
    MomentSaved {
        id: Uuid,
        moment: MomentView,
    },
    ScreenshotData {
        id: Uuid,
        moment_id: Uuid,
        png_base64: String,
    },
    PlaybackChunk {
        id: Uuid,
        from_ms: i64,
        to_ms: i64,
        wav_base64: String,
        has_audio: bool,
        next_audio_ms: Option<i64>,
        #[serde(default)]
        generation: Option<i64>,
    },
    PlaybackValid {
        id: Uuid,
        valid: bool,
    },
    CleanupPreview {
        id: Uuid,
        preview: CleanupPreview,
    },
    CleanupComplete {
        id: Uuid,
        cleared_audio_blocks: u64,
        physical_deletions_pending: u64,
    },
    ExportComplete {
        id: Uuid,
        destination: PathBuf,
        audio_files: u64,
        transcript_segments: u64,
    },
    Error {
        id: Uuid,
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeStats {
    pub source: AudioSource,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub peak: f32,
    pub dropped_packets: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelState {
    pub phase: ModelPhase,
    pub progress_percent: u8,
    pub speaker_phase: ModelPhase,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPhase {
    Missing,
    Checking,
    Downloading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePhase {
    Disabled,
    Idle,
    Starting,
    Recording,
    Stopping,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureBlocker {
    None,
    SourcesDisabled,
    ModelNotReady,
    SetupIncomplete,
    QueryOnly,
    SessionStopped,
    Maintenance,
    StorageLimit,
    SourceUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    pub revision: i64,
    pub enabled_intent: bool,
    pub paused_intent: bool,
    pub storage_paused: bool,
    #[serde(default)]
    pub audio_used_bytes: u64,
    #[serde(default)]
    pub audio_limit_bytes: u64,
    #[serde(default)]
    pub disk_free_bytes: u64,
    #[serde(default)]
    pub storage_warning: bool,
    pub microphone_enabled: bool,
    pub system_enabled: bool,
    pub capture_state: CaptureState,
    pub reason: Option<String>,
    pub microphone_recording: bool,
    pub system_recording: bool,
    pub microphone_phase: SourcePhase,
    pub system_phase: SourcePhase,
    pub capture_blocker: CaptureBlocker,
    pub microphone_reason: Option<String>,
    pub system_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeGap {
    pub source: AudioSource,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    NotReady,
    Stopped,
    Starting,
    Stopping,
    Paused,
    Recording,
}

pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("HEAR_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    dirs::data_dir()
        .map(|base| base.join("hear"))
        .ok_or_else(|| "cannot locate the user's application data directory".to_owned())
}

pub fn socket_path() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("core.sock"))
}

pub fn send(request: Request) -> Result<Response, String> {
    send_from(ClientKind::Cli, request)
}

pub fn send_from(client: ClientKind, request: Request) -> Result<Response, String> {
    send_from_with_id(client, request, Uuid::new_v4())
}

// The envelope ID is also the durable duplicate-submit key for mutations.
// A retry must reuse both this ID and its original expected revision.
pub fn send_from_with_id(
    client: ClientKind,
    request: Request,
    id: Uuid,
) -> Result<Response, String> {
    send_with_policy(client, request, id, true)
}

pub fn send_existing(client: ClientKind, request: Request) -> Result<Response, String> {
    send_with_policy(client, request, Uuid::new_v4(), false)
}

fn send_with_policy(
    client: ClientKind,
    mut request: Request,
    id: Uuid,
    start_on_demand: bool,
) -> Result<Response, String> {
    let socket = socket_path()?;
    let mut stream = match UnixStream::connect(&socket) {
        Ok(stream) => stream,
        Err(_) if start_on_demand => {
            start_core_on_demand()?;
            wait_for_core(&socket)?
        }
        Err(error) => return Err(error.to_string()),
    };
    let timeout = match request {
        Request::Probe { seconds, .. } | Request::RecordTest { seconds, .. } => {
            Duration::from_secs(seconds as u64 + 70)
        }
        Request::ConfigureCloud { .. } => Duration::from_secs(90),
        Request::ExecuteCleanup { .. } | Request::ExportRange { .. } => Duration::from_secs(180),
        _ => Duration::from_secs(5),
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    attach_correlation(&mut request, id);
    let envelope = Envelope {
        version: PROTOCOL_VERSION,
        id,
        client,
        request,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
    stream
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| error.to_string())?;

    let mut response = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    if response.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("core response exceeded protocol limit".to_owned());
    }
    let decoded: Response = serde_json::from_slice(&response).map_err(|error| error.to_string())?;
    match &decoded {
        Response::Status { id, .. }
        | Response::Diagnostic { id, .. }
        | Response::Models { id, .. }
        | Response::ProbeResult { id, .. }
        | Response::RecordTestResult { id, .. }
        | Response::Logs { id, .. }
        | Response::UsageStatistics { id, .. }
        | Response::CloudStatus { id, .. }
        | Response::CloudTasks { id, .. }
        | Response::Timeline { id, .. }
        | Response::CalendarDays { id, .. }
        | Response::Event { id, .. }
        | Response::Accepted { id, .. }
        | Response::TranscriptionQueued { id, .. }
        | Response::EventsChanged { id, .. }
        | Response::TextChanged { id, .. }
        | Response::SessionSpeakers { id, .. }
        | Response::People { id, .. }
        | Response::VoiceSamples { id, .. }
        | Response::ReviewItems { id, .. }
        | Response::ReviewResolved { id, .. }
        | Response::ObjectRevision { id, .. }
        | Response::PersonChanged { id, .. }
        | Response::Search { id, .. }
        | Response::Moments { id, .. }
        | Response::MomentSaved { id, .. }
        | Response::ScreenshotData { id, .. }
        | Response::PlaybackChunk { id, .. }
        | Response::PlaybackValid { id, .. }
        | Response::CleanupPreview { id, .. }
        | Response::CleanupComplete { id, .. }
        | Response::ExportComplete { id, .. }
        | Response::Error { id, .. }
            if *id == envelope.id =>
        {
            Ok(decoded)
        }
        _ => Err("core response ID did not match request".to_owned()),
    }
}

fn attach_correlation(request: &mut Request, id: Uuid) {
    if let Request::Log { event } = request {
        event.correlation_id.get_or_insert(id);
    }
}

fn wait_for_core(socket: &Path) -> Result<UnixStream, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(stream) = UnixStream::connect(socket) {
            return Ok(stream);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("hear-core did not become available".to_owned())
}

fn start_core_on_demand() -> Result<(), String> {
    let path = core_binary_path()?;
    Command::new(path)
        .arg("serve")
        .arg("--on-demand")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("cannot launch hear-core: {error}"))
}

fn core_binary_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("HEAR_CORE_BIN") {
        return Ok(PathBuf::from(path));
    }
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let sibling = executable
        .parent()
        .ok_or_else(|| "cannot locate hear binary directory".to_owned())?;
    let installed = sibling.join("../Library/LoginItems/HearCore.app/Contents/MacOS/hear-core");
    if installed.is_file() {
        return fs::canonicalize(installed).map_err(|error| error.to_string());
    }
    let development = sibling.join("hear-core");
    if development.is_file() {
        return Ok(development);
    }
    Err("hear-core is not installed beside the app or CLI".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_sample_playback_metadata_is_additive() {
        let old = serde_json::json!({"id":Uuid::new_v4(),"person_id":Uuid::new_v4(),"source":"microphone","started_ms":1000,"ended_ms":5000,"model_version":"campplus-zh-v1","created_ms":6000,"revision":1});
        let view: VoiceSampleView = serde_json::from_value(old).unwrap();
        assert!(view.parts.is_empty());
        assert!(view.audio_available.is_none());
    }

    #[test]
    fn playback_generation_is_additive_to_the_existing_chunk_response() {
        let id = Uuid::new_v4();
        let old = serde_json::json!({"type":"playback_chunk","id":id,"from_ms":0,"to_ms":5000,"wav_base64":"","has_audio":false,"next_audio_ms":null});
        assert!(matches!(
            serde_json::from_value::<Response>(old.clone()).unwrap(),
            Response::PlaybackChunk {
                generation: None,
                ..
            }
        ));
        let mut current = old;
        current["generation"] = serde_json::json!(7);
        assert!(matches!(
            serde_json::from_value::<Response>(current).unwrap(),
            Response::PlaybackChunk {
                generation: Some(7),
                ..
            }
        ));
        let request = Request::PlaybackValid {
            from_ms: 0,
            to_ms: 10000,
            sources: EventSources::Both,
            generation: 7,
        };
        assert_eq!(request.name(), "playback_valid");
        assert!(matches!(
            serde_json::from_value::<Request>(serde_json::to_value(request).unwrap()).unwrap(),
            Request::PlaybackValid { generation: 7, .. }
        ));
    }

    #[test]
    fn screenshot_gui_roundtrip_and_cli_defaults_preserve_shape() {
        let request = Request::CaptureScreenshot {
            excluded_window_ids: vec![123],
            anchor_ms: Some(1000),
        };
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["type"], "capture_screenshot");
        assert!(
            matches!(serde_json::from_value::<Request>(encoded).unwrap(), Request::CaptureScreenshot { excluded_window_ids, anchor_ms: Some(1000) } if excluded_window_ids == vec![123])
        );
        assert!(
            matches!(serde_json::from_value::<Request>(serde_json::json!({"type":"capture_screenshot"})).unwrap(), Request::CaptureScreenshot { excluded_window_ids, anchor_ms: None } if excluded_window_ids.is_empty())
        );
    }
    #[test]
    fn source_diagnostics_have_stable_structured_wire_values() {
        let state: ServiceState=serde_json::from_value(serde_json::json!({"revision":1,"enabled_intent":true,"paused_intent":false,"storage_paused":false,"microphone_enabled":true,"system_enabled":true,"capture_state":"stopped","reason":null,"microphone_recording":false,"system_recording":false,"microphone_reason":null,"system_reason":null,"microphone_phase":"starting","system_phase":"unavailable","capture_blocker":"source_unavailable"})).unwrap();
        let encoded = serde_json::to_value(&state).unwrap();
        assert_eq!(encoded["microphone_phase"], "starting");
        assert_eq!(encoded["system_phase"], "unavailable");
        assert_eq!(encoded["capture_blocker"], "source_unavailable");
        let decoded: ServiceState = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.system_phase, SourcePhase::Unavailable);
        assert_eq!(decoded.capture_blocker, CaptureBlocker::SourceUnavailable);
    }

    #[test]
    fn logging_request_inherits_ipc_correlation() {
        let id = Uuid::new_v4();
        let mut request = Request::Log {
            event: LogEvent::simple("gui", LogLevel::Info, "opened"),
        };
        attach_correlation(&mut request, id);
        let Request::Log { event } = request else {
            unreachable!()
        };
        assert_eq!(event.correlation_id, Some(id));
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("null"));
    }

    #[test]
    fn transcription_receipt_serializes_physical_task_ids_and_sources() {
        let range_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let value = serde_json::to_value(Response::TranscriptionQueued {
            id: Uuid::new_v4(),
            range_id,
            tasks: vec![CloudTaskView {
                id: task_id,
                provider: CloudProvider::Asr,
                target_kind: "range".to_owned(),
                target_id: range_id.to_string(),
                source: Some(AudioSource::System),
                started_ms: 1_000,
                ended_ms: 6_000,
                state: "pending".to_owned(),
                error_code: None,
                manual: true,
            }],
        })
        .unwrap();
        assert_eq!(value["type"], "transcription_queued");
        assert_eq!(value["range_id"], range_id.to_string());
        assert_eq!(value["tasks"][0]["id"], task_id.to_string());
        assert_eq!(value["tasks"][0]["source"], "system");
        assert_eq!(value["tasks"][0]["state"], "pending");
    }
    #[test]
    fn timeline_preview_is_optional_for_existing_clients_and_cores() {
        let legacy = serde_json::json!({
            "id":null,"manual_range_id":null,"started_ms":0,"ended_ms":1000,
            "title":null,"sources":"microphone","revision":null,"fit_state":null,
            "processing_state":"unorganized","manual_asr_state":null,
            "manual_asr_started_ms":null,"manual_asr_ended_ms":null,"manual_asr_source":null,
            "coarse_segments":[],"precise_segments":[]
        });
        let mut item: TimelineItem = serde_json::from_value(legacy.clone()).unwrap();
        assert!(item.preview.is_none());
        assert!(item.summary.is_none());
        assert_eq!(serde_json::to_value(&item).unwrap(), legacy);
        item.summary = Some("事件摘要".into());
        item.preview = Some(TimelinePreview {
            text: "摘录".into(),
            source: AudioSource::Microphone,
            is_precise: false,
        });
        let wire = serde_json::to_value(&item).unwrap();
        assert_eq!(wire["preview"]["text"], "摘录");
        assert_eq!(wire["summary"], "事件摘要");
        assert!(
            serde_json::from_value::<TimelineItem>(wire)
                .unwrap()
                .preview
                .is_some()
        );
    }
    #[test]
    fn calendar_projection_roundtrips_without_changing_timeline_requests() {
        let request = Request::CalendarDays {
            boundaries: vec![0, 86_400_000],
        };
        assert_eq!(request.name(), "calendar_days");
        let wire = serde_json::to_value(&request).unwrap();
        assert!(
            matches!(serde_json::from_value::<Request>(wire).unwrap(), Request::CalendarDays { boundaries } if boundaries.len()==2)
        );
        let response = Response::CalendarDays {
            id: Uuid::new_v4(),
            days: vec![0],
        };
        assert!(
            matches!(serde_json::from_value::<Response>(serde_json::to_value(response).unwrap()).unwrap(), Response::CalendarDays { days, .. } if days == vec![0])
        );
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageSeries {
    pub requests: i64,
    pub count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost_nano: i64,
    pub incomplete: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDay {
    pub started_ms: i64,
    pub covered: bool,
    pub partial: bool,
    pub asr: UsageSeries,
    pub llm: UsageSeries,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStatistics {
    pub started_ms: i64,
    pub currency: String,
    pub days: Vec<UsageDay>,
}
