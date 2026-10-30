//! Supplier adapters. Network calls run outside the Store lock. No request or
//! response body, credential, or transcript is written to diagnostic logs.
use crate::usage::Meter;
use base64::Engine;
use hear_protocol::CloudProvider;
use security_framework::os::macos::keychain::SecKeychain;
use security_framework::passwords::{
    PasswordOptions, delete_generic_password, generic_password, set_generic_password,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
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
fn offline_test() -> bool {
    std::env::var_os("HEAR_TEST_PARENT_PID").is_some()
}

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
    if offline_test() {
        return Err("test_offline".into());
    }
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

/// Check the actual signed Core's Keychain access without starting services,
/// prompting, changing credentials, or calling a cloud provider. Never return secrets/references.
pub fn check_credentials(database: &std::path::Path) -> Result<Value, String> {
    let db =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    let mut providers = Vec::new();
    for provider in [CloudProvider::Llm, CloudProvider::Asr] {
        let (state, error) = match crate::events::credential_reference(&db, provider)? {
            None => ("not_configured", None),
            Some(reference) => match credential(provider, &reference) {
                Ok(Some(secret)) if !secret.is_empty() => ("readable", None),
                Ok(_) => ("unavailable", Some("credential_missing".to_owned())),
                Err(error) => ("unavailable", Some(error)),
            },
        };
        providers.push(json!({"provider":provider,"state":state,"error":error}));
    }
    Ok(json!({"ok":providers.iter().all(|p|p["state"]=="readable"),
        "build":hear_protocol::build_info("core"),"providers":providers,
        "network_requests":false,"user_interaction":false}))
}

pub fn save_credential(provider: CloudProvider, secret: &str) -> Result<String, String> {
    if offline_test() {
        return Err("test_offline".into());
    }
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
    if offline_test() {
        return Err("test_offline".into());
    }
    let _guard = keychain_access()
        .lock()
        .map_err(|_| "keychain_delete_lock".to_owned())?;
    delete_generic_password(SERVICE, reference)
        .map_err(|error| format!("keychain_delete_{}", error.code()))
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(45))
            .timeout_write(Duration::from_secs(30))
            .build()
    })
}

/// Internal scheduling metadata, never a supplier body or credential.
#[derive(Debug)]
pub struct Failure {
    pub code: String,
    pub retry_after_ms: Option<i64>,
    pub context: hear_protocol::LogContext,
}
impl From<String> for Failure {
    fn from(code: String) -> Self {
        Self {
            code,
            retry_after_ms: None,
            context: Default::default(),
        }
    }
}
impl From<Failure> for String {
    fn from(error: Failure) -> Self {
        error.code
    }
}

pub fn remaining(deadline: Instant) -> Result<Duration, Failure> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| "provider_deadline".to_owned().into())
}

fn retry_after(value: &str, now: std::time::SystemTime) -> Option<i64> {
    let duration = match value.trim().parse::<u64>() {
        Ok(seconds) => Duration::from_secs(seconds),
        Err(_) => httpdate::parse_http_date(value)
            .ok()?
            .duration_since(now)
            .unwrap_or_default(),
    };
    // Saturate arithmetic, preserving the supplier's lower bound even across restart.
    Some(duration.as_millis().min(i64::MAX as u128) as i64)
}

fn checked_response(
    response: Result<ureq::Response, ureq::Error>,
) -> Result<ureq::Response, Failure> {
    match response {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(code, response)) => Err(Failure {
            code: format!("provider_http_{code}"),
            context: Default::default(),
            retry_after_ms: response
                .header("Retry-After")
                .and_then(|v| retry_after(v, std::time::SystemTime::now())),
        }),
        Err(_) => Err("provider_transport".to_owned().into()),
    }
}

#[cfg(test)]
fn json_response(response: Result<ureq::Response, ureq::Error>) -> Result<Value, Failure> {
    let response = checked_response(response).map_err(|mut e| {
        e.context.cause = Some("response_headers".into());
        e
    })?;
    parse_body(response).map_err(|code| {
        let mut failure = Failure::from(code);
        failure.context.cause = Some("response_body".into());
        failure
    })
}

