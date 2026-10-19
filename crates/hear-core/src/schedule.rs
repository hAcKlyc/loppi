//! Daily actions in the existing Core owner, not a continuous capture gate.
use hear_protocol::RecordingSchedule;
use rusqlite::{Connection, Transaction, params};
use std::ffi::{CStr, CString};

#[derive(Debug)]
pub struct Window {
    pub zone: String,
    pub previous_ms: i64,
    pub previous_start: bool,
    pub next_ms: i64,
    pub next_start: bool,
}

pub fn window(now: i64, start: u16, stop: u16, zone: Option<&str>) -> Result<Window, String> {
    unsafe extern "C" {
        fn hear_schedule_window(
            now: i64,
            start: i32,
            stop: i32,
            zone: *const std::ffi::c_char,
            out: *mut std::ffi::c_char,
            capacity: usize,
            previous: *mut i64,
            previous_start: *mut i32,
            next: *mut i64,
            next_start: *mut i32,
        ) -> i32;
    }
    let zone = zone
        .map(CString::new)
        .transpose()
        .map_err(|_| "schedule_clock_unavailable")?;
    let mut name = [0i8; 256];
    let (mut previous_ms, mut next_ms, mut previous_start, mut next_start) = (0, 0, 0, 0);
    let ok = unsafe {
        hear_schedule_window(
            now,
            start.into(),
            stop.into(),
            zone.as_ref().map_or(std::ptr::null(), |z| z.as_ptr()),
            name.as_mut_ptr(),
            name.len(),
            &mut previous_ms,
            &mut previous_start,
            &mut next_ms,
            &mut next_start,
        )
    };
    if ok != 1 {
        return Err("schedule_clock_unavailable".into());
    }
    Ok(Window {
        zone: unsafe { CStr::from_ptr(name.as_ptr()) }
            .to_string_lossy()
            .into_owned(),
        previous_ms,
        previous_start: previous_start != 0,
        next_ms,
        next_start: next_start != 0,
    })
}

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS recording_schedule (
      id INTEGER PRIMARY KEY CHECK(id=1), enabled INTEGER NOT NULL,
      start_minute INTEGER NOT NULL CHECK(start_minute BETWEEN 0 AND 1439),
      stop_minute INTEGER NOT NULL CHECK(stop_minute BETWEEN 0 AND 1439),
      cursor_ms INTEGER NOT NULL, time_zone TEXT NOT NULL,
      paused_by_schedule INTEGER NOT NULL, CHECK(start_minute!=stop_minute));
      INSERT OR IGNORE INTO recording_schedule VALUES(1,0,480,1380,0,'',0);",
    )
    .map_err(|e| e.to_string())
}

fn read(db: &Connection) -> rusqlite::Result<(RecordingSchedule, i64, String)> {
    db.query_row("SELECT enabled,start_minute,stop_minute,paused_by_schedule,cursor_ms,time_zone FROM recording_schedule WHERE id=1", [], |r| {
        Ok((RecordingSchedule { enabled:r.get(0)?,start_minute:r.get(1)?,stop_minute:r.get(2)?,
            paused_by_schedule:r.get(3)?,..Default::default() }, r.get(4)?, r.get(5)?))
    })
}

pub fn view(db: &Connection) -> Result<RecordingSchedule, String> {
    let (mut config, cursor, zone) = read(db).map_err(|e| e.to_string())?;
    if config.enabled {
        let planned = (|| {
            let now = crate::now_ms()?;
            let current = window(now, config.start_minute, config.stop_minute, None)?;
            if current.zone == zone && now < cursor {
                window(cursor, config.start_minute, config.stop_minute, None)
            } else {
                Ok(current)
            }
        })();
        match planned {
            Ok(w) => {
                config.next_at_ms = Some(w.next_ms);
                config.next_action = Some(if w.next_start { "start" } else { "stop" }.into());
            }
            Err(_) => config.error = Some("schedule_clock_unavailable".into()),
        }
    }
    Ok(config)
}

fn calendar_error(_: String) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("schedule_clock_unavailable".into())
}

pub fn save(
    tx: &Transaction<'_>,
    enabled: bool,
    start: u16,
    stop: u16,
    now: i64,
) -> rusqlite::Result<usize> {
    let (prior, _, _) = read(tx)?;
    if prior.enabled == enabled && prior.start_minute == start && prior.stop_minute == stop {
        return Ok(0);
    }
    let w = window(now, start, stop, None).map_err(calendar_error)?;
    tx.execute("UPDATE recording_schedule SET enabled=?1,start_minute=?2,stop_minute=?3,cursor_ms=?4,time_zone=?5 WHERE id=1",
        params![enabled,start,stop,now,w.zone])?;
    tx.execute(
        "UPDATE service_state SET revision=revision+1 WHERE id=1",
        [],
    )
}

