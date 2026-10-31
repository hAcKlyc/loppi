use super::Command;
use base64::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Mutex, OnceLock},
};
use tauri::Emitter;
struct Buffer {
    pcm: Vec<f32>,
    when: Option<u64>,
}
struct State {
    context: u64,
    epoch: u64,
    active: bool,
    frames: u64,
    rate: u32,
    next: u64,
    buffers: BTreeMap<u64, Buffer>,
}
struct Output {
    state: Arc<Mutex<State>>,
    _stream: cpal::Stream,
}
static OUTPUT: OnceLock<Mutex<Option<Output>>> = OnceLock::new();
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn storage() -> &'static Mutex<Option<Output>> {
    OUTPUT.get_or_init(|| Mutex::new(None))
}
fn event(app: &tauri::AppHandle, context: u64, buffer: u64, epoch: u64, status: i32) {
    let _ = app.emit_to(
        "main",
        "loppi-playback-ended",
        json!({"context":context,"buffer":buffer,"epoch":epoch,"status":status}),
    );
}
fn render<T: cpal::Sample + cpal::FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    state: &Arc<Mutex<State>>,
    app: &tauri::AppHandle,
) {
    let Ok(mut state) = state.lock() else {
        for value in data {
            *value = T::from_sample(0.0);
        }
        return;
    };
    for frame in data.chunks_mut(channels) {
        let mut sample = 0.0;
        if state.active {
            for buffer in state.buffers.values() {
                if let Some(start) = buffer.when {
                    if state.frames >= start {
                        let position = (state.frames - start) as f64 * 48000.0 / state.rate as f64;
                        let index = position as usize;
                        if let Some(&a) = buffer.pcm.get(index) {
                            let b = buffer.pcm.get(index + 1).copied().unwrap_or(a);
                            sample += a + (b - a) * (position.fract() as f32);
                        }
                    }
                }
            }
            state.frames = state.frames.saturating_add(1);
        }
        for value in frame {
            *value = T::from_sample(sample.clamp(-1.0, 1.0));
        }
    }
    let (context, epoch, frames, rate) = (state.context, state.epoch, state.frames, state.rate);
    state.buffers.retain(|id, buffer| {
        let complete = buffer.when.is_some_and(|start| {
            frames.saturating_sub(start) as f64 * 48000.0 / rate as f64 >= buffer.pcm.len() as f64
                && frames >= start
        });
        if complete {
            event(app, context, *id, epoch, 1);
        }
        !complete
    });
}
fn open(app: tauri::AppHandle) -> Result<Output, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("playback_device")?;
    let supported = device
        .default_output_config()
        .map_err(|_| "playback_device")?;
    let rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let config: cpal::StreamConfig = supported.clone().into();
    let state = Arc::new(Mutex::new(State {
        context: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        epoch: 0,
        active: false,
        frames: 0,
        rate,
        next: 1,
        buffers: BTreeMap::new(),
    }));
    let error_state = state.clone();
    let error_app = app.clone();
    let error = move |_: cpal::Error| {
        if let Ok(mut s) = error_state.lock() {
            s.active = false;
            for id in s.buffers.keys() {
                event(&error_app, s.context, *id, s.epoch, 0);
            }
            s.buffers.clear();
        }
    };
    let render_state = state.clone();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |d: &mut [f32], _| render(d, channels, &render_state, &app),
            error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |d: &mut [i16], _| render(d, channels, &render_state, &app),
            error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            config,
            move |d: &mut [u16], _| render(d, channels, &render_state, &app),
            error,
            None,
        ),
        _ => return Err("playback_device".into()),
    }
    .map_err(|_| "playback_device")?;
    stream.play().map_err(|_| "playback_device")?;
    Ok(Output {
        state,
        _stream: stream,
    })
}
fn execute(app: tauri::AppHandle, command: Command) -> Result<Value, String> {
    let mut output = storage().lock().map_err(|_| "playback_device")?;
    if matches!(command, Command::Open) {
        *output = Some(open(app)?);
        return Ok(json!({"context":output.as_ref().unwrap().state.lock().unwrap().context}));
    }
    let current = output.as_ref().ok_or("playback_stale")?;
    let shared = current.state.clone();
    let mut state = shared.lock().map_err(|_| "playback_device")?;
    let (context, epoch) = match &command {
        Command::Resume { context, epoch }
        | Command::Suspend { context, epoch }
        | Command::Close { context, epoch }
        | Command::Load { context, epoch, .. }
        | Command::Schedule { context, epoch, .. }
        | Command::Remove { context, epoch, .. } => (*context, *epoch),
        _ => return Err("playback_invalid_command".into()),
    };
    if context != state.context || epoch < state.epoch {
        return Err("playback_stale".into());
    }
    match command {
        Command::Close { .. } => {
            state.active = false;
            state.buffers.clear();
            drop(state);
            *output = None;
            Ok(json!({}))
        }
        Command::Suspend { .. } => {
            state.active = false;
            state.epoch = epoch;
            state.buffers.clear();
            Ok(json!({}))
        }
        Command::Resume { .. } => {
            state.active = true;
            state.epoch = epoch;
            state.frames = 0;
            state.buffers.clear();
            Ok(json!({"clock":0}))
        }
        command => {
            if epoch != state.epoch || !state.active {
                return Err("playback_stale".into());
            }
            match command {
                Command::Load { wav, .. } => {
                    if wav.len() > 1500000 || state.buffers.len() >= 2 {
                        return Err("playback_buffer_limit".into());
                    }
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(wav)
                        .map_err(|_| "playback_invalid_audio")?;
                    if bytes.len() > 1000000 {
                        return Err("playback_invalid_audio".into());
                    }
                    let mut reader = hound::WavReader::new(Cursor::new(bytes))
                        .map_err(|_| "playback_invalid_audio")?;
                    let spec = reader.spec();
                    let count = reader.duration();
                    if spec.channels != 1
                        || spec.sample_rate != 48000
                        || spec.bits_per_sample != 16
                        || spec.sample_format != hound::SampleFormat::Int
                        || count == 0
                        || count > 240048
                    {
                        return Err("playback_invalid_audio".into());
                    }
                    let pcm = reader
                        .samples::<i16>()
                        .map(|v| v.map(|v| v as f32 / 32768.0))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| "playback_invalid_audio")?;
                    let id = state.next;
                    state.next += 1;
                    state.buffers.insert(id, Buffer { pcm, when: None });
                    Ok(
                        json!({"buffer":id,"duration":count as f64/48000.0,"sampleRate":48000,"numberOfChannels":1,"clock":state.frames as f64/state.rate as f64}),
                    )
                }
                Command::Schedule { buffer, when, .. } => {
                    if !when.is_finite() || !(0.0..=86400.0).contains(&when) {
                        return Err("playback_invalid_schedule".into());
                    }
                    let frame = ((when * state.rate as f64).round() as u64).max(state.frames);
                    let actual = frame as f64 / state.rate as f64;
                    state
                        .buffers
                        .get_mut(&buffer)
                        .ok_or("playback_invalid_audio")?
                        .when = Some(frame);
                    Ok(json!({"when":actual}))
                }
                Command::Remove { buffer, .. } => {
                    state.buffers.remove(&buffer);
                    Ok(json!({}))
                }
                _ => Err("playback_invalid_command".into()),
            }
        }
    }
}
pub async fn playback_output(app: tauri::AppHandle, command: Command) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || execute(app, command))
        .await
        .map_err(|_| "playback_device".to_owned())?
}
pub fn shutdown() {
    if let Ok(mut output) = storage().lock() {
        *output = None;
    }
}
