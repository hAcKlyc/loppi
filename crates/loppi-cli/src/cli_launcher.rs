//! The existing install-cli action owns this two-file Windows launcher.
//! The installed command forwards argv/stdio to the real installation, so Core
//! discovery and upgrades always use that installation's current executable.
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const OWNER: &str = "app.loppi.cli-launcher.v1";
const TARGET: &str = ".loppi-cli-target.json";

fn target(directory: &Path) -> Result<PathBuf, String> {
    let mut bytes = Vec::new();
    loppi_protocol::platform::OpenOptions::new()
        .read(true)
        .open(directory.join(TARGET))
        .map_err(|e| e.to_string())?
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("CLI launcher metadata too large".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "invalid CLI launcher metadata")?;
    if value["owner"] != OWNER {
        return Err("unrelated CLI launcher metadata".into());
    }
    let installed = directory.join("loppi.exe");
    let expected = value["launcher_bytes"]
        .as_u64()
        .ok_or("CLI launcher size missing")?;
    if expected == 0
        || expected > 128 * 1024 * 1024
        || fs::metadata(&installed).map_err(|e| e.to_string())?.len() != expected
    {
        return Err("Installed loppi launcher was modified; preserving it".into());
    }
    let hash = format!(
        "{:x}",
        Sha256::digest(fs::read(installed).map_err(|e| e.to_string())?)
    );
    if value["launcher_sha256"] != hash {
        return Err("Installed loppi launcher was modified; preserving it".into());
    }
    let path = PathBuf::from(
        value["target"]
            .as_str()
            .ok_or("missing CLI installation target")?,
    );
    if !path.is_absolute() {
        return Err("CLI installation target must be absolute".into());
    }
    Ok(path)
}

pub fn install(directory: &Path, source: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let directory = fs::canonicalize(directory).map_err(|e| e.to_string())?;
    let installed = directory.join("loppi.exe");
    if source == installed {
        return Ok(installed);
    }
    let metadata = directory.join(TARGET);
    if fs::symlink_metadata(&installed).is_ok() || fs::symlink_metadata(&metadata).is_ok() {
        if installed.is_file() && target(&directory).ok().as_deref() == Some(source) {
            return Ok(installed);
        }
        return Err(
            "An unrelated loppi command or launcher metadata exists in this directory".into(),
        );
    }
    let bytes=serde_json::to_vec(&json!({"owner":OWNER,"target":source,"launcher_bytes":fs::metadata(source).map_err(|e|e.to_string())?.len(),"launcher_sha256":format!("{:x}",Sha256::digest(fs::read(source).map_err(|e|e.to_string())?))})).map_err(|e|e.to_string())?;
    let mut file = loppi_protocol::platform::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&metadata)
        .map_err(|e| e.to_string())?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&metadata);
        return Err(error.to_string());
    }
    drop(file);
    let mut created_binary = false;
    let result = (|| {
        let mut output = loppi_protocol::platform::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&installed)
            .map_err(|e| e.to_string())?;
        created_binary = true;
        std::io::copy(
            &mut fs::File::open(source).map_err(|e| e.to_string())?,
            &mut output,
        )
        .map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        Ok(installed.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&metadata);
        if created_binary {
            let _ = fs::remove_file(&installed);
        }
    }
    result
}

pub fn forward_if_installed() -> Result<Option<i32>, String> {
    let own = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = own.parent().ok_or("CLI directory missing")?;
    if fs::symlink_metadata(directory.join(TARGET)).is_err() {
        return Ok(None);
    }
    let actual = target(directory)?;
    if actual == own
        || actual
            .parent()
            .is_some_and(|parent| parent.join(TARGET).exists())
    {
        return Err("CLI launcher target must be the real loppi installation".into());
    }
    let status = loppi_protocol::platform::command(actual)
        .args(std::env::args_os().skip(1))
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| format!("cannot start installed loppi CLI: {e}"))?;
    Ok(Some(status.code().unwrap_or(1)))
}
