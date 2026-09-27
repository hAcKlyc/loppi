//! Supplier adapters. Network calls run outside the Store lock. No request or
//! response body, credential, or transcript is written to diagnostic logs.
use base64::Engine;
use hear_protocol::CloudProvider;
use security_framework::os::macos::keychain::SecKeychain;
use security_framework::passwords::{
    PasswordOptions, delete_generic_password, generic_password, set_generic_password,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use uuid::Uuid;

const SERVICE: &str = "app.hear.cloud";
const DEEPSEEK_URL: &str = "https://api.deepseek.com/chat/completions";
const VOLC_SUBMIT: &str = "https://openspeech.bytedance.com/api/v3/auc/bigmodel/submit";
const VOLC_QUERY: &str = "https://openspeech.bytedance.com/api/v3/auc/bigmodel/query";
const VOLC_RESOURCE: &str = "volc.seedasr.auc";

fn account(provider: CloudProvider) -> &'static str {
    match provider {
        CloudProvider::Llm => "deepseek",
        CloudProvider::Asr => "volcengine",
    }
}

static KEYCHAIN_ACCESS: OnceLock<Mutex<()>> = OnceLock::new();

fn keychain_access() -> &'static Mutex<()> {
    KEYCHAIN_ACCESS.get_or_init(|| Mutex::new(()))
}

fn valid_reference(provider: CloudProvider, reference: &str) -> bool {
    reference
        .split_once(':')
        .is_some_and(|(name, id)| name == account(provider) && Uuid::parse_str(id).is_ok())
}

pub fn credential(provider: CloudProvider, reference: &str) -> Result<Option<String>, String> {
    if !valid_reference(provider, reference) {
        return Err("cloud credential reference is invalid".to_owned());
    }
    // The login-item worker must never wait behind an invisible Keychain
    // authorization dialog while the screen is locked. The file-based macOS
    // Keychain remains available to this Developer ID helper without a
    // provisioning profile; interaction is disabled only for the read.
    let _guard = keychain_access()
        .lock()
        .map_err(|_| "keychain_read_lock".to_owned())?;
    let _no_ui = SecKeychain::disable_user_interaction()
        .map_err(|error| format!("keychain_read_{}", error.code()))?;
    match generic_password(PasswordOptions::new_generic_password(SERVICE, reference)) {
        Ok(secret) => String::from_utf8(secret)
            .map(Some)
            .map_err(|_| "cloud credential is not UTF-8".to_owned()),
        Err(error) if error.code() == -25300 => Ok(None), // errSecItemNotFound
        Err(error) => Err(format!("keychain_read_{}", error.code())),
    }
}

pub fn save_credential(provider: CloudProvider, secret: &str) -> Result<String, String> {
    if secret.len() < 8 || secret.len() > 4096 || secret.chars().any(char::is_control) {
        return Err("invalid cloud credential format".to_owned());
    }
    let _guard = keychain_access()
        .lock()
        .map_err(|_| "keychain_save_lock".to_owned())?;
    let reference = format!("{}:{}", account(provider), Uuid::new_v4());
    set_generic_password(SERVICE, &reference, secret.as_bytes())
        .map_err(|error| format!("keychain_save_{}", error.code()))?;
    Ok(reference)
}

pub fn delete_credential(reference: &str) -> Result<(), String> {
    let _guard = keychain_access()
        .lock()
        .map_err(|_| "keychain_delete_lock".to_owned())?;
    delete_generic_password(SERVICE, reference)
        .map_err(|error| format!("keychain_delete_{}", error.code()))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(45))
        .timeout_write(Duration::from_secs(30))
        .build()
}

fn json_response(response: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => return Err(format!("provider_http_{code}")),
        Err(_) => return Err("provider_transport".to_owned()),
    };
    parse_body(response)
}

fn parse_body(response: ureq::Response) -> Result<Value, String> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(8_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| "provider_read_failed".to_owned())?;
    if bytes.len() > 8_000_000 {
        return Err("provider_response_too_large".to_owned());
    }
    serde_json::from_slice(&bytes).map_err(|_| "provider_invalid_json".to_owned())
}

pub fn verify_llm(key: &str, model: &str) -> Result<(), String> {
    let result = chat(
        key,
        model,
        "Return only JSON with {\"ok\":true}. This is a connectivity check; no user data is included.",
        "Return JSON with ok true.",
    )?;
    if result.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err("provider_verification_failed".to_owned())
    }
}

