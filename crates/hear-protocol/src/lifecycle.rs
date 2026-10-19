//! GUI-session ownership coordination. Never reads or writes the business DB.
use super::*;
use fs2::FileExt;
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::sync::{RwLock, RwLockReadGuard};

#[derive(Default)]
pub(crate) struct Session {
    ready: bool,
    stopped: bool,
}
static SESSION: RwLock<Session> = RwLock::new(Session {
    ready: false,
    stopped: false,
});
const WAIT: Duration = Duration::from_secs(30);

pub(crate) fn gui_request() -> Result<RwLockReadGuard<'static, Session>, String> {
    let read = SESSION.read().map_err(|e| e.to_string())?;
    if read.stopped {
        return Err("Hear 正在退出".into());
    }
    if read.ready {
        return Ok(read);
    }
    drop(read);
    let mut session = SESSION.write().map_err(|e| e.to_string())?;
    if session.stopped {
        return Err("Hear 正在退出".into());
    }
    if !session.ready {
        ensure_current()?;
        session.ready = true;
    }
    drop(session);
    let read = SESSION.read().map_err(|e| e.to_string())?;
    if read.stopped {
        return Err("Hear 正在退出".into());
    }
    Ok(read)
}

/// Drain GUI requests first; unlike send_from(Quit), never starts a Core to stop it.
pub fn quit_gui_core() -> Result<(), String> {
    let mut session = SESSION.write().map_err(|e| e.to_string())?;
    session.stopped = true;
    let directory = data_dir()?;
    if !directory.exists() {
        return Ok(());
    }
    let _gate = lock(&directory.join("lifecycle.lock"))?;
    if let Some(owner) = inspect()? {
        stop(&owner, false)?;
    }
    wait_stopped(&directory)
}

enum Owner {
    Current { build: BuildInfo },
    Legacy { version: u32 },
}

// Raw JSON intentionally decouples this stable handshake from ServiceState.
// Only Status and Quit use the explicitly bounded, released legacy dialects.
fn inspect() -> Result<Option<Owner>, String> {
    let status = match exchange(PROTOCOL_VERSION, json!({"type":"status"})) {
        Ok(value) => value,
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::ConnectionRefused =>
        {
            return Ok(None);
        }
        Err(e) => return Err(format!("无法连接 Hear 后台：{e}")),
    };
    if status["type"] == "error" {
        let version = status["message"]
            .as_str()
            .and_then(|s| s.strip_prefix("expected protocol version "))
            .and_then(|s| s.parse::<u32>().ok());
        if status["code"] == "protocol_version" {
            if let Some(version @ 17..=19) = version {
                return Ok(Some(Owner::Legacy { version }));
            }
        }
        return Err(format!(
            "Hear 后台版本不兼容：{}",
            status["message"].as_str().unwrap_or("未知错误")
        ));
    }
    require_type(&status, "status")?;
    let reply = exchange(
        PROTOCOL_VERSION,
        json!({"type":"lifecycle","action":"inspect"}),
    )
    .map_err(|e| format!("无法检查 Hear 后台版本：{e}"))?;
    require_type(&reply, "lifecycle")?;
    let build = serde_json::from_value(reply["build"].clone())
        .map_err(|e| format!("无效的后台版本：{e}"))?;
    Ok(Some(Owner::Current { build }))
}

