use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig, SileroVadModelConfig,
    SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig, VadModelConfig,
    VoiceActivityDetector, Wave,
};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;

const RATE: u32 = 16_000;
const MAX_SAMPLES: usize = RATE as usize * 30;
// Sherpa's max_speech_duration only raises its VAD threshold. Continuous
// speech can still grow beyond it, so bound the utterance before ASR.
const MAX_ACTIVE_SAMPLES: u64 = RATE as u64 * 20;

#[derive(Deserialize)]
struct WorkHeader {
    source: String,
    session: String,
    start_sample: u64,
    samples: usize,
    flush: bool,
}

#[derive(Serialize)]
struct VadResult {
    segments: Vec<SpeechRegion>,
    speech_active: bool,
    speech_pending: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct SpeechRegion {
    start_sample: u64,
    end_sample: u64,
}

struct SourceSession {
    session: String,
    vad: VoiceActivityDetector,
    base_sample: u64,
    next_sample: u64,
    speech_started_sample: Option<u64>,
}

struct VadWorker {
    vad_config: VadModelConfig,
    sessions: HashMap<String, SourceSession>,
}

impl VadWorker {
    fn new(model_dir: &Path) -> Result<Self, String> {
        let vad_path = model_dir.join("silero_vad.onnx");
        if !vad_path.is_file() {
            return Err("missing VAD asset".to_owned());
        }
        let vad_config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(vad_path.to_string_lossy().into_owned()),
                threshold: 0.35,
                min_silence_duration: 0.35,
                min_speech_duration: 0.2,
                window_size: 512,
                max_speech_duration: 30.0,
            },
            sample_rate: RATE as i32,
            num_threads: 1,
            ..Default::default()
        };
        Ok(Self {
            vad_config,
            sessions: HashMap::new(),
        })
    }

    fn process(&mut self, header: WorkHeader, samples: &[f32]) -> VadResult {
        let source = header.source;
        if !matches!(source.as_str(), "microphone" | "system") {
            return VadResult {
                segments: vec![],
                speech_active: false,
                speech_pending: false,
                error: Some("invalid_source".to_owned()),
            };
        }
        if self.sessions.get(&source).is_some_and(|session| {
            session.next_sample != header.start_sample || session.session != header.session
        }) {
            self.sessions.remove(&source);
        }
        if !self.sessions.contains_key(&source) {
            let Some(vad) = VoiceActivityDetector::create(&self.vad_config, 60.0) else {
                return VadResult {
                    segments: vec![],
                    speech_active: false,
                    speech_pending: false,
                    error: Some("vad_unavailable".to_owned()),
                };
            };
            self.sessions.insert(
                source.clone(),
                SourceSession {
                    session: header.session,
                    vad,
                    base_sample: header.start_sample,
                    next_sample: header.start_sample,
                    speech_started_sample: None,
                },
            );
        }
        let session = self.sessions.get_mut(&source).unwrap();
        let mut segments = vec![];
        // The VAD consumes fixed short windows. Feeding a whole sealed block
        // in one call can silently miss speech at its beginning.
        let mut chunk_end = header.start_sample;
        for chunk in samples.chunks(512) {
            session.vad.accept_waveform(chunk);
            chunk_end = chunk_end.saturating_add(chunk.len() as u64);
            let before = segments.len();
            Self::drain(session, &mut segments);
            if session.vad.detected() {
                if segments.len() != before || session.speech_started_sample.is_none() {
                    session.speech_started_sample = Some(chunk_end);
                } else if session
                    .speech_started_sample
                    .is_some_and(|start| chunk_end.saturating_sub(start) >= MAX_ACTIVE_SAMPLES)
                {
                    session.vad.flush();
                    Self::drain(session, &mut segments);
                    session.speech_started_sample = None;
                }
            } else {
                session.speech_started_sample = None;
            }
        }
        session.next_sample += samples.len() as u64;
        if header.flush {
            session.vad.flush();
            Self::drain(session, &mut segments);
        }
        // `detected` describes the VAD state at the end of this block. An
        // utterance can already have been drained by then. Count only drained
        // segments that overlap this block: its trailing silence may finalize
        // speech that happened entirely in the preceding block.
        let block_end = header.start_sample.saturating_add(samples.len() as u64);
        let speech_active = session.vad.detected()
            || segments.iter().any(|segment| {
                segment.start_sample < block_end && segment.end_sample > header.start_sample
            });
        let speech_pending = !header.flush && session.vad.detected();
        if header.flush {
            self.sessions.remove(&source);
        }
        VadResult {
            segments,
            speech_active,
            speech_pending,
            error: None,
        }
    }

    fn drain(session: &mut SourceSession, output: &mut Vec<SpeechRegion>) {
        while let Some(segment) = session.vad.front() {
            // Silero can queue an empty marker after our active-duration
            // flush. It contains no samples, so it is not a speech region.
            // Keep the core's strict positive-duration validation intact.
            if segment.n() > 0 {
                let start_sample = session
                    .base_sample
                    .saturating_add(segment.start().max(0) as u64);
                output.push(SpeechRegion {
                    start_sample,
                    end_sample: start_sample + segment.n() as u64,
                });
            }
            session.vad.pop();
        }
    }
}

