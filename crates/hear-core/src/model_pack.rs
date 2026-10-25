//! Immutable model resources. The Core owns readiness; workers only read paths.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

const LOCK: &[u8] = include_bytes!("../../../config/models.lock.json");

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    model_version: String,
    speech_directory: String,
    speaker_file: String,
    files: Vec<Asset>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    path: String,
    kind: String,
    url: String,
    bytes: u64,
    sha256: String,
}

fn relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && !path.contains('\\')
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    let value: Manifest = serde_json::from_slice(bytes).map_err(|_| "model_manifest")?;
    let mut paths = HashSet::new();
    if value.schema != 1
        || value.model_version.is_empty()
        || !relative(&value.speech_directory)
        || !relative(&value.speaker_file)
        || value.files.is_empty()
        || value.files.iter().any(|f| {
            !relative(&f.path)
                || !paths.insert(&f.path)
                || !matches!(f.kind.as_str(), "speech" | "speaker")
                || f.bytes == 0
                || f.sha256.len() != 64
                || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || !f.url.starts_with("https://")
        })
        || !value.files.iter().any(|f| f.kind == "speech")
        || !value
            .files
            .iter()
            .any(|f| f.path == value.speaker_file && f.kind == "speaker")
    {
        return Err("model_manifest".into());
    }
    Ok(value)
}

fn manifest() -> &'static Manifest {
    static VALUE: OnceLock<Manifest> = OnceLock::new();
    // This is compiled source, validated by the same release preflight and tests.
    VALUE.get_or_init(|| parse_manifest(LOCK).expect("invalid compiled model lock"))
}

pub fn coarse_model_id() -> &'static str {
    &manifest().model_version
}

#[derive(Clone, Debug)]
pub struct Assets {
    root: PathBuf,
}

impl Assets {
    pub fn discover() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|_| "model_bundle_missing")?;
        // Standalone development/test Core can use an explicit build cache.
        // Packaged Core always uses its own bundle, including debug bundles.
        #[cfg(debug_assertions)]
        let development = std::env::var_os("HEAR_MODEL_ROOT").map(PathBuf::from);
        #[cfg(not(debug_assertions))]
        let development: Option<PathBuf> = None;
        Ok(Self {
            root: Self::root_for(&exe, development.as_deref()),
        })
    }

    fn root_for(exe: &Path, development: Option<&Path>) -> PathBuf {
        let parent = exe.parent().unwrap_or_else(|| Path::new("."));
        if parent.file_name().is_some_and(|n| n == "MacOS") {
            return parent.parent().unwrap_or(parent).join("Resources/models");
        }
        development
            .map(Path::to_path_buf)
            .unwrap_or_else(|| parent.join("resources/models"))
    }

    pub fn speech_directory(&self) -> PathBuf {
        self.root.join(&manifest().speech_directory)
    }

    pub fn speaker_file(&self) -> PathBuf {
        self.root.join(&manifest().speaker_file)
    }

    pub fn verify(&self, speaker: bool) -> Result<(), String> {
        let lock = self.root.join("models.lock.json");
        if fs::symlink_metadata(&lock)
            .map_err(|_| "model_bundle_missing")?
            .file_type()
            .is_symlink()
        {
            return Err("model_manifest".into());
        }
        if fs::read(&lock).map_err(|_| "model_bundle_unreadable")? != LOCK {
            return Err("model_manifest".into());
        }
        let kind = if speaker { "speaker" } else { "speech" };
        for asset in manifest().files.iter().filter(|f| f.kind == kind) {
            verify_file(&self.root, asset)?;
        }
        Ok(())
    }
}

fn verify_file(root: &Path, asset: &Asset) -> Result<(), String> {
    let path = root.join(&asset.path);
    let meta = fs::symlink_metadata(&path).map_err(|_| "model_bundle_missing")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != asset.bytes {
        return Err("model_checksum".into());
    }
    let mut file = fs::File::open(&path).map_err(|_| "model_bundle_unreadable")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| "model_bundle_unreadable")?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != asset.sha256 {
        return Err("model_checksum".into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_lock_is_complete_and_paths_cannot_escape() {
        let m = parse_manifest(LOCK).unwrap();
        assert_eq!(m.files.len(), 9);
        assert_eq!(
            m.model_version,
            "x-asr-zipformer-transducer-zh-en-int8-2026-06-03"
        );
        for path in [
            "../outside",
            "/tmp/model",
            "models/../other",
            "models\\other",
            "",
        ] {
            let mut value: serde_json::Value = serde_json::from_slice(LOCK).unwrap();
            value["files"][0]["path"] = path.into();
            assert!(
                parse_manifest(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{path}"
            );
        }
        let mut value: serde_json::Value = serde_json::from_slice(LOCK).unwrap();
        value["files"][1]["path"] = value["files"][0]["path"].clone();
        assert!(parse_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn core_bundle_location_is_independent_of_gui_and_cwd() {
        let exe = Path::new(
            "/tmp/测试 Hear.app/Contents/Library/LoginItems/Renamed.app/Contents/MacOS/hear-core",
        );
        assert_eq!(
            Assets::root_for(exe, Some(Path::new("/ignored/cache"))),
            exe.parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("Resources/models")
        );
        assert_eq!(
            Assets::root_for(
                Path::new("/tmp/debug/hear-core"),
                Some(Path::new("/explicit/cache"))
            ),
            Path::new("/explicit/cache")
        );
    }

    #[test]
    fn old_downloads_do_not_supply_a_missing_bundle() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("models/speech-xasr-v1")).unwrap();
        let assets = Assets {
            root: temp.path().join("CoreResources/models"),
        };
        assert_eq!(assets.verify(false).unwrap_err(), "model_bundle_missing");
        assert!(!assets.root.exists());
    }

    #[test]
    fn immutable_files_reject_same_size_corruption_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let asset = Asset {
            path: "model".into(),
            kind: "speech".into(),
            url: "https://example.test/model".into(),
            bytes: 4,
            sha256: format!("{:x}", Sha256::digest(b"good")),
        };
        let file = temp.path().join("model");
        fs::write(&file, b"good").unwrap();
        verify_file(temp.path(), &asset).unwrap();
        fs::write(&file, b"evil").unwrap();
        assert_eq!(
            verify_file(temp.path(), &asset).unwrap_err(),
            "model_checksum"
        );
        fs::remove_file(&file).unwrap();
        fs::write(temp.path().join("other"), b"good").unwrap();
        std::os::unix::fs::symlink("other", &file).unwrap();
        assert_eq!(
            verify_file(temp.path(), &asset).unwrap_err(),
            "model_checksum"
        );
    }

    #[test]
    fn resource_pressure_is_not_a_checksum_failure() {
        assert_eq!(load_error_code("resource_deferred"), "model_resources_busy");
        assert_eq!(load_error_code("worker_timeout"), "model_load_timeout");
    }
}