/// Caller holds the Store writer. A target already met still consumes the
/// boundary, but never restarts a source or overwrites a later manual action.
pub fn settle(tx: &Transaction<'_>, now: i64) -> rusqlite::Result<bool> {
    settle_in_zone(tx, now, None)
}
fn settle_in_zone(
    tx: &Transaction<'_>,
    now: i64,
    zone_override: Option<&str>,
) -> rusqlite::Result<bool> {
    let (config, cursor, zone) = read(tx)?;
    if !config.enabled {
        return Ok(false);
    }
    let w = window(now, config.start_minute, config.stop_minute, zone_override)
        .map_err(calendar_error)?;
    if zone != w.zone {
        tx.execute(
            "UPDATE recording_schedule SET cursor_ms=?1,time_zone=?2 WHERE id=1",
            params![now, w.zone],
        )?;
        return Ok(false);
    }
    if w.previous_ms <= cursor {
        return Ok(false);
    }
    let (enabled, paused): (bool, bool) = tx.query_row(
        "SELECT enabled_intent,paused_intent FROM service_state WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let changed = if w.previous_start {
        !enabled || paused
    } else {
        !paused
    };
    if changed {
        if w.previous_start {
            crate::close_intent_gaps(tx, "manual_pause", now)?;
            crate::close_intent_gaps(tx, "schedule_pause", now)?;
            tx.execute("UPDATE service_state SET enabled_intent=1,paused_intent=0,revision=revision+1 WHERE id=1",[])?;
            tx.execute(
                "UPDATE recording_schedule SET paused_by_schedule=0 WHERE id=1",
                [],
            )?;
        } else {
            crate::open_intent_gap(tx, "microphone", "schedule_pause", now)?;
            crate::open_intent_gap(tx, "system", "schedule_pause", now)?;
            tx.execute(
                "UPDATE service_state SET paused_intent=1,revision=revision+1 WHERE id=1",
                [],
            )?;
            tx.execute(
                "UPDATE recording_schedule SET paused_by_schedule=1 WHERE id=1",
                [],
            )?;
        }
    }
    tx.execute(
        "UPDATE recording_schedule SET cursor_ms=?1 WHERE id=1",
        [w.previous_ms],
    )?;
    Ok(true)
}

