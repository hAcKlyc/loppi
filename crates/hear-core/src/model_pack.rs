use bzip2::read::BzDecoder;
use hear_protocol::ModelPhase;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tar::Archive;

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

pub fn install_speaker(
    data_dir: &Path,
    progress: impl Fn(ModelPhase, u8),
) -> Result<PathBuf, String> {
    let target = speaker_model_path(data_dir);
    if speaker_ready(data_dir) {
        return Ok(target);
    }
    let root = data_dir.join("models");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let staging = root.join(".speaker-installing-v1");
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let downloaded = staging.join("model.onnx");
        if verify(&downloaded, SPEAKER_SHA).is_err() {
            remove_if_present(&downloaded)?;
            check_space(data_dir, 28_281_138)?;
            let previous = fs::read_dir(&root)
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .find(|entry| {
                    entry.path() != staging
                        && entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".speaker-installing-")
                        && entry.file_type().is_ok_and(|t| t.is_dir())
                        && verify(&entry.path().join("model.onnx"), SPEAKER_SHA).is_ok()
                });
            if let Some(previous) = previous {
                copy_synced(&previous.path().join("model.onnx"), &downloaded)?;
            } else {
                progress(ModelPhase::Downloading, 0);
                download(SPEAKER_URL, &downloaded, SPEAKER_SHA, 35_000_000, |n| {
                    progress(
                        ModelPhase::Downloading,
                        ((n * 100 / 28_281_138).min(100)) as u8,
                    )
                })?;
            }
        }
        progress(ModelPhase::Verifying, 100);
        verify(&downloaded, SPEAKER_SHA)?;
        progress(ModelPhase::Installing, 100);
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
    if result.is_ok() {
        clean_staging(&root, ".speaker-installing-")?;
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
    let response = agent
        .get(url)
        .call()
        .map_err(|_| "model_network".to_owned())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
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
        if std::time::Instant::now() >= deadline {
            return Err("model_network_timeout".into());
        }
        let read = input
            .read(&mut buffer)
            .map_err(|_| "model_network".to_owned())?;
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
pub fn install(data_dir: &Path, progress: impl Fn(ModelPhase, u8)) -> Result<PathBuf, String> {
    let target = installed_path(data_dir);
    if ready(data_dir) {
        return Ok(target);
    }
    let root = data_dir.join("models");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    // A fixed private staging directory belongs to the single Core installer.
    // Retain only hash-valid complete resources across retries/restarts. It is
    // never a published ready pack and provides no byte-range resume.
    let staging = root.join(".installing-speech-v1");
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let mut candidates = vec![target.clone()];
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path() != staging
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with(".installing-")
            && entry.file_type().map_err(|e| e.to_string())?.is_dir()
        {
            candidates.push(entry.path());
        }
    }
    progress(ModelPhase::Checking, 0);
    let sense_source = candidates.iter().find(|p| sense_ready(p));
    let vad_source = candidates
        .iter()
        .find(|p| verify(&p.join("silero_vad.onnx"), SILERO_SHA).is_ok());
    let sense_missing = !sense_ready(&staging);
    let vad_missing = verify(&staging.join("silero_vad.onnx"), SILERO_SHA).is_err();
    let archive = staging.join("sensevoice.tar.bz2");
    let archive_valid = verify(&archive, SENSE_ARCHIVE_SHA).is_ok();
    let required = if sense_missing {
        239_233_841
            + 315_894
            + 71
            + if sense_source.is_none() && !archive_valid {
                163_002_883
            } else {
                0
            }
    } else {
        0
    } + if vad_missing { 643_854 } else { 0 };
    if sense_missing && staging.join("sensevoice").exists() {
        fs::remove_dir_all(staging.join("sensevoice")).map_err(|e| e.to_string())?;
    }
    if !archive_valid {
        remove_if_present(&archive)?;
    }
    if vad_missing {
        remove_if_present(&staging.join("silero_vad.onnx"))?;
    }
    check_space(data_dir, required)?;
    let result = (|| {
        if sense_missing {
            let sense = staging.join("sensevoice");
            if sense.exists() {
                fs::remove_dir_all(&sense).map_err(|e| e.to_string())?;
            }
            if let Some(source) = sense_source {
                fs::create_dir(&sense).map_err(|e| e.to_string())?;
                for name in ["model.int8.onnx", "tokens.txt", "LICENSE"] {
                    copy_synced(&source.join("sensevoice").join(name), &sense.join(name))?;
                }
            } else {
                if !archive_valid {
                    remove_if_present(&archive)?;
                    // Older interrupted installers may have a complete archive.
                    if let Some(source) = candidates
                        .iter()
                        .find(|p| verify(&p.join("sensevoice.tar.bz2"), SENSE_ARCHIVE_SHA).is_ok())
                    {
                        copy_synced(&source.join("sensevoice.tar.bz2"), &archive)?;
                    } else {
                        progress(ModelPhase::Downloading, 0);
                        download(
                            SENSE_ARCHIVE_URL,
                            &archive,
                            SENSE_ARCHIVE_SHA,
                            170_000_000,
                            |n| {
                                progress(
                                    ModelPhase::Downloading,
                                    ((n * 100 / 163_002_883).min(100)) as u8,
                                )
                            },
                        )?;
                    }
                }
                progress(ModelPhase::Installing, 0);
                extract_sense(&archive, &sense)?;
            }
            progress(ModelPhase::Verifying, 100);
            if !sense_ready(&staging) {
                return Err("model_checksum".into());
            }
        }
        remove_if_present(&archive)?;
        if vad_missing {
            let vad = staging.join("silero_vad.onnx");
            remove_if_present(&vad)?;
            if let Some(source) = vad_source {
                copy_synced(&source.join("silero_vad.onnx"), &vad)?;
            } else {
                progress(ModelPhase::Downloading, 0);
                download(SILERO_URL, &vad, SILERO_SHA, 2_000_000, |n| {
                    progress(
                        ModelPhase::Downloading,
                        ((n * 100 / 643_854).min(100)) as u8,
                    )
                })?;
            }
        }
        progress(ModelPhase::Verifying, 100);
        verify(&staging.join("silero_vad.onnx"), SILERO_SHA)?;
        progress(ModelPhase::Installing, 100);
        if target.exists() {
            fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
        }
        fs::rename(&staging, &target).map_err(|e| e.to_string())?;
        File::open(&root)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        clean_staging(&root, ".installing-")?;
        Ok(target.clone())
    })();
    // Keep complete components after a failed later download. On the next
    // attempt invalid files are discarded before retrying their download.
    result
}
fn sense_ready(root: &Path) -> bool {
    verify(&root.join("sensevoice/model.int8.onnx"), SENSE_MODEL_SHA).is_ok()
        && verify(&root.join("sensevoice/tokens.txt"), SENSE_TOKENS_SHA).is_ok()
        && root.join("sensevoice/LICENSE").is_file()
}
fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn copy_synced(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map_err(|e| e.to_string())?;
    File::open(to)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

fn clean_staging(root: &Path, prefix: &str) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name().to_string_lossy().starts_with(prefix)
            && entry.file_type().map_err(|e| e.to_string())?.is_dir()
        {
            fs::remove_dir_all(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn check_space(directory: &Path, staging: u64) -> Result<(), String> {
    let free = fs2::available_space(directory).map_err(|_| "model_disk_check".to_owned())?;
    if free < staging + 2 * 1_073_741_824 {
        return Err("model_disk_space".into());
    }
    Ok(())
}
pub fn load_error_code(error: &str) -> String {
    match error {
        "resource_deferred" => "model_resources_busy",
        "worker_timeout" => "model_load_timeout",
        _ => "model_load_failed",
    }
    .into()
}
pub fn error_code(error: &str) -> String {
    if error.starts_with("model_") {
        return error.into();
    }
    if error.contains("checksum") {
        "model_checksum"
    } else if error.contains("archive") {
        "model_archive"
    } else if error.contains("No space") {
        "model_disk_space"
    } else {
        "model_install"
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn load_cancellation_is_resource_pressure_not_model_corruption() {
        assert_eq!(load_error_code("resource_deferred"), "model_resources_busy");
        assert_eq!(load_error_code("worker_timeout"), "model_load_timeout");
        assert_eq!(load_error_code("worker_failed"), "model_load_failed");
    }

    #[test]
    fn model_pack_stays_not_ready_with_partial_or_wrong_weights() {
        let directory = tempfile::tempdir().unwrap();
        let pack = installed_path(directory.path());
        fs::create_dir_all(pack.join("sensevoice")).unwrap();
        fs::write(pack.join("sensevoice/model.int8.onnx"), b"wrong").unwrap();
        assert!(!ready(directory.path()));
    }
}