// Streaming lets thinking responses make observable progress without discarding
// a paid request at the old 60s boundary. The request still has an absolute
// deadline, an idle-read timeout, and a bounded total response. Reasoning is
// consumed but never retained, published or logged.
fn streamed_chat(response: Result<ureq::Response, ureq::Error>) -> Result<Value, Failure> {
    let response = checked_response(response).map_err(|mut e| {
        e.context.cause = Some("response_headers".into());
        e
    })?;
    let mut reader = BufReader::new(response.into_reader().take(8_000_001));
    let mut bytes = 0;
    let mut line = String::new();
    let mut data = String::new();
    let mut content = String::new();
    let mut finish = Value::Null;
    let mut usage = Value::Null;
    loop {
        line.clear();
        let count = reader.read_line(&mut line).map_err(|e| {
            let mut failure = Failure::from(
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) {
                    "provider_read_timeout".to_owned()
                } else {
                    "provider_read_failed".to_owned()
                },
            );
            failure.context.cause = Some("response_body".into());
            failure
        })?;
        bytes += count;
        if bytes > 8_000_000 {
            return Err("provider_response_too_large".to_owned().into());
        }
        if count == 0 {
            return Err("provider_incomplete_output".to_owned().into());
        }
        if line.trim().is_empty() {
            if data.is_empty() {
                continue;
            }
            if data.trim() == "[DONE]" {
                if finish.is_null() {
                    return Err("provider_incomplete_output".to_owned().into());
                }
                return Ok(
                    json!({"choices":[{"message":{"content":content},"finish_reason":finish}],"usage":usage}),
                );
            }
            let chunk: Value =
                serde_json::from_str(&data).map_err(|_| "provider_invalid_json".to_owned())?;
            if let Some(delta) = chunk
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                content.push_str(delta);
            }
            if let Some(reason) = chunk
                .pointer("/choices/0/finish_reason")
                .filter(|v| !v.is_null())
            {
                finish = reason.clone();
            }
            if let Some(value) = chunk.get("usage").filter(|v| !v.is_null()) {
                usage = value.clone();
            }
            data.clear();
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start().trim_end_matches(['\r', '\n']));
        }
    }
}

fn parse_body(response: ureq::Response) -> Result<Value, String> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(8_000_001)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            match error.kind() {
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                    "provider_read_timeout"
                }
                _ => "provider_read_failed",
            }
            .to_owned()
        })?;
    if bytes.len() > 8_000_000 {
        return Err("provider_response_too_large".to_owned());
    }
    serde_json::from_slice(&bytes).map_err(|_| "provider_invalid_json".to_owned())
}

const VERIFY_SYSTEM: &str = r#"This is a connectivity check; no user data is included.

Output requirements:
Return one JSON object with the required field ok set to the boolean true (not the string "true"). Do not include other fields, Markdown code fences, or explanatory text.
Example output:
{"ok":true}
"#;

pub fn verify_llm(key: &str, model: &str, meter: &dyn Meter) -> Result<(), String> {
    let result = chat(
        key,
        model,
        VERIFY_SYSTEM,
        "Return JSON with ok true.",
        Instant::now() + Duration::from_secs(60),
        meter,
    )?;
    if result.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err("provider_verification_failed".to_owned())
    }
}

pub fn chat(
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    deadline: Instant,
    meter: &dyn Meter,
) -> Result<Value, Failure> {
    chat_content(
        key,
        model,
        system,
        json!(user),
        deadline,
        meter,
        true,
        DEEPSEEK_URL,
    )
}

pub fn describe_image(
    key: &str,
    model: &str,
    png_base64: &str,
    deadline: Instant,
    meter: &dyn Meter,
) -> Result<Value, Failure> {
    chat_content(
        key,
        model,
        crate::moments::IMAGE_SYSTEM,
        image_content(png_base64),
        deadline,
        meter,
        false,
        DEEPSEEK_URL,
    )
}

fn image_content(png_base64: &str) -> Value {
    json!([
        {"type":"text","text":"请输出这张截图的摘要和完整 Markdown 文字转录。"},
        {"type":"image_url","image_url":{"url":format!("data:image/png;base64,{png_base64}"),"detail":"original"}}
    ])
}

fn chat_content(
    key: &str,
    model: &str,
    system: &str,
    user: Value,
    deadline: Instant,
    meter: &dyn Meter,
    thinking: bool,
    endpoint: &str,
) -> Result<Value, Failure> {
    let mut body = json!({
        "model": model,
        "thinking": {"type": if thinking { "enabled" } else { "disabled" }},
        "response_format": {"type":"json_object"},
        "stream": true,
        "stream_options": {"include_usage":true},
        "messages": [
            {"role":"system","content":system},
            {"role":"user","content":user}
        ]
    });
    // Window fitting must reconcile old facts with corrections in one call.
    // Low effort enables that reasoning without changing screenshot requests.
    if thinking {
        body["reasoning_effort"] = json!("low");
        // Explicit paid replay may compare reasoning effort without changing
        // production requests or reading a credential outside Core.
        #[cfg(test)]
        if std::env::var("HEAR_REAL_UNDERSTANDING_ALLOW").as_deref() == Ok("1") {
            if let Ok(effort) = std::env::var("HEAR_REAL_UNDERSTANDING_EFFORT") {
                assert!(matches!(effort.as_str(), "low" | "high"));
                body["reasoning_effort"] = json!(effort);
            }
        }
    }
    let timeout = remaining(deadline)?;
    let usage_id = meter.begin(model, None, None)?;
    if offline_test() && endpoint == DEEPSEEK_URL {
        return Err("test_offline".to_owned().into());
    }
    let headers_started = Instant::now();
    let response = agent()
        .post(endpoint)
        .timeout(timeout)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    let headers_ms = headers_started.elapsed().as_millis() as u64;
    let body_started = Instant::now();
    let response = metered_result(
        meter,
        &usage_id,
        streamed_chat(response).map_err(|mut error| {
            error.context.headers_ms = Some(headers_ms);
            error.context.body_ms = Some(body_started.elapsed().as_millis() as u64);
            error
        }),
    )?;
    parse_metered_chat_content(meter, &usage_id, &response)
}

