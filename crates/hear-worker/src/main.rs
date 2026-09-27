use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, SileroVadModelConfig,
    SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig, VadModelConfig,
    VoiceActivityDetector, Wave,
};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;

const RATE: u32 = 16_000;
const MAX_SAMPLES: usize = RATE as usize * 30;

#[derive(Deserialize)]
struct WorkHeader {
    source: String,
    start_sample: u64,
    samples: usize,
    flush: bool,
}

#[derive(Serialize)]
struct WorkResult {
    segments: Vec<RecognizedSegment>,
    speech_active: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct RecognizedSegment {
    start_sample: u64,
    end_sample: u64,
    text: String,
}

struct SourceSession {
    vad: VoiceActivityDetector,
    base_sample: u64,
    next_sample: u64,
}

struct Worker {
    recognizer: OfflineRecognizer,
    vad_config: VadModelConfig,
    sessions: HashMap<String, SourceSession>,
}

impl Worker {
    fn new(model_dir: &Path) -> Result<Self, String> {
        let vad_path = model_dir.join("silero_vad.onnx");
        let sense_dir = model_dir.join("sensevoice");
        let model_path = sense_dir.join("model.int8.onnx");
        let tokens_path = sense_dir.join("tokens.txt");
        for path in [&vad_path, &model_path, &tokens_path] {
            if !path.is_file() {
                return Err(format!("missing model asset: {}", path.display()));
            }
        }
        let mut recognizer_config = OfflineRecognizerConfig::default();
        recognizer_config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
            model: Some(model_path.to_string_lossy().into_owned()),
            language: Some("auto".to_owned()),
            use_itn: true,
        };
        recognizer_config.model_config.tokens = Some(tokens_path.to_string_lossy().into_owned());
        recognizer_config.model_config.num_threads = 2;
        let recognizer = OfflineRecognizer::create(&recognizer_config)
            .ok_or_else(|| "SenseVoice model could not load".to_owned())?;
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
            recognizer,
            vad_config,
            sessions: HashMap::new(),
        })
    }

    fn process(&mut self, header: WorkHeader, samples: &[f32]) -> WorkResult {
        let source = header.source;
        if !matches!(source.as_str(), "microphone" | "system") {
            return WorkResult {
                segments: vec![],
                speech_active: false,
                error: Some("invalid_source".to_owned()),
            };
        }
        if self
            .sessions
            .get(&source)
            .is_some_and(|session| session.next_sample != header.start_sample)
        {
            self.sessions.remove(&source);
        }
        if !self.sessions.contains_key(&source) {
            let Some(vad) = VoiceActivityDetector::create(&self.vad_config, 60.0) else {
                return WorkResult {
                    segments: vec![],
                    speech_active: false,
                    error: Some("vad_unavailable".to_owned()),
                };
            };
            self.sessions.insert(
                source.clone(),
                SourceSession {
                    vad,
                    base_sample: header.start_sample,
                    next_sample: header.start_sample,
                },
            );
        }
        let session = self.sessions.get_mut(&source).unwrap();
        let mut segments = vec![];
        // The VAD consumes fixed short windows. Feeding a whole sealed block
        // in one call can silently miss speech at its beginning.
        for chunk in samples.chunks(512) {
            session.vad.accept_waveform(chunk);
            Self::drain(&self.recognizer, session, &mut segments);
        }
        session.next_sample += samples.len() as u64;
        if header.flush {
            session.vad.flush();
            Self::drain(&self.recognizer, session, &mut segments);
        }
        let speech_active = session.vad.detected();
        if header.flush {
            self.sessions.remove(&source);
        }
        WorkResult {
            segments,
            speech_active,
            error: None,
        }
    }

    fn drain(
        recognizer: &OfflineRecognizer,
        session: &mut SourceSession,
        output: &mut Vec<RecognizedSegment>,
    ) {
        while let Some(segment) = session.vad.front() {
            let start_sample = session
                .base_sample
                .saturating_add(segment.start().max(0) as u64);
            let end_sample = start_sample + segment.n().max(0) as u64;
            let stream = recognizer.create_stream();
            stream.accept_waveform(RATE as i32, segment.samples());
            recognizer.decode(&stream);
            let text = stream
                .get_result()
                .map(|result| result.text)
                .unwrap_or_default();
            output.push(RecognizedSegment {
                start_sample,
                end_sample,
                text,
            });
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

fn serve(model_dir: &Path) -> Result<(), String> {
    let mut worker = Worker::new(model_dir)?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    while let Some(header) = read_header(&mut input)? {
        if header.samples > MAX_SAMPLES {
            return Err("worker request audio exceeded limit".to_owned());
        }
        let mut raw = vec![0u8; header.samples * 4];
        input
            .read_exact(&mut raw)
            .map_err(|error| error.to_string())?;
        let samples: Vec<f32> = raw
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        let result = worker.process(header, &samples);
        let body = serde_json::to_vec(&result).map_err(|error| error.to_string())?;
        output
            .write_all(&(body.len() as u32).to_le_bytes())
            .and_then(|()| output.write_all(&body))
            .and_then(|()| output.flush())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn embed(model_path: &Path) -> Result<(), String> {
    let config = SpeakerEmbeddingExtractorConfig {
        model: Some(model_path.to_string_lossy().into_owned()),
        num_threads: 1,
        debug: false,
        provider: Some("cpu".to_owned()),
    };
    let extractor = SpeakerEmbeddingExtractor::create(&config)
        .ok_or_else(|| "speaker model could not load".to_owned())?;
    let mut input = io::stdin().lock();
    let mut count = [0u8; 4];
    input
        .read_exact(&mut count)
        .map_err(|error| error.to_string())?;
    let count = u32::from_le_bytes(count) as usize;
    if !(RATE as usize * 2..=RATE as usize * 15).contains(&count) {
        return Err("speaker sample duration invalid".to_owned());
    }
    let mut bytes = vec![0u8; count * 4];
    input
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|piece| f32::from_le_bytes(piece.try_into().unwrap()))
        .collect();
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err("speaker sample is invalid".to_owned());
    }
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
    serde_json::to_writer(io::stdout().lock(), &embedding).map_err(|error| error.to_string())
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, model_dir] if command == "serve" => serve(Path::new(model_dir)),
        [command, model_path] if command == "embed" => embed(Path::new(model_path)),
        [command, model_dir, wav] if command == "self-test" => {
            let worker = Worker::new(Path::new(model_dir))?;
            let wave = Wave::read(wav).ok_or_else(|| "cannot read test WAV".to_owned())?;
            let stream = worker.recognizer.create_stream();
            stream.accept_waveform(wave.sample_rate(), wave.samples());
            worker.recognizer.decode(&stream);
            let result = stream.get_result().ok_or_else(|| "no result".to_owned())?;
            println!("{}", result.text);
            Ok(())
        }
        _ => Err(
            "usage: hear-worker serve MODEL_DIR | embed MODEL_PATH | self-test MODEL_DIR WAV"
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