pub fn manual(tx: &Transaction<'_>, now: i64) -> rusqlite::Result<()> {
    settle(tx, now)?;
    crate::close_intent_gaps(tx, "schedule_pause", now)?;
    tx.execute(
        "UPDATE recording_schedule SET paused_by_schedule=0 WHERE id=1",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const BEFORE: i64 = 1_792_533_600_000; // 2026-10-20 22:00 UTC
    const STOP: i64 = BEFORE + 3_600_000;
    const START: i64 = STOP + 9 * 3_600_000;

    fn store() -> crate::Store {
        let s = crate::Store::new(Connection::open_in_memory().unwrap()).unwrap();
        s.db.execute("UPDATE service_state SET enabled_intent=1", [])
            .unwrap();
        s.db.execute(
            "UPDATE recording_schedule SET enabled=1,cursor_ms=?1,time_zone='GMT'",
            [BEFORE],
        )
        .unwrap();
        // Foundation canonicalizes UTC to GMT on this platform. Use its name,
        // not an assumed alias, just as the production persisted record does.
        let zone = window(BEFORE, 480, 1380, Some("UTC")).unwrap().zone;
        s.db.execute("UPDATE recording_schedule SET time_zone=?1", [zone])
            .unwrap();
        s
    }
    fn tick(s: &mut crate::Store, now: i64) {
        let tx = s.db.transaction().unwrap();
        settle_in_zone(&tx, now, Some("UTC")).unwrap();
        tx.commit().unwrap();
    }
    fn intent(s: &crate::Store) -> (bool, bool) {
        s.db.query_row(
            "SELECT enabled_intent,paused_intent FROM service_state",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }
    #[test]
    fn local_calendar_handles_midnight_dst_gaps_folds_and_collisions() {
        let w = window(BEFORE, 480, 1380, Some("UTC")).unwrap();
        assert_eq!((w.next_ms, w.next_start), (STOP, false));
        assert_eq!(window(STOP, 480, 1380, Some("UTC")).unwrap().next_ms, START);
        let spring = window(1_772_949_600_000, 150, 180, Some("America/New_York")).unwrap();
        assert_eq!(
            (spring.next_ms, spring.next_start),
            (1_772_953_200_000, false)
        );
        let collision = window(1_772_953_200_000, 150, 165, Some("America/New_York")).unwrap();
        assert_eq!(
            (collision.previous_ms, collision.previous_start),
            (1_772_953_200_000, false)
        );
        let fold = window(1_793_509_200_000, 90, 180, Some("America/New_York")).unwrap();
        assert_eq!(fold.next_ms, 1_793_511_000_000);
        let after_fold = window(1_793_515_500_000, 90, 180, Some("America/New_York")).unwrap();
        assert_eq!(after_fold.previous_ms, 1_793_511_000_000);
        assert!(window(BEFORE, 0, 0, Some("UTC")).is_err());
        assert!(window(BEFORE, 480, 1380, Some("invalid-zone")).is_err());
    }
    #[test]
    fn recovery_uses_only_latest_and_never_replays_after_manual_changes() {
        let mut s = store();
        tick(&mut s, STOP);
        assert_eq!(intent(&s), (true, true));
        let rev = s.state().unwrap().revision;
        tick(&mut s, STOP + 1);
        assert_eq!(s.state().unwrap().revision, rev);
        // Manual resumption after the boundary survives restart and backward time.
        s.db.execute("UPDATE service_state SET paused_intent=0", [])
            .unwrap();
        let mut s = crate::Store::new(s.db).unwrap();
        tick(&mut s, STOP + 60_000);
        tick(&mut s, BEFORE);
        assert_eq!(intent(&s), (true, false));
        tick(&mut s, START);
        assert_eq!(s.state().unwrap().revision, rev);
        s.db.execute("UPDATE service_state SET paused_intent=1", [])
            .unwrap();
        tick(&mut s, START + 1);
        assert!(intent(&s).1);
        // Ten days missed: one action, no replay chain or intervening stops.
        tick(&mut s, START + 10 * 86_400_000);
        assert!(!intent(&s).1);
        assert_eq!(s.state().unwrap().revision, rev + 1);
    }
    #[test]
    fn rollback_keeps_boundary_pending_and_storage_protection_is_sticky() {
        let mut s = store();
        {
            let tx = s.db.transaction().unwrap();
            settle_in_zone(&tx, STOP, Some("UTC")).unwrap();
            // Simulated failure before commit: neither target nor cursor persists.
        }
        assert!(!intent(&s).1);
        tick(&mut s, STOP);
        assert!(intent(&s).1);
        s.db.execute("UPDATE capture_guard SET pause_reason='storage_limit'", [])
            .unwrap();
        tick(&mut s, START);
        assert!(!intent(&s).1);
        assert!(s.state().unwrap().storage_paused);
    }
    #[test]
    fn timezone_change_rebases_without_capture_action() {
        let mut s = store();
        let tx = s.db.transaction().unwrap();
        settle_in_zone(&tx, STOP, Some("Asia/Shanghai")).unwrap();
        tx.commit().unwrap();
        assert_eq!(intent(&s), (true, false));
        assert_eq!(read(&s.db).unwrap().1, STOP);
    }
    #[test]
    fn saving_starts_in_future_and_duplicate_save_does_not_rebase() {
        let mut s = store();
        let tx = s.db.transaction().unwrap();
        save(&tx, true, 481, 1381, STOP).unwrap();
        tx.commit().unwrap();
        assert_eq!(intent(&s), (true, false));
        let cursor = read(&s.db).unwrap().1;
        let tx = s.db.transaction().unwrap();
        assert_eq!(save(&tx, true, 481, 1381, START).unwrap(), 0);
        tx.commit().unwrap();
        assert_eq!(read(&s.db).unwrap().1, cursor);
        let tx = s.db.transaction().unwrap();
        save(&tx, false, 481, 1381, START).unwrap();
        tx.commit().unwrap();
        tick(&mut s, START + 10 * 86_400_000);
        assert_eq!(intent(&s), (true, false));
        let tx = s.db.transaction().unwrap();
        save(&tx, true, 481, 1381, START + 10 * 86_400_000).unwrap();
        tx.commit().unwrap();
        assert_eq!(read(&s.db).unwrap().1, START + 10 * 86_400_000);
    }
    #[test]
    fn schedule_gap_survives_capture_teardown_and_closes_on_start() {
        let mut s = store();
        tick(&mut s, STOP);
        s.close_open_gaps(crate::AudioSource::Microphone).unwrap();
        s.close_open_gaps(crate::AudioSource::System).unwrap();
        let open = |s: &crate::Store| -> i64 {
            s.db.query_row(
            "SELECT count(*) FROM media_gaps WHERE reason='schedule_pause' AND ended_ms IS NULL", [], |r| r.get(0)).unwrap()
        };
        assert_eq!(open(&s), 2);
        tick(&mut s, START);
        assert_eq!(open(&s), 0);
        assert!(!s.state().unwrap().recording_schedule.paused_by_schedule);
    }
    #[test]
    fn manual_mutation_consumes_due_action_before_user_intent() {
        let mut s = store();
        let now = crate::now_ms().unwrap();
        let zone = window(now, 480, 1380, None).unwrap().zone;
        s.db.execute(
            "UPDATE recording_schedule SET time_zone=?1,cursor_ms=0",
            [zone],
        )
        .unwrap();
        let revision = s.state().unwrap().revision;
        let id = uuid::Uuid::new_v4();
        let result = s
            .handle(
                id,
                hear_protocol::Request::Pause {
                    expected_revision: revision,
                },
            )
            .unwrap();
        assert!(matches!(result, hear_protocol::Response::Status { .. }));
        assert!(intent(&s).1);
        let cursor = read(&s.db).unwrap().1;
        assert!(cursor > 0);
        s.settle_schedule().unwrap();
        assert!(intent(&s).1);
        // A receipt replay never re-applies a manual or scheduled action.
        let rev = s.state().unwrap().revision;
        s.handle(
            id,
            hear_protocol::Request::Pause {
                expected_revision: revision,
            },
        )
        .unwrap();
        assert_eq!(s.state().unwrap().revision, rev);
    }
}
