//! Bounded model RPC. Each model process owns computation only; the core owns
//! admission, persistent tasks and publication. One deadline covers the request.
use serde::{Deserialize, de::DeserializeOwned};
use sherpa_onnx::LinearResampler;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
pub struct SpeechRegion {
    pub start_sample: u64,
    pub end_sample: u64,
}
#[derive(Deserialize)]
pub struct VadResult {
    pub segments: Vec<SpeechRegion>,
    pub speech_active: bool,
    #[serde(default = "unknown_speech_pending")]
    pub speech_pending: bool,
    pub error: Option<String>,
}
fn unknown_speech_pending() -> bool {
    true
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub struct AnalysisClient {
    child: OwnedChild,
    input: ChildStdin,
    output: ChildStdout,
    last_used: Instant,
    cursors: std::collections::HashMap<String, (String, u64)>,
    resident: Option<crate::resources::Resident>,
}

pub fn resample_block(start_48k: u64, mono_48k: &[f32]) -> Result<Vec<f32>, String> {
    if mono_48k.len() > 48_000 * 11 {
        return Err("analysis block was too long".into());
    }
    let resampler =
        LinearResampler::create(48_000, 16_000).ok_or("analysis resampler unavailable")?;
    let mut samples = resampler.resample(mono_48k, true);
    let expected = ((start_48k + mono_48k.len() as u64) / 3 - start_48k / 3) as usize;
    samples.truncate(expected);
    samples.resize(expected, 0.0);
    Ok(samples)
}

impl AnalysisClient {
    pub fn start(worker_path: &Path, mode: &str, model_dir: &Path) -> Result<Self, String> {
        let mut child = OwnedChild(
            Command::new(worker_path)
                .arg(mode)
                .arg(model_dir)
                .arg("--parent-pid")
                .arg(std::process::id().to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?,
        );
        let input = child.0.stdin.take().ok_or("worker stdin unavailable")?;
        let output = child.0.stdout.take().ok_or("worker stdout unavailable")?;
        for fd in [input.as_raw_fd(), output.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err("worker pipe could not be made nonblocking".into());
            }
        }
        Ok(Self {
            child,
            input,
            output,
            last_used: Instant::now(),
            cursors: std::collections::HashMap::new(),
            resident: None,
        })
    }
    pub fn managed(
        mut self,
        governor: &std::sync::Arc<crate::resources::Governor>,
        work: crate::resources::Work,
    ) -> Self {
        self.resident = Some(governor.register(work, self.pid()));
        self
    }
    pub fn pid(&self) -> u32 {
        self.child.0.id()
    }
    pub fn idle_for(&self) -> Duration {
        self.last_used.elapsed()
    }
    pub fn needs_replay(&self, source: &str, session: &str, start: u64) -> bool {
        self.cursors
            .get(source)
            .is_none_or(|(prior, next)| prior != session || *next != start)
    }
    pub fn analyze(
        &mut self,
        source: &str,
        session: &str,
        start_48k: u64,
        pcm: &[f32],
        flush: bool,
    ) -> Result<VadResult, String> {
        let samples = resample_block(start_48k, pcm)?;
        let result: VadResult = self.request(
            source,
            session,
            start_48k / 3,
            &samples,
            flush,
            Duration::from_secs(30),
            &|| false,
        )?;
        if result.error.is_none() {
            if flush {
                self.cursors.remove(source);
            } else {
                self.cursors.insert(
                    source.to_owned(),
                    (session.to_owned(), start_48k + pcm.len() as u64),
                );
            }
        }
        Ok(result)
    }
    pub fn request<T: DeserializeOwned>(
        &mut self,
        source: &str,
        session: &str,
        start: u64,
        samples: &[f32],
        flush: bool,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<T, String> {
        if samples.len() > 16_000 * 30 || samples.iter().any(|v| !v.is_finite()) {
            return Err("worker audio exceeded limit".into());
        }
        let deadline = Instant::now() + timeout;
        self.last_used = Instant::now();
        let header = serde_json::to_vec(&serde_json::json!({"source":source,"session":session,"start_sample":start,"samples":samples.len(),"flush":flush})).map_err(|e|e.to_string())?;
        let mut frame = Vec::with_capacity(4 + header.len() + samples.len() * 4);
        frame.extend_from_slice(&(header.len() as u32).to_le_bytes());
        frame.extend_from_slice(&header);
        for sample in samples {
            frame.extend_from_slice(&sample.to_le_bytes());
        }
        let mut offset = 0;
        while offset < frame.len() {
            wait_fd(self.input.as_raw_fd(), libc::POLLOUT, deadline, cancelled)?;
            match self.input.write(&frame[offset..]) {
                Ok(0) => return Err("worker_failed".into()),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return Err("worker_failed".into()),
            }
        }
        let mut size = [0; 4];
        read(&mut self.output, &mut size, deadline, cancelled)?;
        let size = u32::from_le_bytes(size) as usize;
        if size == 0 || size > 1_000_000 {
            return Err("worker response exceeded limit".into());
        }
        let mut body = vec![0; size];
        read(&mut self.output, &mut body, deadline, cancelled)?;
        self.last_used = Instant::now();
        serde_json::from_slice(&body).map_err(|_| "model_invalid".to_owned())
    }
}
fn wait_fd(
    fd: i32,
    events: i16,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    loop {
        if cancelled() {
            return Err("resource_deferred".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("worker_timeout".into());
        }
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let ready = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                remaining.as_millis().clamp(1, 100) as i32,
            )
        };
        if ready > 0 {
            return Ok(());
        }
        if ready < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err("worker_failed".into());
        }
    }
}
fn read(
    reader: &mut ChildStdout,
    bytes: &mut [u8],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let mut offset = 0;
    while offset < bytes.len() {
        wait_fd(reader.as_raw_fd(), libc::POLLIN, deadline, cancelled)?;
        match reader.read(&mut bytes[offset..]) {
            Ok(0) => return Err("worker_failed".into()),
            Ok(n) => offset += n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err("worker_failed".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture(code: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker");
        std::fs::write(&path, format!("#!/usr/bin/python3\n{code}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        (dir, path)
    }
    #[test]
    fn pressure_cancellation_reaps_worker_and_preserves_one_request_deadline() {
        let (_dir, path) = fixture("import time\ntime.sleep(10)");
        let mut worker = AnalysisClient::start(&path, "embed", Path::new("model")).unwrap();
        let pid = worker.pid();
        let started = Instant::now();
        let result: Result<Vec<f32>, _> = worker.request(
            "microphone",
            "s",
            0,
            &[0.; 32],
            true,
            Duration::from_secs(3),
            &|| started.elapsed() > Duration::from_millis(30),
        );
        assert_eq!(result.err().unwrap(), "resource_deferred");
        assert!(started.elapsed() < Duration::from_millis(500));
        drop(worker);
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }
    #[test]
    fn response_header_and_payload_share_the_request_deadline() {
        let (_dir, path) = fixture(
            "import sys,time,struct\nn=struct.unpack('<I',sys.stdin.buffer.read(4))[0]\nsys.stdin.buffer.read(n)\ntime.sleep(.06)\nsys.stdout.buffer.write(struct.pack('<I',2));sys.stdout.buffer.flush()\ntime.sleep(.06)\nsys.stdout.buffer.write(b'[]');sys.stdout.buffer.flush()",
        );
        let mut worker = AnalysisClient::start(&path, "embed", Path::new("model")).unwrap();
        let result: Result<Vec<f32>, _> = worker.request(
            "microphone",
            "s",
            0,
            &[],
            true,
            Duration::from_millis(100),
            &|| false,
        );
        assert_eq!(result.err().unwrap(), "worker_timeout");
    }
    #[test]
    fn each_source_and_session_has_its_own_replay_cursor() {
        let (_dir, path) = fixture(
            "import sys,struct,json\nwhile True:\n size=sys.stdin.buffer.read(4)\n if not size:break\n h=json.loads(sys.stdin.buffer.read(struct.unpack('<I',size)[0]));sys.stdin.buffer.read(h['samples']*4)\n body=json.dumps({'segments':[],'speech_active':True,'error':None}).encode()\n sys.stdout.buffer.write(struct.pack('<I',len(body))+body);sys.stdout.buffer.flush()",
        );
        let mut worker = AnalysisClient::start(&path, "vad", Path::new("model")).unwrap();
        assert!(worker.needs_replay("microphone", "a", 0));
        worker
            .analyze("microphone", "a", 0, &[0.; 480], false)
            .unwrap();
        assert!(!worker.needs_replay("microphone", "a", 480));
        assert!(worker.needs_replay("system", "b", 480));
        worker.analyze("system", "b", 0, &[0.; 480], false).unwrap();
        assert!(!worker.needs_replay("system", "b", 480));
        assert!(worker.needs_replay("microphone", "new-session", 480));
        assert!(worker.needs_replay("microphone", "a", 960));
    }
}
