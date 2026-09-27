use bzip2::read::BzDecoder;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tar::Archive;
use uuid::Uuid;

const SENSE_ARCHIVE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2";
const SENSE_ARCHIVE_SHA: &str = "7d1efa2138a65b0b488df37f8b89e3d91a60676e416f515b952358d83dfd347e";
const SENSE_MODEL_SHA: &str = "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51";
const SENSE_TOKENS_SHA: &str = "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc";
const SILERO_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";
const SILERO_SHA: &str = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6";
const SENSE_ROOT: &str = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17";
const SPEAKER_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx";
const SPEAKER_SHA: &str = "f682b514c05d947ee3fa91cd6ec6c5c7543479a128373fa29b1faedccd21fd11";

pub fn installed_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("speech-v1")
}

pub fn speaker_model_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models/speaker-v1/model.onnx")
}

pub fn speaker_ready(data_dir: &Path) -> bool {
    verify(&speaker_model_path(data_dir), SPEAKER_SHA).is_ok()
}

pub fn install_speaker(data_dir: &Path) -> Result<PathBuf, String> {
    let target = speaker_model_path(data_dir);
    if speaker_ready(data_dir) {
        return Ok(target);
    }
    let root = data_dir.join("models");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let staging = root.join(format!(".speaker-installing-{}", Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let downloaded = staging.join("model.onnx");
        download(SPEAKER_URL, &downloaded, SPEAKER_SHA, 35_000_000, |_| {})?;
        let destination = root.join("speaker-v1");
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|error| error.to_string())?;
        }
        fs::rename(&staging, &destination).map_err(|error| error.to_string())?;
        File::open(&root)
            .and_then(|file| file.sync_all())
            .map_err(|error| error.to_string())?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

fn sha_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify(path: &Path, sha: &str) -> Result<(), String> {
    if sha_file(path)? == sha {
        Ok(())
    } else {
        Err(format!("model checksum mismatch: {}", path.display()))
    }
}

pub fn ready(data_dir: &Path) -> bool {
    let path = installed_path(data_dir);
    verify(&path.join("sensevoice/model.int8.onnx"), SENSE_MODEL_SHA).is_ok()
        && verify(&path.join("sensevoice/tokens.txt"), SENSE_TOKENS_SHA).is_ok()
        && verify(&path.join("silero_vad.onnx"), SILERO_SHA).is_ok()
}

fn download(
    url: &str,
    destination: &Path,
    expected_sha: &str,
    limit: u64,
    report: impl Fn(u64),
) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout_read(std::time::Duration::from_secs(60))
        .build();
    let response = agent.get(url).call().map_err(|error| error.to_string())?;
    let mut input = response.into_reader();
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        if bytes > limit {
            return Err("model download exceeded expected size".to_owned());
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        hasher.update(&buffer[..read]);
        report(bytes);
    }
    output.sync_all().map_err(|error| error.to_string())?;
    if format!("{:x}", hasher.finalize()) != expected_sha {
        return Err("model download checksum mismatch".to_owned());
    }
    Ok(())
}

fn extract_sense(archive_path: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|error| error.to_string())?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let input = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = Archive::new(BzDecoder::new(input));
    let mut extracted = 0;
    for entry in archive.entries().map_err(|error| error.to_string())? {
        let mut entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path().map_err(|error| error.to_string())?;
        let mut parts = path.components();
        if parts
            .next()
            .is_none_or(|part| part.as_os_str() != SENSE_ROOT)
        {
            return Err("unexpected model archive root".to_owned());
        }
        let file = parts.next();
        if parts.next().is_some() {
            continue; // test samples and upstream examples are not installed.
        }
        let name = match file.map(|part| part.as_os_str().to_string_lossy().into_owned()) {
            Some(name) if matches!(name.as_str(), "model.int8.onnx" | "tokens.txt" | "LICENSE") => {
                name
            }
            _ => continue,
        };
        if !entry.header().entry_type().is_file() || entry.size() > 300_000_000 {
            return Err("unexpected model archive entry".to_owned());
        }
        let path = destination.join(name);
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
        output.sync_all().map_err(|error| error.to_string())?;
        extracted += 1;
    }
    if extracted != 3 {
        return Err("SenseVoice archive was incomplete".to_owned());
    }
    Ok(())
}

/// Downloads independently pinned official model artifacts. A crash leaves an
/// unpublished staging directory, never a partly ready model pack.
pub fn install(data_dir: &Path, progress: impl Fn(u8)) -> Result<PathBuf, String> {
    let target = installed_path(data_dir);
    if ready(data_dir) {
        return Ok(target);
    }
    let root = data_dir.join("models");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    // A terminated installer can leave a full archive in a private staging
    // directory. No other installer runs in this single-owner core.
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".installing-")
            && entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
        {
            fs::remove_dir_all(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    let staging = root.join(format!(".installing-{}", Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let archive = staging.join("sensevoice.tar.bz2");
        download(
            SENSE_ARCHIVE_URL,
            &archive,
            SENSE_ARCHIVE_SHA,
            170_000_000,
            |bytes| {
                progress(((bytes.saturating_mul(70) / 163_002_883).min(70)) as u8);
            },
        )?;
        extract_sense(&archive, &staging.join("sensevoice"))?;
        progress(95);
        fs::remove_file(archive).map_err(|error| error.to_string())?;
        download(
            SILERO_URL,
            &staging.join("silero_vad.onnx"),
            SILERO_SHA,
            2_000_000,
            |_| {
                progress(98);
            },
        )?;
        verify(&staging.join("sensevoice/model.int8.onnx"), SENSE_MODEL_SHA)?;
        verify(&staging.join("sensevoice/tokens.txt"), SENSE_TOKENS_SHA)?;
        verify(&staging.join("silero_vad.onnx"), SILERO_SHA)?;
        if target.exists() {
            fs::remove_dir_all(&target).map_err(|error| error.to_string())?;
        }
        fs::rename(&staging, &target).map_err(|error| error.to_string())?;
        File::open(&root)
            .and_then(|file| file.sync_all())
            .map_err(|error| error.to_string())?;
        progress(100);
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_pack_stays_not_ready_with_partial_or_wrong_weights() {
        let directory = tempfile::tempdir().unwrap();
        let pack = installed_path(directory.path());
        fs::create_dir_all(pack.join("sensevoice")).unwrap();
        fs::write(pack.join("sensevoice/model.int8.onnx"), b"wrong").unwrap();
        assert!(!ready(directory.path()));
    }
}