fn observe_usage(meter: &dyn Meter, result: Result<(), String>) {
    // The response has already been paid for. A statistics failure cannot
    // replace business output or make the task require another paid attempt.
    if result.is_err() {
        meter.observation_failed();
    }
}

fn parse_metered_chat_content(
    meter: &dyn Meter,
    id: &str,
    response: &Value,
) -> Result<Value, Failure> {
    observe_usage(meter, meter.llm_response(id, response));
    parse_chat_content(response)
}

pub(crate) fn parse_chat_content(response: &Value) -> Result<Value, Failure> {
    let choice = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|array| array.first())
        .ok_or_else(|| "provider_missing_choice".to_owned())?;
    if choice.get("finish_reason").and_then(Value::as_str) != Some("stop") {
        use hear_protocol::OutputFinishReason;
        let finish_reason = match choice.get("finish_reason").and_then(Value::as_str) {
            Some("length") => OutputFinishReason::Length,
            Some("content_filter") => OutputFinishReason::ContentFilter,
            Some("tool_calls") => OutputFinishReason::ToolCalls,
            Some(_) => OutputFinishReason::Other,
            None => OutputFinishReason::Missing,
        };
        return Err(Failure {
            code: "provider_incomplete_output".into(),
            retry_after_ms: None,
            context: hear_protocol::LogContext {
                finish_reason: Some(finish_reason),
                ..Default::default()
            },
        });
    }
    let content = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| "provider_missing_content".to_owned())?;
    serde_json::from_str::<UniqueJson>(content)
        .map(|value| value.0)
        .map_err(|error| {
            // Parser categories carry no response text, paths or credentials.
            let code = match error.classify() {
                serde_json::error::Category::Eof => "provider_invalid_json_content_eof",
                serde_json::error::Category::Syntax => "provider_invalid_json_content_syntax",
                serde_json::error::Category::Data => "provider_invalid_json_content_data",
                serde_json::error::Category::Io => "provider_invalid_json_content_io",
            };
            Failure {
                code: code.into(),
                retry_after_ms: None,
                context: hear_protocol::LogContext {
                    finish_reason: Some(hear_protocol::OutputFinishReason::Stop),
                    json_line: Some(error.line()),
                    json_column: Some(error.column()),
                    output_bytes: Some(content.len()),
                    ..Default::default()
                },
            }
        })
}

// Inspect object members before Value's map can overwrite a duplicate. This
// applies recursively, including same-value duplicates and escaped key names.
struct UniqueJson(Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueJson(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueJson>()? {
                    values.push(value.0);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate object key"));
                    }
                    values.insert(key, map.next_value::<UniqueJson>()?.0);
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn volc_request(
    url: &str,
    key: &str,
    request_id: Uuid,
    body: Value,
    deadline: Instant,
) -> Result<(String, String, Value), Failure> {
    if offline_test() {
        return Err("test_offline".to_owned().into());
    }
    let response = agent()
        .post(url)
        .timeout(remaining(deadline)?)
        .set("X-Api-Key", key)
        .set("X-Api-Resource-Id", VOLC_RESOURCE)
        .set("X-Api-Request-Id", &request_id.to_string())
        .set("X-Api-Sequence", "-1")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    let response = checked_response(response)?;
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

pub enum AsrAudioFormat {
    Wav,
    Ogg,
}

pub fn submit_audio(
    key: &str,
    request_id: Uuid,
    audio: &[u8],
    format: AsrAudioFormat,
    vocabulary: &Value,
    duration_ms: i64,
    deadline: Instant,
    meter: &dyn Meter,
) -> Result<(), Failure> {
    let (name, maximum, minimum) = match format {
        AsrAudioFormat::Wav => ("wav", 12_000_000, 44),
        AsrAudioFormat::Ogg => ("ogg", 10_000_000, 64),
    };
    if audio.len() > maximum || audio.len() < minimum {
        return Err("asr_audio_size".to_owned().into());
    }
    let body = audio_body(audio, name, vocabulary);
    remaining(deadline)?;
    let usage_id = meter.begin(VOLC_RESOURCE, Some(request_id), Some(duration_ms))?;
    let (status, _, response) = metered_result(
        meter,
        &usage_id,
        volc_request(VOLC_SUBMIT, key, request_id, body, deadline),
    )?;
    observe_usage(meter, meter.asr_response(request_id, &status, &response));
    if status == "20000000" {
        Ok(())
    } else {
        Err(format!("provider_status_{status}").into())
    }
}

fn audio_body(audio: &[u8], name: &str, vocabulary: &Value) -> Value {
    let mut body = json!({
        "user": {"uid":"hear-local"},
        "audio": {"data":base64::engine::general_purpose::STANDARD.encode(audio),"format":name},
        "request": {"model_name":"bigmodel","enable_speaker_info":true,"show_utterances":true,"ssd_version":"300"}
    });
    if vocabulary["hotwords"]
        .as_array()
        .is_some_and(|words| !words.is_empty())
    {
        body["request"]["corpus"] = json!({"context":vocabulary.to_string()});
    }
    body
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Value>,
}

pub fn query_audio(
    key: &str,
    request_id: Uuid,
    deadline: Instant,
    meter: &dyn Meter,
) -> Result<AsrQuery, Failure> {
    let (status, message, response) =
        volc_request(VOLC_QUERY, key, request_id, json!({}), deadline)?;
    parse_metered_asr_query(meter, request_id, &status, &message, &response)
}

fn parse_metered_asr_query(
    meter: &dyn Meter,
    request_id: Uuid,
    status: &str,
    message: &str,
    response: &Value,
) -> Result<AsrQuery, Failure> {
    observe_usage(meter, meter.asr_response(request_id, status, response));
    parse_asr_query(status, message, response)
}

fn parse_asr_query(status: &str, message: &str, response: &Value) -> Result<AsrQuery, Failure> {
    match status {
        "20000001" | "20000002" => Ok(AsrQuery::Processing),
        "20000003" => Ok(AsrQuery::Complete(Vec::new())),
        // Observed query response: "The task was not found for this application.
        // Check the application and task ID." Keep submission/recovery decisions
        // with the durable task owner, including prior submit attempts.
        "45000092" => Ok(AsrQuery::Missing),
        "45000000" if message.contains("cannot find task") => Ok(AsrQuery::Missing),
        "20000000" => parse_asr_result(response)
            .map(AsrQuery::Complete)
            .map_err(Into::into),
        _ => Err(format!("provider_status_{status}").into()),
    }
}

pub(crate) fn parse_asr_result(response: &Value) -> Result<Vec<AsrUtterance>, String> {
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
            .ok_or_else(|| "provider_invalid_utterance_text".to_owned())?;
        if start_ms < 0 || end_ms <= start_ms {
            return Err("provider_invalid_utterance_range".to_owned());
        }
        if text.len() > 20_000 {
            return Err("provider_invalid_utterance_length".to_owned());
        }
        let speaker = item.pointer("/additions/speaker").and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        });
        let utterance = AsrUtterance {
            start_ms,
            end_ms,
            text: text.to_owned(),
            speaker,
            words: item.get("words").cloned(),
        };
        // Some successful seedasr responses join speech separated by
        // minutes of silence. Use complete, aligned word metadata to
        // split those pauses before the core checks audio coverage.
        let mut pieces = split_long_asr_pauses(&utterance, item).unwrap_or_else(|| vec![utterance]);
        for piece in &mut pieces {
            if let Some(words) = item.get("words").and_then(Value::as_array) {
                piece.words = Some(Value::Array(
                    words
                        .iter()
                        .filter(|w| {
                            w["start_time"]
                                .as_i64()
                                .is_some_and(|a| a >= piece.start_ms)
                                && w["end_time"].as_i64().is_some_and(|b| b <= piece.end_ms)
                        })
                        .cloned()
                        .collect(),
                ));
            }
        }
        utterances.extend(pieces);
    }
    Ok(utterances)
}