pub fn chat(key: &str, model: &str, system: &str, user: &str) -> Result<Value, String> {
    let body = json!({
        "model": model,
        "thinking": {"type":"disabled"},
        "response_format": {"type":"json_object"},
        "messages": [
            {"role":"system","content":system},
            {"role":"user","content":user}
        ]
    });
    let response = json_response(
        agent()
            .post(DEEPSEEK_URL)
            .set("Authorization", &format!("Bearer {key}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string()),
    )?;
    let choice = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|array| array.first())
        .ok_or_else(|| "provider_missing_choice".to_owned())?;
    if choice.get("finish_reason").and_then(Value::as_str) != Some("stop") {
        return Err("provider_incomplete_output".to_owned());
    }
    let content = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| "provider_missing_content".to_owned())?;
    serde_json::from_str(content).map_err(|_| "provider_invalid_json_content".to_owned())
}

fn volc_request(
    url: &str,
    key: &str,
    request_id: Uuid,
    body: Value,
) -> Result<(String, String, Value), String> {
    let response = agent()
        .post(url)
        .set("X-Api-Key", key)
        .set("X-Api-Resource-Id", VOLC_RESOURCE)
        .set("X-Api-Request-Id", &request_id.to_string())
        .set("X-Api-Sequence", "-1")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => return Err(format!("provider_http_{code}")),
        Err(_) => return Err("provider_transport".to_owned()),
    };
    let status = response
        .header("X-Api-Status-Code")
        .unwrap_or("")
        .to_owned();
    let message = response
        .header("X-Api-Message")
        .unwrap_or("")
        .to_ascii_lowercase();
    let value = parse_body(response)?;
    Ok((status, message, value))
}

pub fn submit_audio(key: &str, request_id: Uuid, wav: &[u8]) -> Result<(), String> {
    if wav.len() > 12_000_000 || wav.len() < 44 {
        return Err("asr_audio_size".to_owned());
    }
    let body = json!({
        "user": {"uid":"hear-local"},
        "audio": {"data":base64::engine::general_purpose::STANDARD.encode(wav),"format":"wav"},
        "request": {"model_name":"bigmodel","enable_speaker_info":true,"show_utterances":true}
    });
    let (status, _, _) = volc_request(VOLC_SUBMIT, key, request_id, body)?;
    if status == "20000000" {
        Ok(())
    } else {
        Err(format!("provider_status_{status}"))
    }
}

#[derive(Debug, Clone)]
pub enum AsrQuery {
    Missing,
    Processing,
    Complete(Vec<AsrUtterance>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrUtterance {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub speaker: Option<String>,
}

pub fn query_audio(key: &str, request_id: Uuid) -> Result<AsrQuery, String> {
    let (status, message, response) = volc_request(VOLC_QUERY, key, request_id, json!({}))?;
    match status.as_str() {
        "20000001" | "20000002" => Ok(AsrQuery::Processing),
        "20000003" => Ok(AsrQuery::Complete(Vec::new())),
        "45000000" if message.contains("cannot find task") => Ok(AsrQuery::Missing),
        "20000000" => {
            let array = response
                .pointer("/result/utterances")
                .and_then(Value::as_array)
                .ok_or_else(|| "provider_missing_utterances".to_owned())?;
            let mut utterances = Vec::with_capacity(array.len());
            for item in array {
                let start_ms = item
                    .get("start_time")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "provider_invalid_timing".to_owned())?;
                let end_ms = item
                    .get("end_time")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "provider_invalid_timing".to_owned())?;
                let text = item
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "provider_invalid_utterance".to_owned())?;
                if start_ms < 0 || end_ms <= start_ms || text.len() > 20_000 {
                    return Err("provider_invalid_utterance".to_owned());
                }
                let speaker = item.pointer("/additions/speaker").and_then(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                });
                utterances.push(AsrUtterance {
                    start_ms,
                    end_ms,
                    text: text.to_owned(),
                    speaker,
                });
            }
            Ok(AsrQuery::Complete(utterances))
        }
        _ => Err(format!("provider_status_{status}")),
    }
}

pub fn verify_asr(key: &str) -> Result<(), String> {
    let mut wav = Vec::with_capacity(44 + 32_000);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(32_036u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    for index in 0..16_000 {
        let phase = index as f32 * 440.0 * std::f32::consts::TAU / 16_000.0;
        wav.extend_from_slice(&((phase.sin() * 3_000.0) as i16).to_le_bytes());
    }
    let request_id = Uuid::new_v4();
    submit_audio(key, request_id, &wav)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match query_audio(key, request_id)? {
            AsrQuery::Complete(_) => return Ok(()),
            AsrQuery::Missing => return Err("provider_verification_task_missing".to_owned()),
            AsrQuery::Processing if Instant::now() < deadline => {
                thread::sleep(Duration::from_secs(1))
            }
            AsrQuery::Processing => return Err("provider_verification_timeout".to_owned()),
        }
    }
}
