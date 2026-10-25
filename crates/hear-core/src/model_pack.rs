use hear_protocol::ModelPhase;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// Pin the exact measured files. The same-named GitHub tarball has different
// encoder/decoder bytes and is not interchangeable with this evaluated revision.
const XASR_BASE_URL: &str = "https://huggingface.co/csukuangfj2/sherpa-onnx-x-asr-zipformer-transducer-zh-en-int8-2026-06-03/resolve/bbc3ebbf8f134848f804967f3a3b69f981308a85";
const XASR_LICENSE_URL: &str = "https://raw.githubusercontent.com/Gilgamesh-J/X-ASR/838297cd47fed858e6cf72eaf5a52f948a3edd73/LICENSE";
pub const COARSE_MODEL_ID: &str = "x-asr-zipformer-transducer-zh-en-int8-2026-06-03";
const XASR_FILES: &[(&str, &str, u64)] = &[
    (
        "encoder-epoch-99-avg-1.int8.onnx",
        "2964d4afd257762931184b15fac04bcae79158b6b84fd9441fc33c8eb5a360ab",
        161_744_450,
    ),
    (
        "decoder-epoch-99-avg-1.onnx",
        "c6a045cd52d4d2693c19218f404be6e37b6130e1244814ce31fa2188e4355cb5",
        11_309_084,
    ),
    (
        "joiner-epoch-99-avg-1.int8.onnx",
        "82e88dad29301e29144565cd3b7347fa163f07ddb30ea60346b7c4c536adf4ba",
        2_581_422,
    ),
    (
        "tokens.txt",
        "ad0df596d9cc60c90795409549ccf7d1c4f901bdc060dd6b2b067d9ef2fddfaf",
        59_240,
    ),
    (
        "LICENSE",
        "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4",
        11357,
    ),
];
const SILERO_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";
const SILERO_SHA: &str = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6";
const SPEAKER_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx";
const SPEAKER_SHA: &str = "f682b514c05d947ee3fa91cd6ec6c5c7543479a128373fa29b1faedccd21fd11";

pub fn installed_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("speech-xasr-v1")
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
    xasr_ready(&path) && verify(&path.join("silero_vad.onnx"), SILERO_SHA).is_ok()
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
    // Keep hash-valid individual files; partial downloads never become ready.
    let staging = root.join(".installing-speech-xasr-v1");
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let asr = staging.join("xasr");
    fs::create_dir_all(&asr).map_err(|e| e.to_string())?;
    // The previous pack may supply the unchanged VAD, never the new ASR.
    let mut candidates = vec![target.clone(), root.join("speech-v1")];
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
    let vad = staging.join("silero_vad.onnx");
    let vad_missing = verify(&vad, SILERO_SHA).is_err();
    let required: u64 = XASR_FILES
        .iter()
        .filter(|(name, sha, _)| verify(&asr.join(name), sha).is_err())
        .map(|(_, _, bytes)| *bytes)
        .sum::<u64>()
        + if vad_missing { 643_854 } else { 0 };
    check_space(data_dir, required)?;
    for (name, sha, bytes) in XASR_FILES {
        let path = asr.join(name);
        if verify(&path, sha).is_ok() {
            continue;
        }
        remove_if_present(&path)?;
        if let Some(source) = candidates
            .iter()
            .find(|p| verify(&p.join("xasr").join(name), sha).is_ok())
        {
            copy_synced(&source.join("xasr").join(name), &path)?;
        } else {
            progress(ModelPhase::Downloading, 0);
            let url = if *name == "LICENSE" {
                XASR_LICENSE_URL.to_owned()
            } else {
                format!("{XASR_BASE_URL}/{name}")
            };
            download(&url, &path, sha, *bytes, |n| {
                progress(ModelPhase::Downloading, ((n * 100 / bytes).min(100)) as u8)
            })?;
        }
    }
    progress(ModelPhase::Verifying, 100);
    if !xasr_ready(&staging) {
        return Err("model_checksum".into());
    }
    if vad_missing {
        remove_if_present(&vad)?;
        if let Some(source) = candidates
            .iter()
            .find(|p| verify(&p.join("silero_vad.onnx"), SILERO_SHA).is_ok())
        {
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
    verify(&vad, SILERO_SHA)?;
    progress(ModelPhase::Installing, 100);
    if target.exists() {
        fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    }
    fs::rename(&staging, &target).map_err(|e| e.to_string())?;
    File::open(&root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    clean_staging(&root, ".installing-")?;
    Ok(target)
}
fn xasr_ready(root: &Path) -> bool {
    XASR_FILES
        .iter()
        .all(|(name, sha, _)| verify(&root.join("xasr").join(name), sha).is_ok())
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
        fs::create_dir_all(pack.join("xasr")).unwrap();
        fs::write(pack.join("xasr/encoder-epoch-99-avg-1.int8.onnx"), b"wrong").unwrap();
        assert!(!ready(directory.path()));
    }

    #[test]
    fn previous_speech_pack_is_not_the_selected_model() {
        let directory = tempfile::tempdir().unwrap();
        let previous = directory.path().join("models/speech-v1");
        fs::create_dir_all(previous.join("sensevoice")).unwrap();
        fs::write(previous.join("silero_vad.onnx"), b"old VAD").unwrap();
        assert_ne!(installed_path(directory.path()), previous);
        assert!(!ready(directory.path()));
        assert!(previous.join("silero_vad.onnx").is_file());
    }

    #[test]
    #[ignore = "requires the pinned local X-ASR fixture"]
    fn selected_pack_verifies_each_file_and_reuses_legacy_vad() {
        let source =
            PathBuf::from(std::env::var("HEAR_TEST_XASR_MODELS").expect("set fixture root"));
        let directory = tempfile::tempdir().unwrap();
        let staging = directory
            .path()
            .join("models/.installing-speech-xasr-v1/xasr");
        fs::create_dir_all(&staging).unwrap();
        for (name, sha, _) in XASR_FILES {
            verify(&source.join("speech-xasr-v1/xasr").join(name), sha).unwrap();
            copy_synced(
                &source.join("speech-xasr-v1/xasr").join(name),
                &staging.join(name),
            )
            .unwrap();
        }
        let legacy = directory.path().join("models/speech-v1");
        fs::create_dir_all(&legacy).unwrap();
        copy_synced(
            &source.join("speech-xasr-v1/silero_vad.onnx"),
            &legacy.join("silero_vad.onnx"),
        )
        .unwrap();
        assert!(!ready(directory.path()));
        let downloaded = std::cell::Cell::new(false);
        let pack = install(directory.path(), |phase, _| {
            if matches!(phase, ModelPhase::Downloading) {
                downloaded.set(true);
            }
        })
        .unwrap();
        assert!(!downloaded.get());
        assert!(ready(directory.path()));
        verify(&legacy.join("silero_vad.onnx"), SILERO_SHA).unwrap();
        for (name, _, _) in XASR_FILES {
            let file = pack.join("xasr").join(name);
            let saved = fs::read(&file).unwrap();
            fs::remove_file(&file).unwrap();
            assert!(!ready(directory.path()), "missing {name}");
            fs::write(&file, b"wrong").unwrap();
            assert!(!ready(directory.path()), "corrupt {name}");
            fs::write(&file, saved).unwrap();
        }
        assert!(ready(directory.path()));
    }
}
