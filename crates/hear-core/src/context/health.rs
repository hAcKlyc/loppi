//! Coverage aggregates survive detail pagination. Durations are unions, not row sums.
use super::*;
use std::collections::BTreeMap;
fn duration(mut spans: Vec<(i64, i64)>) -> i64 {
    spans.sort_unstable();
    let mut end = i64::MIN;
    let mut total = 0;
    for (a, b) in spans {
        total += (b - a.max(end)).max(0);
        end = end.max(b);
    }
    total
}
fn gap_spans(
    db: &Connection,
    r: &TimeRange,
    m: i64,
) -> Result<BTreeMap<(String, String), Vec<(i64, i64)>>> {
    let mut out = BTreeMap::<_, Vec<_>>::new();
    let mut stmt = sql(db.prepare("SELECT source,reason,max(started_ms,?1),min(coalesce(ended_ms,?4),?2) FROM media_gaps WHERE started_ms<?2 AND coalesce(ended_ms,?4)>?1 AND (source='microphone' AND ?3&1!=0 OR source='system' AND ?3&2!=0)"))?;
    for row in sql(
        stmt.query_map(params![r.from_ms, r.to_ms, m, api::now_ms()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        }),
    )? {
        let (source, reason, a, b) = sql(row)?;
        out.entry((source, reason)).or_default().push((a, b));
    }
    Ok(out)
}
pub(super) fn gaps(db: &Connection, r: &TimeRange, m: i64) -> Result<Value> {
    Ok(json!(gap_spans(db,r,m)?.into_iter().map(|((source,reason),spans)|json!({"source":source,"reason":reason,"duration_ms":duration(spans)})).collect::<Vec<_>>()))
}
pub(super) fn recent(db: &Connection) -> Result<Value> {
    let end = api::now_ms();
    let r = TimeRange {
        from_ms: end - 7 * 86_400_000,
        to_ms: end,
        timezone: api::timezone(),
    };
    let gaps = gap_spans(db, &r, 3)?;
    let mut sources = Vec::new();
    for source in ["microphone", "system"] {
        let mut stmt = sql(db.prepare("SELECT max(started_ms,?1),min(ended_ms,?2) FROM audio_blocks WHERE source=?3 AND started_ms<?2 AND ended_ms>?1"))?;
        let spans = sql(
            sql(stmt.query_map(params![r.from_ms, r.to_ms, source], |r| {
                Ok((r.get(0)?, r.get(1)?))
            }))?
            .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        let recorded = duration(spans.clone());
        let mut observed = spans;
        let mut missing = Vec::new();
        let mut reasons = Vec::new();
        for ((s, reason), spans) in &gaps {
            if s == source {
                observed.extend(spans.clone());
                missing.extend(spans.clone());
                reasons.push(json!({"reason":reason,"duration_ms":duration(spans.clone())}));
            }
        }
        let total = duration(observed);
        sources.push(json!({"source":source,"recorded_ms":recorded,"gap_ms":duration(missing),"observed_ms":total,"success_rate":if total>0 {Some(recorded as f64/total as f64)} else {None},"gaps":reasons}));
    }
    Ok(
        json!({"from":api::timestamp(r.from_ms),"to":api::timestamp(r.to_ms),"sources":sources,"rate_basis":"recorded duration / union of recorded and gap intervals; unknown time excluded; not a transcription accuracy score"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_spans_are_not_double_counted() {
        assert_eq!(duration(vec![(0, 20), (10, 30), (40, 50), (0, 5)]), 40);
    }
}