fn ensure_current() -> Result<(), String> {
    let directory = data_dir()?;
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    let _gate = lock(&directory.join("lifecycle.lock"))?;
    let owner = inspect()?;
    let expected = build_info("core");
    if let Some(Owner::Current { build }) = &owner {
        if build == &expected {
            return Ok(());
        }
        if newer(build, &expected)? {
            return Err("后台来自更新的 Hear，请打开已安装的最新版本".into());
        }
    }
    // Validate the replacement before asking the running owner to leave.
    let binary = core_binary_path()?;
    let output = Command::new(&binary)
        .arg("--build-info")
        .output()
        .map_err(|e| e.to_string())?;
    let bundled: BuildInfo = serde_json::from_slice(&output.stdout)
        .map_err(|_| "安装包后台版本无效，请重新安装完整的 Hear")?;
    if !output.status.success() || bundled != expected {
        return Err("安装包组件版本不一致，请重新安装完整的 Hear".into());
    }
    let mode = match owner {
        Some(ref owner) => Some(stop(owner, true)?),
        None => None,
    };
    wait_stopped(&directory)?;
    let mut command = Command::new(binary);
    command.arg("serve");
    if let Some(mode) = mode {
        command.args(["--handoff", mode]);
    } else {
        command.arg("--on-demand");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("无法启动新版 Hear 后台：{e}"))?;
    // Reap if this GUI remains alive after its Core exits.
    thread::spawn(move || {
        let _ = child.wait();
    });
    let deadline = Instant::now() + WAIT;
    loop {
        match inspect()? {
            Some(Owner::Current { build }) if build == expected => return Ok(()),
            Some(_) => return Err("另一个版本的 Hear 正在使用后台，请退出旧版应用后重试".into()),
            None => {}
        }
        if Instant::now() >= deadline {
            return Err("Hear 后台启动超时，请重试".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn newer(running: &BuildInfo, bundled: &BuildInfo) -> Result<bool, String> {
    fn version(s: &str) -> Result<Vec<u32>, String> {
        s.split('.')
            .map(|v| v.parse().map_err(|_| "无法比较 Hear 版本".into()))
            .collect()
    }
    let a = version(&running.version)?;
    let b = version(&bundled.version)?;
    Ok(a > b || (a == b && running.built_at_epoch > bundled.built_at_epoch))
}

fn stop(owner: &Owner, handoff: bool) -> Result<&'static str, String> {
    let legacy_active = if let Owner::Legacy { version } = owner {
        let status = exchange(*version, json!({"type":"status"})).map_err(|e| e.to_string())?;
        require_type(&status, "status")?;
        status["state"]["microphone_recording"] == true
            || status["state"]["system_recording"] == true
            || status["state"]["capture_state"] == "starting"
    } else {
        false
    };
    let (version, request, expected) = match owner {
        Owner::Current { .. } => (
            PROTOCOL_VERSION,
            json!({"type":"lifecycle","action":if handoff {"handoff"} else {"quit"}}),
            "lifecycle",
        ),
        Owner::Legacy { version } => (*version, json!({"type":"quit"}), "status"),
    };
    let reply =
        exchange(version, request).map_err(|e| format!("Hear 后台尚未退出，请重试：{e}"))?;
    require_type(&reply, expected)?;
    if expected == "lifecycle" {
        let capture = reply["allow_capture"]
            .as_bool()
            .ok_or("后台未返回采集状态")?;
        let normal = reply["normal_run"].as_bool().ok_or("后台未返回运行状态")?;
        return Ok(if capture {
            "capture"
        } else if normal {
            "normal"
        } else {
            "query"
        });
    }
    // Quit clears permission; capture activity must be observed before it.
    Ok(if legacy_active { "capture" } else { "query" })
}

fn require_type(reply: &Value, expected: &str) -> Result<(), String> {
    if reply["type"] == expected {
        Ok(())
    } else {
        Err(format!(
            "Hear 后台返回异常：{}",
            reply["message"].as_str().unwrap_or("无效响应")
        ))
    }
}

fn exchange(version: u32, request: Value) -> std::io::Result<Value> {
    let id = Uuid::new_v4();
    let mut stream = UnixStream::connect(socket_path().map_err(std::io::Error::other)?)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&serde_json::to_vec(
        &json!({"version":version,"id":id,"client":"gui","request":request}),
    )?)?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut bytes = Vec::new();
    stream.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(std::io::Error::other("lifecycle reply too large"));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    if value["id"] != id.to_string() {
        return Err(std::io::Error::other("lifecycle response ID mismatch"));
    }
    Ok(value)
}

fn lock(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        if Instant::now() >= deadline {
            return Err("Hear 后台仍在结束任务，请稍后重试".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_stopped(directory: &Path) -> Result<(), String> {
    // The Core releases this only after capture children have joined and the
    // socket has been removed. Never unlink a live socket or kill a busy owner.
    let _owner = lock(&directory.join("core.lock"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_replace_a_newer_release_or_same_version_newer_build() {
        let base = build_info("core");
        let mut newer_build = base.clone();
        newer_build.built_at_epoch += 1;
        assert!(newer(&newer_build, &base).unwrap());
        newer_build.version = "9.0.0".into();
        newer_build.built_at_epoch = 0;
        assert!(newer(&newer_build, &base).unwrap());
        assert!(!newer(&base, &base).unwrap());
    }
}