fn read_header<R: Read>(input: &mut R) -> Result<Option<WorkHeader>, String> {
    let mut size = [0u8; 4];
    match input.read_exact(&mut size) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > 4096 {
        return Err("worker request header exceeded limit".to_owned());
    }
    let mut bytes = vec![0; size];
    input
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn serve_vad(model_dir: &Path) -> Result<(), String> {
    let mut worker = VadWorker::new(model_dir)?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    while let Some(header) = read_header(&mut input)? {
        if header.samples > MAX_SAMPLES {
            return Err("worker request audio exceeded limit".to_owned());
        }
        let samples = read_samples(&mut input, header.samples)?;
        let result = worker.process(header, &samples);
        write_result(&mut output, &result)?;
    }
    Ok(())
}

fn serve_embeddings(model_path: &Path) -> Result<(), String> {
    let config = SpeakerEmbeddingExtractorConfig {
        model: Some(model_path.to_string_lossy().into_owned()),
        num_threads: 1,
        debug: false,
        provider: Some("cpu".to_owned()),
    };
    let extractor = SpeakerEmbeddingExtractor::create(&config)
        .ok_or_else(|| "speaker model could not load".to_owned())?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    while let Some(header) = read_header(&mut input)? {
        if !(RATE as usize * 2..=RATE as usize * 15).contains(&header.samples) {
            return Err("speaker sample duration invalid".into());
        }
        let samples = read_samples(&mut input, header.samples)?;
        let stream = extractor
            .create_stream()
            .ok_or_else(|| "speaker stream unavailable".to_owned())?;
        stream.accept_waveform(RATE as i32, &samples);
        stream.input_finished();
        if !extractor.is_ready(&stream) {
            return Err("speaker sample is too short".to_owned());
        }
        let embedding = extractor
            .compute(&stream)
            .ok_or_else(|| "speaker embedding failed".to_owned())?;
        if embedding.len() < 64 || embedding.iter().any(|value| !value.is_finite()) {
            return Err("speaker embedding invalid".to_owned());
        }
        write_result(&mut output, &embedding)?;
    }
    Ok(())
}

fn recognizer(model_dir: &Path) -> Result<OfflineRecognizer, String> {
    let mut config = OfflineRecognizerConfig::default();
    let asset = |name| {
        Some(
            model_dir
                .join("xasr")
                .join(name)
                .to_string_lossy()
                .into_owned(),
        )
    };
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: asset("encoder-epoch-99-avg-1.int8.onnx"),
        decoder: asset("decoder-epoch-99-avg-1.onnx"),
        joiner: asset("joiner-epoch-99-avg-1.int8.onnx"),
    };
    config.model_config.tokens = asset("tokens.txt");
    config.model_config.provider = Some("cpu".to_owned());
    config.model_config.num_threads = 2;
    OfflineRecognizer::create(&config).ok_or_else(|| "X-ASR model could not load".to_owned())
}

fn read_samples(input: &mut impl Read, count: usize) -> Result<Vec<f32>, String> {
    if count > MAX_SAMPLES {
        return Err("worker audio exceeded limit".into());
    }
    let mut raw = vec![0; count * 4];
    input.read_exact(&mut raw).map_err(|e| e.to_string())?;
    let samples: Vec<f32> = raw
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    if samples.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite worker audio".into());
    }
    Ok(samples)
}