fn split_long_asr_pauses(utterance: &AsrUtterance, item: &Value) -> Option<Vec<AsrUtterance>> {
    let words = item.get("words")?.as_array()?;
    let mut timed = Vec::new();
    let mut cursor = 0;
    for word in words {
        let text = word.get("text")?.as_str()?;
        if text.is_empty() {
            return None;
        }
        let offset = utterance.text[cursor..].find(text)? + cursor;
        // Preserve the original wording, punctuation and spaces. Never use
        // an incomplete word list to invent timing for unaligned speech.
        if utterance.text[cursor..offset]
            .chars()
            .any(char::is_alphanumeric)
        {
            return None;
        }
        cursor = offset + text.len();
        let start = word.get("start_time")?.as_i64()?;
        let end = word.get("end_time")?.as_i64()?;
        if start == -1 && end == -1 && !text.chars().any(char::is_alphanumeric) {
            continue;
        }
        if start < utterance.start_ms || end > utterance.end_ms || end <= start {
            return None;
        }
        if timed
            .last()
            .is_some_and(|&(previous_start, _, _)| start < previous_start)
        {
            return None;
        }
        timed.push((start, end, offset));
    }
    if utterance.text[cursor..].chars().any(char::is_alphanumeric) {
        return None;
    }
    let &(_, mut end, _) = timed.first()?;
    // Preserve the supplier envelope so publication still sees any original
    // out-of-task bound or unsupported leading/trailing audio interval.
    let mut start = utterance.start_ms;
    let mut text_start = 0;
    let mut split = Vec::new();
    for &(next_start, next_end, offset) in &timed[1..] {
        if next_start - end > crate::events::MAX_ASR_INTERIOR_SILENCE_MS {
            split.push(AsrUtterance {
                words: None,
                start_ms: start,
                end_ms: end,
                text: utterance.text[text_start..offset].to_owned(),
                speaker: utterance.speaker.clone(),
            });
            start = next_start;
            text_start = offset;
        }
        end = end.max(next_end);
    }
    if split.is_empty() {
        return None;
    }
    split.push(AsrUtterance {
        words: None,
        start_ms: start,
        end_ms: utterance.end_ms,
        text: utterance.text[text_start..].to_owned(),
        speaker: utterance.speaker.clone(),
    });
    Some(split)
}

