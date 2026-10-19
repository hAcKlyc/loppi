//! Anonymous consumption facts, independent of task publication and content deletion.
//! Money is CNY nanoyuan; historical requests pin an immutable price revision.
use hear_protocol::{CloudProvider, UsageDay, UsageSeries, UsageStatistics};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Deserialize)]
struct Catalog {
    revision: String,
    effective_from_ms: i64,
    calendar_year: String,
    holidays: Vec<[String; 2]>,
    models: Vec<Model>,
}
#[derive(Deserialize)]
struct Model {
    id: String,
    capability: String,
    vendor: String,
    api_model: String,
    name: String,
    #[serde(default)]
    audio_hour_nano: i64,
    #[serde(default)]
    input_million_nano: i64,
    #[serde(default)]
    cached_million_nano: i64,
    #[serde(default)]
    output_million_nano: i64,
}
const CATALOG: &str = include_str!("usage-prices.json");

pub fn init(db: &Connection, now: i64) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS usage_meta(id INTEGER PRIMARY KEY CHECK(id=1), started_ms INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS usage_models(id TEXT PRIMARY KEY, capability TEXT NOT NULL, vendor TEXT NOT NULL, api_model TEXT NOT NULL, name TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS usage_prices(id TEXT PRIMARY KEY, specification TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS usage_requests(
        id TEXT PRIMARY KEY, capability TEXT NOT NULL, model_id TEXT NOT NULL, connection_id TEXT NOT NULL,
        remote_id TEXT, task_id TEXT, purpose TEXT NOT NULL, started_ms INTEGER NOT NULL, observed_ms INTEGER NOT NULL,
        price_id TEXT NOT NULL, band INTEGER, accepted INTEGER NOT NULL DEFAULT 0,
        state TEXT NOT NULL DEFAULT 'unknown', input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER,
        audio_ms INTEGER, response_model TEXT, cost_nano INTEGER,
        FOREIGN KEY(model_id) REFERENCES usage_models(id), FOREIGN KEY(price_id) REFERENCES usage_prices(id));
      CREATE UNIQUE INDEX IF NOT EXISTS usage_remote ON usage_requests(model_id,connection_id,remote_id) WHERE remote_id IS NOT NULL;
      CREATE INDEX IF NOT EXISTS usage_time ON usage_requests(started_ms,capability);")
      .map_err(|e|e.to_string())?;
    db.execute("INSERT OR IGNORE INTO usage_meta VALUES(1,?1)", [now])
        .map_err(|e| e.to_string())?;
    let catalog: Catalog = serde_json::from_str(CATALOG).map_err(|e| e.to_string())?;
    db.execute(
        "INSERT OR IGNORE INTO usage_prices VALUES(?1,?2)",
        params![catalog.revision, CATALOG],
    )
    .map_err(|e| e.to_string())?;
    for model in catalog.models {
        db.execute(
            "INSERT OR IGNORE INTO usage_models VALUES(?1,?2,?3,?4,?5)",
            params![
                model.id,
                model.capability,
                model.vendor,
                model.api_model,
                model.name
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Network adapters report usage before parsing business output. No secret or body is persisted.
pub trait Meter {
    fn begin(
        &self,
        model: &str,
        remote: Option<Uuid>,
        audio_ms: Option<i64>,
    ) -> Result<String, String>;
    fn llm_response(&self, id: &str, response: &Value) -> Result<(), String>;
    fn asr_response(&self, remote: Uuid, status: &str, response: &Value) -> Result<(), String>;
    fn rejected(&self, id: &str) -> Result<(), String>;
    fn observation_failed(&self);
}

pub struct Recorder<'a> {
    shared: &'a crate::CoreState,
    connection: String,
    task_id: Option<Uuid>,
    purpose: &'static str,
}
impl<'a> Recorder<'a> {
    // Caller holds the existing provider outbound gate while resolving identity and calling.
    pub fn task(
        shared: &'a crate::CoreState,
        provider: CloudProvider,
        task_id: Uuid,
    ) -> Result<Self, String> {
        let store = shared.store.lock().map_err(|e| e.to_string())?;
        let connection = crate::events::credential_reference(&store.db, provider)?
            .ok_or("credential_missing")?;
        Ok(Self {
            shared,
            connection,
            task_id: Some(task_id),
            purpose: "processing",
        })
    }
    pub fn verification(shared: &'a crate::CoreState) -> Self {
        Self {
            shared,
            connection: Uuid::new_v4().to_string(),
            task_id: None,
            purpose: "verification",
        }
    }
}
impl Meter for Recorder<'_> {
    fn observation_failed(&self) {
        self.shared.logs.emit(
            hear_protocol::ClientKind::Core,
            Uuid::new_v4(),
            hear_protocol::LogEvent::simple(
                "usage",
                hear_protocol::LogLevel::Warn,
                "observation_failed",
            ),
        );
    }

    fn begin(
        &self,
        model: &str,
        remote: Option<Uuid>,
        audio_ms: Option<i64>,
    ) -> Result<String, String> {
        let store = self.shared.store.lock().map_err(|e| e.to_string())?;
        begin(
            &store.db,
            model,
            &self.connection,
            self.task_id,
            self.purpose,
            remote,
            audio_ms,
            crate::now_ms()?,
        )
    }
    fn llm_response(&self, id: &str, response: &Value) -> Result<(), String> {
        let store = self.shared.store.lock().map_err(|e| e.to_string())?;
        llm_response(&store.db, id, response, crate::now_ms()?)
    }
    fn asr_response(&self, remote: Uuid, status: &str, response: &Value) -> Result<(), String> {
        let store = self.shared.store.lock().map_err(|e| e.to_string())?;
        asr_response(
            &store.db,
            &self.connection,
            remote,
            status,
            response,
            crate::now_ms()?,
        )
    }
    fn rejected(&self, id: &str) -> Result<(), String> {
        let store = self.shared.store.lock().map_err(|e| e.to_string())?;
        store
            .db
            .execute(
                "UPDATE usage_requests SET state='rejected',cost_nano=0 WHERE id=?1 AND accepted=0",
                [id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn band(
    db: &Connection,
    catalog: &Catalog,
    at: i64,
    capability: &str,
) -> Result<Option<i64>, String> {
    if at < catalog.effective_from_ms {
        return Ok(None);
    }
    if capability == "asr" {
        return Ok(Some(1));
    }
    let (date,weekday,hour):(String,i64,i64)=db.query_row(
      "SELECT strftime('%Y-%m-%d',?1/1000,'unixepoch','+8 hours'),CAST(strftime('%w',?1/1000,'unixepoch','+8 hours') AS INTEGER),CAST(strftime('%H',?1/1000,'unixepoch','+8 hours') AS INTEGER)",[at],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    if weekday == 0 || weekday == 6 || !((9..12).contains(&hour) || (14..18).contains(&hour)) {
        return Ok(Some(1));
    }
    // A future holiday calendar must be verified before using weekday peak prices.
    if !date.starts_with(&catalog.calendar_year) {
        return Ok(None);
    }
    let holiday = catalog
        .holidays
        .iter()
        .any(|range| date[5..] >= range[0][..] && date[5..] <= range[1][..]);
    Ok(Some(if holiday { 1 } else { 2 }))
}

fn begin(
    db: &Connection,
    model: &str,
    connection: &str,
    task_id: Option<Uuid>,
    purpose: &str,
    remote: Option<Uuid>,
    audio_ms: Option<i64>,
    now: i64,
) -> Result<String, String> {
    let catalog: Catalog = serde_json::from_str(CATALOG).map_err(|e| e.to_string())?;
    let model = catalog
        .models
        .iter()
        .find(|m| m.api_model == model)
        .ok_or("usage_model_unknown")?;
    let band = band(db, &catalog, now, &model.capability)?;
    let id = Uuid::new_v4().to_string();
    db.execute("INSERT OR IGNORE INTO usage_requests(id,capability,model_id,connection_id,remote_id,task_id,purpose,started_ms,observed_ms,price_id,band,audio_ms)
      VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8,?9,?10,?11)",params![id,model.capability,model.id,connection,remote.map(|r|r.to_string()),task_id.map(|t|t.to_string()),purpose,now,catalog.revision,band,audio_ms]).map_err(|e|e.to_string())?;
    if let Some(remote) = remote {
        db.execute("UPDATE usage_requests SET state='unknown',cost_nano=NULL WHERE model_id=?1 AND connection_id=?2 AND remote_id=?3 AND accepted=0",params![model.id,connection,remote.to_string()]).map_err(|e|e.to_string())?;
        db.query_row(
            "SELECT id FROM usage_requests WHERE model_id=?1 AND connection_id=?2 AND remote_id=?3",
            params![model.id, connection, remote.to_string()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    } else {
        Ok(id)
    }
}

fn token(value: &Value, key: &str) -> Option<i64> {
    value
        .get(key)?
        .as_i64()
        .filter(|v| (0..=100_000_000).contains(v))
}
fn llm_response(db: &Connection, id: &str, response: &Value, now: i64) -> Result<(), String> {
    let u = &response["usage"];
    let input = token(u, "prompt_tokens");
    let output = token(u, "completion_tokens");
    let cached = token(u, "prompt_cache_hit_tokens")
        .or_else(|| token(&u["prompt_tokens_details"], "cached_tokens"));
    let valid = matches!((input,cached),(Some(i),Some(c)) if c<=i);
    let cached = if valid { cached } else { None };
    let response_model = response
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 256);
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE usage_requests SET accepted=1,input_tokens=?2,output_tokens=?3,cached_tokens=?4,response_model=?5,state=?6,observed_ms=?7 WHERE id=?1",
      params![id,input,output,cached,response_model,if input.is_some()&&output.is_some()&&cached.is_some(){"reported"}else{"unknown"},now]).map_err(|e|e.to_string())?;
    price(&tx, id)?;
    tx.commit().map_err(|e| e.to_string())
}
fn asr_response(
    db: &Connection,
    connection: &str,
    remote: Uuid,
    status: &str,
    response: &Value,
    now: i64,
) -> Result<(), String> {
    let id:Option<String>=db.query_row("SELECT id FROM usage_requests WHERE model_id='volcengine:volc.seedasr.auc' AND connection_id=?1 AND remote_id=?2",params![connection,remote.to_string()],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    let Some(id) = id else { return Ok(()) }; // Pre-upgrade jobs have no recoverable consumption history.
    if !matches!(status, "20000000" | "20000001" | "20000002" | "20000003") {
        return Ok(());
    }
    let duration = response
        .pointer("/audio_info/duration")
        .and_then(Value::as_i64)
        .filter(|v| (0..=18_000_000).contains(v));
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE usage_requests SET accepted=1,audio_ms=coalesce(?2,audio_ms),state=CASE WHEN ?2 IS NOT NULL THEN 'reported' WHEN state='reported' THEN state ELSE 'estimated' END,observed_ms=?3 WHERE id=?1",params![id,duration,now]).map_err(|e|e.to_string())?;
    price(&tx, &id)?;
    tx.commit().map_err(|e| e.to_string())
}
fn price(db: &Connection, id: &str) -> Result<(), String> {
    let (spec,model_id,band,input,output,cached,audio):(String,String,Option<i64>,Option<i64>,Option<i64>,Option<i64>,Option<i64>)=db.query_row(
      "SELECT p.specification,u.model_id,u.band,u.input_tokens,u.output_tokens,u.cached_tokens,u.audio_ms FROM usage_requests u JOIN usage_prices p ON p.id=u.price_id WHERE u.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
    let catalog: Catalog = serde_json::from_str(&spec).map_err(|e| e.to_string())?;
    let model = catalog
        .models
        .iter()
        .find(|m| m.id == model_id)
        .ok_or("usage_price_model_missing")?;
    let amount = band.and_then(|b| {
        let (numerator, divisor) = if model.capability == "asr" {
            (audio? as i128 * model.audio_hour_nano as i128, 3_600_000)
        } else {
            (
                ((input? - cached?) as i128 * model.input_million_nano as i128
                    + cached? as i128 * model.cached_million_nano as i128
                    + output? as i128 * model.output_million_nano as i128)
                    * b as i128,
                1_000_000,
            )
        };
        i64::try_from((numerator + divisor / 2) / divisor).ok()
    });
    db.execute(
        "UPDATE usage_requests SET cost_nano=?2 WHERE id=?1",
        params![id, amount],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Content-free diagnostic projection. Unlike the calendar view, accepts an
/// explicit arbitrary window. Unknown observations remain counted as unknown.
pub fn diagnostic_totals(db: &Connection, from: i64, to: i64) -> Result<serde_json::Value, String> {
    let mut totals = serde_json::Map::new();
    for capability in ["llm", "asr"] {
        let value = db.query_row("SELECT count(*),coalesce(sum(accepted),0),sum(input_tokens),sum(output_tokens),sum(audio_ms),sum(cost_nano),coalesce(sum(CASE WHEN state!='rejected' AND (state='unknown' OR cost_nano IS NULL) THEN 1 ELSE 0 END),0) FROM usage_requests WHERE started_ms>=?1 AND started_ms<?2 AND capability=?3",params![from,to,capability], |r| Ok(serde_json::json!({
            "requests":r.get::<_,i64>(0)?,"accepted":r.get::<_,i64>(1)?,"known_input_tokens":r.get::<_,Option<i64>>(2)?,"known_output_tokens":r.get::<_,Option<i64>>(3)?,"known_audio_ms":r.get::<_,Option<i64>>(4)?,"known_cost_nano":r.get::<_,Option<i64>>(5)?,"incomplete":r.get::<_,i64>(6)?
        }))).map_err(|e|e.to_string())?;
        totals.insert(capability.into(), value);
    }
    Ok(serde_json::Value::Object(totals))
}

pub fn statistics(db: &Connection, boundaries: &[i64]) -> Result<UsageStatistics, String> {
    if !matches!(boundaries.len(),8|31|91) || boundaries[0]<0 || boundaries.windows(2).any(|w|!matches!(w[1].checked_sub(w[0]),Some(span) if (20*3_600_000..=28*3_600_000).contains(&span))) {
        return Err("usage_range_invalid".to_owned());
    }
    let since: i64 = db
        .query_row("SELECT started_ms FROM usage_meta WHERE id=1", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let mut days = Vec::with_capacity(boundaries.len() - 1);
    // Index-bounded aggregates, no loading request bodies or the historical task list.
    let mut statement=db.prepare("SELECT capability,COUNT(*),coalesce(SUM(accepted),0),coalesce(SUM(input_tokens),0),coalesce(SUM(output_tokens),0),coalesce(SUM(cached_tokens),0),coalesce(SUM(cost_nano),0),SUM(CASE WHEN state!='rejected' AND (state='unknown' OR cost_nano IS NULL) THEN 1 ELSE 0 END) FROM usage_requests WHERE started_ms>=?1 AND started_ms<?2 GROUP BY capability").map_err(|e|e.to_string())?;
    for window in boundaries.windows(2) {
        let mut day = UsageDay {
            started_ms: window[0],
            covered: window[1] > since,
            partial: window[0] < since && window[1] > since,
            asr: UsageSeries::default(),
            llm: UsageSeries::default(),
        };
        let rows = statement
            .query_map(params![window[0], window[1]], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    UsageSeries {
                        requests: r.get(1)?,
                        count: r.get(2)?,
                        input_tokens: r.get(3)?,
                        output_tokens: r.get(4)?,
                        cached_tokens: r.get(5)?,
                        cost_nano: r.get(6)?,
                        incomplete: r.get(7)?,
                    },
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (kind, series) = row.map_err(|e| e.to_string())?;
            if kind == "asr" {
                day.asr = series;
            } else {
                day.llm = series;
            }
        }
        days.push(day);
    }
    Ok(UsageStatistics {
        started_ms: since,
        currency: "CNY".into(),
        days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn at(db: &Connection, date: &str) -> i64 {
        db.query_row(
            "SELECT CAST(strftime('%s',?1) AS INTEGER)*1000",
            [date],
            |r| r.get(0),
        )
        .unwrap()
    }
    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        init(&db, 1792166400000).unwrap();
        db
    }
    fn bounds(db: &Connection) -> Vec<i64> {
        let start = at(db, "2026-10-16 00:00:00");
        (0..=7).map(|i| start + i * 86_400_000).collect()
    }
    fn amount(db: &Connection, id: &str) -> Option<i64> {
        db.query_row(
            "SELECT cost_nano FROM usage_requests WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }
    fn response() -> Value {
        json!({"model":"deepseek-flash","usage":{"prompt_tokens":1000000,"prompt_cache_hit_tokens":250000,"completion_tokens":100000}})
    }
    #[test]
    fn prices_match_current_resource_and_cache_is_not_double_billed() {
        let db = database();
        let now = at(&db, "2026-10-17 08:00:00");
        let id = begin(
            &db,
            "deepseek-flash",
            "account",
            None,
            "processing",
            None,
            None,
            now,
        )
        .unwrap();
        llm_response(&db, &id, &response(), now).unwrap();
        assert_eq!(amount(&db, &id), Some(1_155_000_000));
        let remote = Uuid::new_v4();
        let id = begin(
            &db,
            "volc.seedasr.auc",
            "account",
            None,
            "processing",
            Some(remote),
            Some(3_600_000),
            now,
        )
        .unwrap();
        asr_response(&db, "account", remote, "20000000", &json!({}), now).unwrap();
        assert_eq!(amount(&db, &id), Some(800_000_000));
    }
    #[test]
    fn official_peak_boundaries_holidays_and_missing_future_calendar() {
        let db = database();
        let catalog: Catalog = serde_json::from_str(CATALOG).unwrap();
        for (time, want) in [
            ("2026-10-19 00:59:59", Some(1)),
            ("2026-10-19 01:00:00", Some(2)),
            ("2026-10-19 04:00:00", Some(1)),
            ("2026-10-19 06:00:00", Some(2)),
            ("2026-10-19 10:00:00", Some(1)),
            ("2026-10-17 01:00:00", Some(1)),
            ("2027-01-04 01:00:00", None),
        ] {
            assert_eq!(
                band(&db, &catalog, at(&db, time), "llm").unwrap(),
                want,
                "{time}"
            );
        }
        let mut earlier = catalog;
        earlier.effective_from_ms = 0;
        assert_eq!(
            band(&db, &earlier, at(&db, "2026-10-01 01:00:00"), "llm").unwrap(),
            Some(1)
        );
        assert_eq!(
            band(&db, &earlier, at(&db, "2026-10-10 01:00:00"), "llm").unwrap(),
            Some(1)
        ); // Weekend makeup work is still off-peak.
    }
    #[test]
    fn asr_polling_restarts_and_retries_do_not_duplicate_remote_consumption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        let db = Connection::open(&path).unwrap();
        init(&db, 1792166400000).unwrap();
        let now = at(&db, "2026-10-18 00:00:00");
        let remote = Uuid::new_v4();
        let id = begin(
            &db,
            "volc.seedasr.auc",
            "account",
            None,
            "processing",
            Some(remote),
            Some(60_000),
            now,
        )
        .unwrap();
        asr_response(&db, "account", remote, "20000000", &json!({}), now).unwrap();
        drop(db);
        let db = Connection::open(&path).unwrap();
        init(&db, now + 86400000).unwrap();
        assert_eq!(
            begin(
                &db,
                "volc.seedasr.auc",
                "account",
                None,
                "processing",
                Some(remote),
                Some(60_000),
                now + 1000
            )
            .unwrap(),
            id
        );
        for _ in 0..10 {
            asr_response(
                &db,
                "account",
                remote,
                "20000000",
                &json!({"audio_info":{"duration":60_000}}),
                now + 2000,
            )
            .unwrap();
        }
        let stats = statistics(&db, &bounds(&db)).unwrap();
        assert_eq!(stats.days.iter().map(|d| d.asr.count).sum::<i64>(), 1);
        let other = begin(
            &db,
            "volc.seedasr.auc",
            "account",
            None,
            "processing",
            Some(Uuid::new_v4()),
            Some(60_000),
            now,
        )
        .unwrap();
        assert_ne!(id, other);
        assert_eq!(stats.started_ms, 1792166400000);
    }
    #[test]
    fn historical_prices_are_immutable_and_currency_has_sub_cent_precision() {
        let db = database();
        let now = at(&db, "2026-10-19 01:00:00");
        let id = begin(
            &db,
            "deepseek-v4-pro",
            "account",
            None,
            "processing",
            None,
            None,
            now,
        )
        .unwrap();
        llm_response(
            &db,
            &id,
            &json!({"usage":{"prompt_tokens":1,"prompt_cache_hit_tokens":0,"completion_tokens":1}}),
            now,
        )
        .unwrap();
        assert_eq!(amount(&db, &id), Some(36_000));
        init(&db, now + 1000).unwrap();
        assert_eq!(amount(&db, &id), Some(36_000));
        db.execute("INSERT INTO usage_prices VALUES('new-price','{}')", [])
            .unwrap();
        price(&db, &id).unwrap();
        assert_eq!(amount(&db, &id), Some(36_000));
    }
    #[test]
    fn missing_and_invalid_usage_are_not_zero_and_pre_collection_days_are_uncovered() {
        let db = database();
        let now = at(&db, "2026-10-18 00:00:00");
        let id = begin(
            &db,
            "deepseek-flash",
            "account",
            None,
            "processing",
            None,
            None,
            now,
        )
        .unwrap();
        llm_response(&db,&id,&json!({"usage":{"prompt_tokens":20,"prompt_cache_hit_tokens":25,"completion_tokens":3}}),now).unwrap();
        assert_eq!(amount(&db, &id), None);
        let stats = statistics(&db, &bounds(&db)).unwrap();
        assert!(stats.days[0].partial);
        let earlier: Vec<i64> = bounds(&db).iter().map(|v| v - 86_400_000).collect();
        assert!(!statistics(&db, &earlier).unwrap().days[0].covered);
        assert_eq!(stats.days[2].llm.incomplete, 1);
        assert_eq!(stats.days[2].llm.input_tokens, 20);
        assert!(statistics(&db, &[i64::MIN; i64::BITS as usize]).is_err());
        assert!(statistics(&db, &[0, 1, 2, 3, 4, 5, 6, i64::MAX]).is_err());
    }
    #[test]
    fn reported_duration_survives_later_empty_poll_and_new_account_is_distinct() {
        let db = database();
        let now = at(&db, "2026-10-18 00:00:00");
        let remote = Uuid::new_v4();
        let id = begin(
            &db,
            "volc.seedasr.auc",
            "a",
            None,
            "verification",
            Some(remote),
            Some(1000),
            now,
        )
        .unwrap();
        asr_response(
            &db,
            "a",
            remote,
            "20000000",
            &json!({"audio_info":{"duration":900}}),
            now,
        )
        .unwrap();
        let cost = amount(&db, &id);
        asr_response(&db, "a", remote, "20000001", &json!({}), now + 1000).unwrap();
        assert_eq!(amount(&db, &id), cost);
        let second = begin(
            &db,
            "volc.seedasr.auc",
            "b",
            None,
            "processing",
            Some(remote),
            Some(1000),
            now,
        )
        .unwrap();
        assert_ne!(id, second);
    }
    #[test]
    fn duration_and_cost_commit_atomically_even_when_repricing_fails() {
        let db = database();
        let now = at(&db, "2026-10-18 00:00:00");
        let remote = Uuid::new_v4();
        let id = begin(
            &db,
            "volc.seedasr.auc",
            "a",
            None,
            "processing",
            Some(remote),
            Some(1000),
            now,
        )
        .unwrap();
        asr_response(&db, "a", remote, "20000000", &json!({}), now).unwrap();
        let prior = amount(&db, &id);
        db.execute_batch("CREATE TRIGGER fail_reprice BEFORE UPDATE OF cost_nano ON usage_requests BEGIN SELECT RAISE(ABORT,'test disk failure'); END;").unwrap();
        assert!(
            asr_response(
                &db,
                "a",
                remote,
                "20000000",
                &json!({"audio_info":{"duration":900}}),
                now + 1
            )
            .is_err()
        );
        let pair: (i64, String) = db
            .query_row(
                "SELECT audio_ms,state FROM usage_requests WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(pair, (1000, "estimated".into()));
        assert_eq!(amount(&db, &id), prior);
        db.execute_batch("DROP TRIGGER fail_reprice").unwrap();
        asr_response(
            &db,
            "a",
            remote,
            "20000000",
            &json!({"audio_info":{"duration":900}}),
            now + 2,
        )
        .unwrap();
        assert_eq!(amount(&db, &id), Some(200000));
    }
    #[test]
    fn usage_survives_business_failure_and_task_removal() {
        let db = database();
        let now = at(&db, "2026-10-18 00:00:00");
        let id = begin(
            &db,
            "deepseek-flash",
            "account",
            Some(Uuid::new_v4()),
            "processing",
            None,
            None,
            now,
        )
        .unwrap();
        // No valid choices/JSON: cloud adapter still records this envelope before business parsing.
        llm_response(&db, &id, &response(), now).unwrap();
        assert!(amount(&db, &id).unwrap() > 0);
        db.execute_batch("CREATE TABLE cloud_tasks(id TEXT); DELETE FROM cloud_tasks;")
            .unwrap();
        assert!(amount(&db, &id).unwrap() > 0);
    }
}
