use serde::Deserialize;
use sherpa_onnx::LinearResampler;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
pub struct RecognizedSegment {
    pub start_sample: u64,
    pub end_sample: u64,
    pub text: String,
}

#[derive(Deserialize)]
pub struct WorkResult {
    pub segments: Vec<RecognizedSegment>,
    pub speech_active: bool,
    pub error: Option<String>,
}

pub struct AnalysisClient {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

impl AnalysisClient {
    pub fn start(worker_path: &Path, model_dir: &Path) -> Result<Self, String> {
        let mut child = Command::new(worker_path)
            .arg("serve")
            .arg(model_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| "worker stdin unavailable".to_owned())?;
        let flags = unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            return Err("worker input could not be made nonblocking".to_owned());
        }
        let output = child
            .stdout
            .take()
            .ok_or_else(|| "worker stdout unavailable".to_owned())?;
        Ok(Self {
            child,
            input,
            output,
        })
    }

    pub fn analyze(
        &mut self,
        source: &str,
        start_48k: u64,
        mono_48k: &[f32],
        flush: bool,
    ) -> Result<WorkResult, String> {
        let resampler = LinearResampler::create(48_000, 16_000)
            .ok_or_else(|| "analysis resampler unavailable".to_owned())?;
        let mut samples = resampler.resample(mono_48k, true);
        // Blocks are independently decodable, but the worker's sample clock
        // must remain exact across them. Library flush padding is not part of
        // the source timeline.
        let expected = ((start_48k + mono_48k.len() as u64) / 3 - start_48k / 3) as usize;
        samples.truncate(expected);
        samples.resize(expected, 0.0);
        if samples.len() > 16_000 * 11 {
            return Err("analysis block was too long".to_owned());
        }
        let header = serde_json::json!({
            "source": source,
            "start_sample": start_48k / 3,
            "samples": samples.len(),
            "flush": flush,
        });
        let header = serde_json::to_vec(&header).map_err(|error| error.to_string())?;
        let mut frame_bytes = Vec::with_capacity(4 + header.len() + samples.len() * 4);
        frame_bytes.extend_from_slice(&(header.len() as u32).to_le_bytes());
        frame_bytes.extend_from_slice(&header);
        for sample in samples {
            frame_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        write_deadline(&mut self.input, &frame_bytes, Duration::from_secs(30))?;
        let mut size = [0; 4];
        read_deadline(&mut self.output, &mut size, Duration::from_secs(30))?;
        let size = u32::from_le_bytes(size) as usize;
        if size == 0 || size > 1_000_000 {
            return Err("worker response exceeded limit".to_owned());
        }
        let mut body = vec![0; size];
        read_deadline(&mut self.output, &mut body, Duration::from_secs(30))?;
        serde_json::from_slice(&body).map_err(|error| error.to_string())
    }
}

fn write_deadline(writer: &mut ChildStdin, bytes: &[u8], timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut offset = 0;
    while offset < bytes.len() {
        if Instant::now() >= deadline {
            return Err("worker input timed out".to_owned());
        }
        match writer.write(&bytes[offset..]) {
            Ok(0) => return Err("worker stopped accepting audio".to_owned()),
            Ok(written) => offset += written,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let mut descriptor = libc::pollfd {
                    fd: writer.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                };
                let ready = unsafe {
                    libc::poll(
                        &mut descriptor,
                        1,
                        remaining.as_millis().min(i32::MAX as u128) as i32,
                    )
                };
                if ready == 0 {
                    return Err("worker input timed out".to_owned());
                }
                if ready < 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

impl Drop for AnalysisClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_deadline(
    reader: &mut ChildStdout,
    bytes: &mut [u8],
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("worker response timed out".to_owned());
        }
        let mut descriptor = libc::pollfd {
            fd: reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if ready == 0 {
            return Err("worker response timed out".to_owned());
        }
        if ready < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let read = reader
            .read(&mut bytes[offset..])
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("worker exited before responding".to_owned());
        }
        offset += read;
    }
    Ok(())
}