fn write_result(output: &mut impl Write, result: &impl Serialize) -> Result<(), String> {
    let body = serde_json::to_vec(result).map_err(|e| e.to_string())?;
    output
        .write_all(&(body.len() as u32).to_le_bytes())
        .and_then(|()| output.write_all(&body))
        .and_then(|()| output.flush())
        .map_err(|e| e.to_string())
}

// X-ASR's encoder convolutions require several feature frames. VAD flushes
// and coverage subtraction can produce shorter, valid tails. Pad only the
// inference buffer: Core retains the original samples and time interval.
fn recognize(recognizer: &OfflineRecognizer, rate: i32, samples: &[f32]) -> Result<String, String> {
    if samples.is_empty() {
        return Ok(String::new());
    }
    if rate <= 0 {
        return Err("invalid sample rate".into());
    }
    let minimum = (rate as usize).div_ceil(5); // 200 ms
    let mut padded = Vec::new();
    let input = if samples.len() < minimum {
        padded.extend_from_slice(samples);
        padded.resize(minimum, 0.0);
        &padded
    } else {
        samples
    };
    let stream = recognizer.create_stream();
    stream.accept_waveform(rate, input);
    recognizer.decode(&stream);
    stream
        .get_result()
        .map(|r| r.text)
        .ok_or_else(|| "recognizer result unavailable".into())
}

fn serve_recognizer(model_dir: &Path) -> Result<(), String> {
    let recognizer = recognizer(model_dir)?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    while let Some(header) = read_header(&mut input)? {
        let samples = read_samples(&mut input, header.samples)?;
        let text = recognize(&recognizer, RATE as i32, &samples)?;
        write_result(&mut output, &text)?;
    }
    Ok(())
}

fn watch_parent(parent: i32) {
    std::thread::spawn(move || {
        loop {
            if unsafe { libc::getppid() } != parent {
                // Another thread may be in ONNX. Running C++ atexit
                // destructors here races that inference/model initialization.
                unsafe { libc::_exit(70) };
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

fn run() -> Result<(), String> {
    if std::env::args().nth(1).as_deref() == Some("--runtime-info") {
        println!(
            "{}",
            serde_json::json!({
                "architecture": std::env::consts::ARCH,
                "sherpa_onnx": sherpa_onnx::version(),
                "onnxruntime": sherpa_onnx::onnxruntime_version(),
                "provider": "cpu",
            })
        );
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--build-info") {
        println!(
            "{}",
            serde_json::to_string(&hear_protocol::build_info("worker")).unwrap()
        );
        return Ok(());
    }
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() >= 2 && args[args.len() - 2] == "--parent-pid" {
        let parent: i32 = args
            .pop()
            .unwrap()
            .parse()
            .map_err(|_| "invalid parent PID")?;
        args.pop();
        if parent <= 1 {
            return Err("invalid parent PID".into());
        }
        watch_parent(parent);
    }
    match args.as_slice() {
        [command, model_dir] if command == "vad" => serve_vad(Path::new(model_dir)),
        [command, model_dir] if command == "recognize" => serve_recognizer(Path::new(model_dir)),
        [command, model_path] if command == "embed" => serve_embeddings(Path::new(model_path)),
        [command, model_dir, wav] if command == "self-test" => {
            let recognizer = recognizer(Path::new(model_dir))?;
            let wave = Wave::read(wav).ok_or_else(|| "cannot read test WAV".to_owned())?;
            println!("{}", recognize(&recognizer, wave.sample_rate(), wave.samples())?);
            Ok(())
        }
        _ => Err(
            "usage: hear-worker vad MODEL_DIR | recognize MODEL_DIR | embed MODEL_PATH | self-test MODEL_DIR WAV"
                .to_owned(),
        ),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("hear-worker: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn orphan_watchdog_skips_global_destructors() {
        if std::env::var_os("HEAR_TEST_ORPHAN_EXIT").is_some() {
            extern "C" fn destructor() {
                unsafe { libc::_exit(99) };
            }
            assert_eq!(unsafe { libc::atexit(destructor) }, 0);
            super::watch_parent(std::process::id() as i32);
            std::thread::sleep(std::time::Duration::from_secs(5));
            panic!("watchdog did not terminate orphan");
        }
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::orphan_watchdog_skips_global_destructors"])
            .env("HEAR_TEST_ORPHAN_EXIT", "1")
            .status()
            .unwrap();
        assert_eq!(result.code(), Some(70));
    }
}