pub fn verify_asr(key: &str, meter: &dyn Meter) -> Result<(), String> {
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
    let deadline = Instant::now() + Duration::from_secs(60);
    submit_audio(
        key,
        request_id,
        &wav,
        AsrAudioFormat::Wav,
        &json!({}),
        1000,
        deadline,
        meter,
    )?;
    loop {
        match query_audio(key, request_id, deadline, meter)? {
            AsrQuery::Complete(_) => return Ok(()),
            AsrQuery::Missing => return Err("provider_verification_task_missing".to_owned()),
            AsrQuery::Processing if Instant::now() < deadline => {
                thread::sleep(Duration::from_secs(1))
            }
            AsrQuery::Processing => return Err("provider_verification_timeout".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn credential_check_is_read_only_and_does_not_expose_references() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hear.sqlite");
        assert!(check_credentials(&path).is_err());
        assert!(!path.exists(), "check must not create an empty database");
        let store = crate::Store::new(rusqlite::Connection::open(&path).unwrap()).unwrap();
        let db = &store.db;
        let empty = check_credentials(&path).unwrap();
        assert_eq!(empty["ok"], false);
        assert_eq!(empty["providers"][0]["state"], "not_configured");
        let invalid_reference = "private-reference-must-not-appear";
        crate::events::set_configured_with_credential(
            db,
            CloudProvider::Llm,
            "deepseek-flash",
            invalid_reference,
        )
        .unwrap();
        let result = check_credentials(&path).unwrap();
        assert_eq!(result["ok"], false);
        assert_eq!(result["providers"][0]["state"], "unavailable");
        assert_eq!(result["network_requests"], false);
        assert_eq!(result["user_interaction"], false);
        assert!(!result.to_string().contains(invalid_reference));
        assert_eq!(
            crate::events::credential_reference(db, CloudProvider::Llm).unwrap(),
            Some(invalid_reference.to_owned())
        );
    }
    #[test]
    fn asr_hotwords_are_a_json_string_and_empty_config_omits_corpus() {
        let context = json!({"hotwords":[{"word":"看山"},{"word":"盗梦空间"}]});
        let body = audio_body(&[], "wav", &context);
        assert_eq!(
            serde_json::from_str::<Value>(body["request"]["corpus"]["context"].as_str().unwrap())
                .unwrap(),
            context
        );
        assert!(
            audio_body(&[], "wav", &json!({}))["request"]
                .get("corpus")
                .is_none()
        );
    }
    #[test]
    fn duplicate_keys_are_rejected_before_value_overwrite() {
        for content in [
            r#"{"events":[],"events":[]}"#,
            r#"{"events":[{"summary":"keep","summary":null}]}"#,
            r#"{"x":[{"deep":{"secret":1,"\u0073ecret":2}}]}"#,
        ] {
            let response =
                json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]});
            let error = parse_chat_content(&response).unwrap_err();
            assert_eq!(error.code, "provider_invalid_json_content_data");
            assert!(!format!("{error:?}").contains("secret"));
        }
        let content = r#"{"a":[true,false,null,-3,18446744073709551615,1.5,"中文"],"b":{"a":0}}"#;
        let response = json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]});
        assert_eq!(
            parse_chat_content(&response).unwrap(),
            serde_json::from_str::<Value>(content).unwrap()
        );
    }
    #[test]
    fn malformed_output_preserves_only_numeric_parser_context() {
        let response = serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":"{\n  \"private words\": "}}]});
        let error = parse_chat_content(&response).unwrap_err();
        assert_eq!(error.context.json_line, Some(2));
        assert!(error.context.json_column.unwrap() > 0);
        assert!(error.context.output_bytes.unwrap() > 0);
        assert!(
            !serde_json::to_string(&error.context)
                .unwrap()
                .contains("private")
        );
    }

    #[test]
    fn invalid_json_reports_category_without_supplier_content() {
        for (content, code) in [
            ("{\"events\":[", "provider_invalid_json_content_eof"),
            ("secret plaintext", "provider_invalid_json_content_syntax"),
        ] {
            let response =
                json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]});
            assert_eq!(parse_chat_content(&response).unwrap_err().code, code);
        }
    }
    #[test]
    fn prompt_output_example_passes_connectivity_response_parser() {
        let example = VERIFY_SYSTEM
            .lines()
            .find(|line| line.starts_with("{\"ok\":"))
            .unwrap();
        let response = json!({"choices":[{"finish_reason":"stop","message":{"content":example}}]});
        assert_eq!(
            parse_chat_content(&response)
                .unwrap()
                .get("ok")
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn screenshot_http_response_publishes_both_outputs_and_records_usage() {
        use crate::{events, moments};
        use std::cell::Cell;
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut size = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        size = value.trim().parse::<usize>().unwrap();
                    }
                }
            }
            let mut bytes = vec![0; size];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                request["messages"][1]["content"][1]["image_url"]["detail"],
                "original"
            );
            assert!(
                request["messages"][1]["content"][1]["image_url"]["url"]
                    .as_str()
                    .unwrap()
                    .starts_with("data:image/png;base64,")
            );
            assert_eq!(request["response_format"]["type"], "json_object");
            assert_eq!(request["thinking"]["type"], "disabled");
            assert!(request.get("reasoning_effort").is_none());
            assert_eq!(request["stream"], true);
            let response = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"finish_reason":"stop","delta":{"content":json!({"summary":"测试图摘要","transcript":"# Hello\n\n完整转录"}).to_string()}}]}),
                json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":10}})
            );
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
        });
        struct TestMeter {
            begun: Cell<bool>,
            observed: Cell<bool>,
        }
        impl Meter for TestMeter {
            fn begin(
                &self,
                model: &str,
                _: Option<Uuid>,
                _: Option<i64>,
            ) -> Result<String, String> {
                assert_eq!(model, "deepseek-flash");
                self.begun.set(true);
                Ok("test".into())
            }
            fn llm_response(&self, _: &str, response: &Value) -> Result<(), String> {
                assert_eq!(response["usage"]["prompt_tokens"], 20);
                self.observed.set(true);
                Ok(())
            }
            fn asr_response(&self, _: Uuid, _: &str, _: &Value) -> Result<(), String> {
                unreachable!()
            }
            fn rejected(&self, _: &str) -> Result<(), String> {
                panic!("unexpected rejection")
            }
            fn observation_failed(&self) {
                panic!("unexpected usage failure")
            }
        }
        let meter = TestMeter {
            begun: Cell::new(false),
            observed: Cell::new(false),
        };
        let directory = tempfile::tempdir().unwrap();
        let mut store = crate::Store::new(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        events::set_configured_with_credential(
            &store.db,
            CloudProvider::Llm,
            "deepseek-flash",
            "deepseek:mock",
        )
        .unwrap();
        let id = Uuid::new_v4();
        let relative = std::path::PathBuf::from(format!("screenshots/{id}.png"));
        std::fs::create_dir(directory.path().join("screenshots")).unwrap();
        std::fs::write(
            directory.path().join(&relative),
            [137, 80, 78, 71, 13, 10, 26, 10],
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        moments::save_capture(
            &tx,
            &moments::CaptureResult {
                id,
                relative,
                triggered_ms: 1000,
                captured_ms: 1001,
                ocr_text: None,
                ocr_state: "failed",
            },
            1002,
        )
        .unwrap();
        tx.commit().unwrap();
        let task = events::take_task(&mut store.db, CloudProvider::Llm, 2000)
            .unwrap()
            .unwrap();
        let png = moments::image(&store.db, directory.path(), id).unwrap();
        let output = chat_content(
            "fake-test-key",
            &task.model_id,
            moments::IMAGE_SYSTEM,
            image_content(&png),
            Instant::now() + Duration::from_secs(5),
            &meter,
            false,
            &endpoint,
        )
        .unwrap();
        moments::publish_image(&mut store.db, &task, output, 3000).unwrap();
        let row = moments::list(&store.db, 0, 2000, 0, 20).unwrap().remove(0);
        assert_eq!(row.image_summary.as_deref(), Some("测试图摘要"));
        assert_eq!(row.image_transcript.as_deref(), Some("# Hello\n\n完整转录"));
        assert_eq!(row.image_state.as_deref(), Some("complete"));
        assert!(meter.begun.get() && meter.observed.get());
        server.join().unwrap();
    }

    #[test]
    fn screenshot_request_contains_actual_image_and_original_detail() {
        let input = image_content("iVBORw0KGgo=");
        assert_eq!(input[1]["type"], "image_url");
        assert_eq!(
            input[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(input[1]["image_url"]["detail"], "original");
    }

    #[test]
    fn metering_failure_preserves_paid_output_and_original_rejection() {
        use super::*;
        use std::cell::Cell;
        struct FailingMeter(Cell<usize>);
        impl Meter for FailingMeter {
            fn begin(&self, _: &str, _: Option<Uuid>, _: Option<i64>) -> Result<String, String> {
                Err("storage_unavailable".into())
            }
            fn llm_response(&self, _: &str, _: &Value) -> Result<(), String> {
                Err("storage_unavailable".into())
            }
            fn asr_response(&self, _: Uuid, _: &str, _: &Value) -> Result<(), String> {
                Err("storage_unavailable".into())
            }
            fn rejected(&self, _: &str) -> Result<(), String> {
                Err("storage_unavailable".into())
            }
            fn observation_failed(&self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let meter = FailingMeter(Cell::new(0));
        let response =
            json!({"choices":[{"finish_reason":"stop","message":{"content":"{\"ok\":true}"}}]});
        assert_eq!(
            parse_metered_chat_content(&meter, "id", &response).unwrap(),
            json!({"ok":true})
        );
        assert!(matches!(
            parse_metered_asr_query(&meter, Uuid::new_v4(), "20000003", "", &json!({})).unwrap(),
            AsrQuery::Complete(_)
        ));
        let failure = Failure {
            code: "provider_http_429".into(),
            retry_after_ms: Some(2000),
            context: Default::default(),
        };
        let rejected = metered_result::<()>(&meter, "id", Err(failure)).unwrap_err();
        assert_eq!(rejected.code, "provider_http_429");
        assert_eq!(rejected.retry_after_ms, Some(2000));
        assert_eq!(meter.0.get(), 3);
        // Preflight failure still stops before any network call.
        assert_eq!(
            chat(
                "unused",
                "deepseek-flash",
                "",
                "",
                Instant::now() + Duration::from_secs(1),
                &meter
            )
            .unwrap_err()
            .code,
            "storage_unavailable"
        );
    }

    #[test]
    fn asr_query_recognizes_current_and_legacy_missing_task_responses() {
        for (status, message) in [
            (
                "45000092",
                "The task was not found for this application. Check the application and task ID.",
            ),
            ("45000000", "cannot find task"),
        ] {
            assert!(matches!(
                parse_asr_query(status, message, &json!({})),
                Ok(AsrQuery::Missing)
            ));
        }
        // A generic argument error must not trigger an audio submission.
        assert_eq!(
            parse_asr_query("45000000", "invalid resource", &json!({}))
                .unwrap_err()
                .code,
            "provider_status_45000000"
        );
        assert!(matches!(
            parse_asr_query("20000001", "", &json!({})),
            Ok(AsrQuery::Processing)
        ));
        assert!(
            matches!(parse_asr_query("20000003", "", &json!({})), Ok(AsrQuery::Complete(items)) if items.is_empty())
        );
    }
    use super::*;

    #[test]
    fn retry_headers_preserve_seconds_dates_and_deadlines() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(retry_after("120", now), Some(120_000));
        assert_eq!(
            retry_after(&httpdate::fmt_http_date(now + Duration::from_secs(90)), now),
            Some(90_000)
        );
        assert_eq!(
            retry_after(&httpdate::fmt_http_date(now - Duration::from_secs(90)), now),
            Some(0)
        );
        assert_eq!(retry_after("not a delay", SystemTime::now()), None);
        let response = ureq::Response::new(429, "Too Many Requests", "private").unwrap();
        assert_eq!(
            checked_response(Err(ureq::Error::Status(429, response)))
                .err()
                .unwrap()
                .code,
            "provider_http_429"
        );
        assert_eq!(
            remaining(Instant::now() - Duration::from_millis(1))
                .err()
                .unwrap()
                .code,
            "provider_deadline"
        );
    }

    #[test]
    fn streaming_discards_reasoning_requires_completion_and_preserves_usage() {
        let content = json!({"ok":true}).to_string();
        let stream = format!(
            ": keep-alive\n\ndata: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"reasoning_content":"private reasoning"}}]}),
            json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":10}})
        );
        let response = streamed_chat(Ok(ureq::Response::new(200, "OK", &stream).unwrap())).unwrap();
        assert_eq!(parse_chat_content(&response).unwrap(), json!({"ok":true}));
        assert_eq!(response["usage"]["prompt_tokens"], 20);
        assert!(!response.to_string().contains("private reasoning"));
        let truncated = stream.replace("data: [DONE]\n\n", "");
        assert_eq!(
            streamed_chat(Ok(ureq::Response::new(200, "OK", &truncated).unwrap()))
                .unwrap_err()
                .code,
            "provider_incomplete_output"
        );
        assert_eq!(
            streamed_chat(Ok(
                ureq::Response::new(200, "OK", "data: [DONE]\n\n").unwrap()
            ))
            .unwrap_err()
            .code,
            "provider_incomplete_output"
        );
    }

    #[test]
    fn streamed_requests_bound_slow_headers_body_disconnect_and_no_response() {
        use std::io::Write;
        use std::net::TcpListener;
        for mode in ["headers", "body", "disconnect", "silent"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                if mode == "headers" || mode == "silent" {
                    std::thread::sleep(Duration::from_millis(300));
                    if mode == "headers" {
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                    }
                    return;
                }
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n").unwrap();
                let _ =
                    stream.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"{\"}}]}\n\n");
                if mode == "body" {
                    // Bytes continue arriving inside the idle timeout. The
                    // total request budget must still end this stream.
                    for _ in 0..100 {
                        std::thread::sleep(Duration::from_millis(10));
                        if stream.write_all(b": keep-alive\n\n").is_err() {
                            break;
                        }
                    }
                }
            });
            let began = Instant::now();
            let error = streamed_chat(
                agent()
                    .post(&format!("http://{address}"))
                    .timeout(Duration::from_millis(100))
                    .send_string("{}"),
            )
            .unwrap_err();
            assert!(
                matches!(
                    error.code.as_str(),
                    "provider_read_timeout"
                        | "provider_transport"
                        | "provider_incomplete_output"
                        | "provider_read_failed"
                ),
                "{mode}: {}",
                error.code
            );
            assert!(began.elapsed() < Duration::from_secs(2), "{mode}");
            if mode != "disconnect" {
                assert!(began.elapsed() >= Duration::from_millis(80), "{mode}");
            }
            server.join().unwrap();
        }
    }

    #[test]
    fn slow_response_body_obeys_request_budget_and_retry_metadata_survives_http_error() {
        use std::io::Write;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nRetry-After: 123\r\nConnection: close\r\n\r\n").unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n{",
                )
                .unwrap();
            for _ in 0..100 {
                std::thread::sleep(Duration::from_millis(10));
                if stream.write_all(b" ").is_err() {
                    break;
                }
            }
        });
        let url = format!("http://{address}");
        let error = json_response(agent().get(&url).call()).unwrap_err();
        assert_eq!(error.code, "provider_http_429");
        assert_eq!(error.retry_after_ms, Some(123_000));
        let began = Instant::now();
        let error = json_response(agent().get(&url).timeout(Duration::from_millis(100)).call())
            .unwrap_err();
        assert!(matches!(
            error.code.as_str(),
            "provider_read_timeout" | "provider_transport"
        ));
        assert!(began.elapsed() < Duration::from_secs(2));
        server.join().unwrap();
    }

    fn word(start: i64, end: i64, text: &str) -> Value {
        json!({"start_time":start,"end_time":end,"text":text})
    }
    fn result(start: i64, end: i64, text: &str, words: Value) -> Value {
        json!({"result":{"utterances":[{"start_time":start,"end_time":end,
            "text":text,"words":words,"additions":{"speaker":1}}]}})
    }

    #[test]
    fn long_pauses_split_without_losing_punctuation_words_or_speaker() {
        // Real failing timing topology, synthetic transcript only.
        let response = result(
            26_180,
            370_755,
            "开始，继续。后来，结束！",
            json!([
                word(26_180, 26_780, "开始"),
                word(27_540, 29_795, "继续"),
                word(97_720, 99_160, "后来"),
                word(370_635, 370_755, "结束")
            ]),
        );
        let items = parse_asr_result(&response).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|u| (u.start_ms, u.end_ms))
                .collect::<Vec<_>>(),
            [(26_180, 29_795), (97_720, 99_160), (370_635, 370_755)]
        );
        assert_eq!(
            items.iter().map(|u| u.text.as_str()).collect::<String>(),
            "开始，继续。后来，结束！"
        );
        assert!(items.iter().all(|u| u.speaker.as_deref() == Some("1")));
    }

    #[test]
    fn short_pauses_and_optional_words_preserve_the_provider_sentence() {
        for words in [
            Value::Null,
            json!([]),
            json!([word(200, 500, "hello"), word(3500, 3800, "world")]),
        ] {
            let items = parse_asr_result(&result(100, 4000, "hello world.", words)).unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!((items[0].start_ms, items[0].end_ms), (100, 4000));
            assert_eq!(items[0].text, "hello world.");
        }
    }

    #[test]
    fn untimed_punctuation_keeps_original_spacing_when_splitting() {
        let items = parse_asr_result(&result(
            0,
            10_300,
            "Hello, world!",
            json!([
                word(0, 300, "Hello"),
                word(-1, -1, ","),
                word(-1, -1, " "),
                word(10_000, 10_300, "world"),
                word(-1, -1, "!")
            ]),
        ))
        .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "Hello, ");
        assert_eq!(items[1].text, "world!");
    }

    #[test]
    fn incomplete_or_invalid_word_metadata_never_guesses_a_split() {
        for words in [
            json!([word(0, 300, "开始"), word(10_000, 10_300, "后来")]),
            json!([
                word(0, 300, "开始"),
                word(10_000, 10_000, "遗漏"),
                word(10_000, 10_300, "后来")
            ]),
            json!([
                word(0, 300, "开始"),
                word(-1, -1, "遗漏"),
                word(10_000, 10_300, "后来")
            ]),
            json!([
                word(0, 300, "开始"),
                word(10_000, 10_300, "遗漏"),
                word(9000, 10_300, "后来")
            ]),
        ] {
            let items = parse_asr_result(&result(0, 10_300, "开始遗漏后来", words)).unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].text, "开始遗漏后来");
            assert_eq!((items[0].start_ms, items[0].end_ms), (0, 10_300));
        }
    }

    #[test]
    fn repeated_response_with_zero_duration_tail_is_rejected_atomically() {
        // Synthetic structure of a real 5-second provider response. Dropping
        // only its zero-duration tail would publish repeated hallucinations.
        let mut utterances = Vec::new();
        for i in 0..42 {
            utterances.push(
                json!({"start_time":i*120,"end_time":((i+1)*120).min(5000),"text":"重复内容"}),
            );
        }
        for _ in 0..12 {
            utterances.push(json!({"start_time":5000,"end_time":5000,"text":"重复内容"}));
        }
        assert_eq!(
            parse_asr_result(&json!({"result":{"utterances":utterances}})).unwrap_err(),
            "provider_invalid_utterance_range"
        );
    }

    #[test]
    fn malformed_utterances_still_fail_the_whole_response() {
        for (start, end) in [(-1, 300), (300, 300), (400, 300)] {
            assert_eq!(
                parse_asr_result(&result(start, end, "无效", Value::Null)).unwrap_err(),
                "provider_invalid_utterance_range"
            );
        }
    }
}

// Only explicit admission failures establish zero usage; transport/read failures stay unknown.
fn metered_result<T>(
    meter: &dyn Meter,
    id: &str,
    result: Result<T, Failure>,
) -> Result<T, Failure> {
    if let Err(error) = &result {
        if matches!(
            error.code.as_str(),
            "provider_http_400"
                | "provider_http_401"
                | "provider_http_403"
                | "provider_http_404"
                | "provider_http_422"
                | "provider_http_429"
        ) {
            observe_usage(meter, meter.rejected(id));
        }
    }
    result
}
