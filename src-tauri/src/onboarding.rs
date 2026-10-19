//! GUI routing metadata only. Business readiness remains in Core and macOS.
use hear_protocol::{ClientKind, ModelPhase, Request, Response, SourcePhase};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::Manager;

pub struct Preferences(Mutex<PathBuf>);
impl Preferences {
    pub fn new(path: PathBuf) -> Self {
        Self(Mutex::new(path))
    }
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    schema_version: u32,
    pub completed: bool,
}
fn read(path: &Path) -> Result<Option<Metadata>, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let value: Metadata = serde_json::from_slice(&bytes)
                .map_err(|_| "首次设置记录损坏，请点击恢复设置记录")?;
            if value.schema_version != 1 {
                return Err("首次设置记录版本不支持，请使用匹配的应用版本".into());
            }
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法读取首次设置记录，请检查应用配置目录权限后重试".into()),
    }
}
fn write(path: &Path, completed: bool) -> Result<Metadata, String> {
    let parent = path.parent().ok_or("配置目录不可用")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".onboarding-{}.tmp", uuid::Uuid::new_v4()));
    let value = Metadata {
        schema_version: 1,
        completed,
    };
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(&serde_json::to_vec(&value).map_err(|e| e.to_string())?)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(temp);
        return Err(error);
    }
    Ok(value)
}
fn business() -> Result<(hear_protocol::ServiceState, bool), String> {
    let Response::Status { state, .. } =
        hear_protocol::send_from(ClientKind::Gui, Request::Status)?
    else {
        return Err("无法读取记录状态".into());
    };
    let Response::CloudStatus { queues, .. } =
        hear_protocol::send_from(ClientKind::Gui, Request::CloudStatus)?
    else {
        return Err("无法读取云端配置".into());
    };
    let configured = [
        hear_protocol::CloudProvider::Llm,
        hear_protocol::CloudProvider::Asr,
    ]
    .iter()
    .all(|p| queues.iter().any(|q| q.provider == *p && q.configured));
    Ok((state, configured))
}
#[tauri::command]
pub async fn onboarding_status(app: tauri::AppHandle, recover: bool) -> Result<Metadata, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let prefs = app.state::<Preferences>();
        let path = prefs.0.lock().map_err(|e| e.to_string())?;
        if !recover {
            if let Some(value) = read(&path)? {
                return Ok(value);
            }
        }
        // Missing history is not a first-run signal. Existing intent or both
        // credentials preserve the legacy user's normal route, including pause.
        let (state, configured) = business()?;
        write(&path, state.enabled_intent || configured)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn complete_onboarding(app: tauri::AppHandle) -> Result<Metadata, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let prefs = app.state::<Preferences>();
        let path = prefs.0.lock().map_err(|e| e.to_string())?;
        // Reconcile a lost write acknowledgement without repeating anything.
        if let Some(value) = read(&path)? {
            if value.completed {
                return Ok(value);
            }
        }
        let (state, configured) = business()?;
        let Response::Models { state: models, .. } =
            hear_protocol::send_from(ClientKind::Gui, Request::Models)?
        else {
            return Err("无法读取本地组件".into());
        };
        let privacy = super::privacy_status();
        if !recording_space_available(
            state.audio_used_bytes,
            state.audio_limit_bytes,
            state.disk_free_bytes,
        ) {
            return Err("录音空间不足，请释放磁盘空间或到设置中调整音频上限".into());
        }
        if !configured || !matches!(models.phase, ModelPhase::Ready) {
            return Err("请先准备本地语音组件和两项云服务".into());
        }
        if !state.microphone_enabled && !state.system_enabled {
            return Err("请至少选择一种声音来源".into());
        }
        if state.microphone_enabled
            && (privacy.microphone != 3
                || !privacy.microphone_available
                || matches!(state.microphone_phase, SourcePhase::Unavailable))
        {
            return Err("请完成麦克风授权，或取消选择麦克风".into());
        }
        if state.system_enabled
            && (!(if privacy.tap_required {
                privacy.tap_ready
            } else {
                privacy.screen_and_system_audio == 1
            }) || matches!(state.system_phase, SourcePhase::Unavailable))
        {
            return Err("请完成电脑声音授权，或取消选择电脑声音".into());
        }
        // Never change capture intent, pause, login or schedules here.
        write(&path, true)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn open_privacy_settings(source: String) -> Result<(), String> {
    let pane = match source.as_str() {
        "microphone" => "Privacy_Microphone",
        "system" => "Privacy_ScreenCapture",
        _ => return Err("未知声音来源".into()),
    };
    let status = std::process::Command::new("/usr/bin/open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{pane}"
        ))
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("请打开系统设置 → 隐私与安全性，允许 Hear 访问麦克风或屏幕与系统音频录制".into())
    }
}
fn recording_space_available(used: u64, limit: u64, free: u64) -> bool {
    used < limit && free > 1_073_741_824
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_requires_existing_recording_space_admission() {
        assert!(!recording_space_available(0, 100, 1_073_741_824));
        assert!(!recording_space_available(100, 100, 2_147_483_648));
        assert!(recording_space_available(99, 100, 1_073_741_825));
    }
    #[test]
    fn metadata_is_private_atomic_and_corruption_is_not_first_run() {
        let dir =
            std::env::temp_dir().join(format!("hear-onboarding-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("onboarding.json");
        assert!(read(&path).unwrap().is_none());
        write(&path, false).unwrap();
        assert!(!read(&path).unwrap().unwrap().completed);
        write(&path, true).unwrap();
        assert!(read(&path).unwrap().unwrap().completed);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(&path, b"bad").unwrap();
        assert!(read(&path).is_err());
        fs::write(&path, br#"{"schema_version":2,"completed":true}"#).unwrap();
        assert!(read(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[tauri::command]
pub fn open_setup_help(provider: String) -> Result<(), String> {
    let url = match provider.as_str() {
        "llm" => "https://api-docs.deepseek.com/api/deepseek-api/",
        "asr" => {
            "https://docs.volcengine.com/docs/DoubaoVoice/LargemodelrecordingfilerecognitionstandardversionAPI?lang=zh"
        }
        _ => return Err("未知服务".into()),
    };
    let status = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("无法打开官方帮助，请在浏览器访问对应服务商文档".into())
    }
}
