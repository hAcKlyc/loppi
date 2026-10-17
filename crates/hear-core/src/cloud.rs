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
}
impl From<String> for Failure {
    fn from(code: String) -> Self {
        Self {
            code,
            retry_after_ms: None,
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
            retry_after_ms: response
                .header("Retry-After")
                .and_then(|v| retry_after(v, std::time::SystemTime::now())),
        }),
        Err(_) => Err("provider_transport".to_owned().into()),
    }
}

fn json_response(response: Result<ureq::Response, ureq::Error>) -> Result<Value, Failure> {
    parse_body(checked_response(response)?).map_err(Into::into)
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

pub fn verify_llm(key: &str, model: &str, meter: &dyn Meter) -> Result<(), String> {
    let result = chat(
        key,
        model,
        "Return only JSON with {\"ok\":true}. This is a connectivity check; no user data is included.",
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
    let body = json!({
        "model": model,
        "thinking": {"type":"disabled"},
        "response_format": {"type":"json_object"},
        "messages": [
            {"role":"system","content":system},
            {"role":"user","content":user}
        ]
    });
    let timeout = remaining(deadline)?;
    let usage_id = meter.begin(model, None, None)?;
    let response = metered_result(
        meter,
        &usage_id,
        json_response(
            agent()
                .post(DEEPSEEK_URL)
                .timeout(timeout)
                .set("Authorization", &format!("Bearer {key}"))
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
        ),
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

fn parse_chat_content(response: &Value) -> Result<Value, Failure> {
    let choice = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|array| array.first())
        .ok_or_else(|| "provider_missing_choice".to_owned())?;
    if choice.get("finish_reason").and_then(Value::as_str) != Some("stop") {
        return Err("provider_incomplete_output".to_owned().into());
    }
    let content = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| "provider_missing_content".to_owned())?;
    serde_json::from_str(content).map_err(|_| "provider_invalid_json_content".to_owned().into())
}

fn volc_request(
    url: &str,
    key: &str,
    request_id: Uuid,
    body: Value,
    deadline: Instant,
) -> Result<(String, String, Value), Failure> {
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
    let body = json!({
        "user": {"uid":"hear-local"},
        "audio": {"data":base64::engine::general_purpose::STANDARD.encode(audio),"format":name},
        "request": {"model_name":"bigmodel","enable_speaker_info":true,"show_utterances":true,"ssd_version":"300"}
    });
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
            .ok_or_else(|| "provider_invalid_utterance".to_owned())?;
        if start_ms < 0 || end_ms <= start_ms || text.len() > 20_000 {
            return Err("provider_invalid_utterance".to_owned());
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
        };
        // Some successful seedasr responses join speech separated by
        // minutes of silence. Use complete, aligned word metadata to
        // split those pauses before the core checks audio coverage.
        utterances
            .extend(split_long_asr_pauses(&utterance, item).unwrap_or_else(|| vec![utterance]));
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
            "provider_read_failed" | "provider_transport"
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
    fn malformed_utterances_still_fail_the_whole_response() {
        for (start, end) in [(-1, 300), (300, 300), (400, 300)] {
            assert_eq!(
                parse_asr_result(&result(start, end, "无效", Value::Null)).unwrap_err(),
                "provider_invalid_utterance"
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
