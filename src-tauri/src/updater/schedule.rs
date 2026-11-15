use std::time::{Duration, Instant, SystemTime};

const STARTUP_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);
pub(super) const QUICK_RETRY: Duration = Duration::from_secs(10 * 60);
pub(super) const DOWNLOAD_RETRY: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone, Copy)]
pub(super) struct Time {
    wall: SystemTime,
    monotonic: Instant,
}

impl Time {
    pub(super) fn now() -> Self {
        Self {
            wall: SystemTime::now(),
            monotonic: Instant::now(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Deadline {
    start: Time,
    delay: Duration,
}

impl Deadline {
    pub(super) fn after(start: Time, delay: Duration) -> Self {
        Self { start, delay }
    }

    pub(super) fn remaining(self, now: Time) -> Duration {
        // Wall time accounts for sleep. Monotonic time keeps a backwards clock
        // adjustment from indefinitely postponing a check or retry.
        let wall = now.wall.duration_since(self.start.wall).unwrap_or_default();
        let monotonic = now
            .monotonic
            .saturating_duration_since(self.start.monotonic);
        self.delay.saturating_sub(wall.max(monotonic))
    }
}

pub(super) struct Schedule {
    lookup: Deadline,
    download: Option<Deadline>,
    check_retry_used: bool,
}

impl Schedule {
    pub(super) fn new(now: Time) -> Self {
        Self {
            lookup: Deadline::after(now, STARTUP_DELAY),
            download: None,
            check_retry_used: false,
        }
    }

    pub(super) fn remaining(&self, now: Time) -> Duration {
        let lookup = self.lookup.remaining(now);
        self.download
            .map_or(lookup, |download| lookup.min(download.remaining(now)))
    }

    pub(super) fn checking(&mut self, now: Time) {
        // Consume due attempts only. An earlier manual or wake check may fail
        // while a short download retry still needs its own future lookup.
        self.download
            .take_if(|deadline| deadline.remaining(now).is_zero());
    }

    pub(super) fn checked(&mut self, now: Time, success: bool) {
        let delay = if !success && !self.check_retry_used {
            QUICK_RETRY
        } else {
            CHECK_INTERVAL
        };
        self.check_retry_used = !success;
        self.lookup = Deadline::after(now, delay);
        if success {
            // The current package will rearm its short retry if still pending.
            self.download = None;
        }
    }

    pub(super) fn retry_download_at(&mut self, retry: Deadline) {
        self.download = Some(retry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(time: Time, seconds: u64) -> Time {
        let delay = Duration::from_secs(seconds);
        Time {
            wall: time.wall + delay,
            monotonic: time.monotonic + delay,
        }
    }

    #[test]
    fn checks_once_after_startup_and_again_after_half_an_hour() {
        let start = Time::now();
        let mut schedule = Schedule::new(start);
        assert_eq!(schedule.remaining(start), Duration::from_secs(10));
        assert_eq!(schedule.remaining(after(start, 9)), Duration::from_secs(1));

        let first_check = after(start, 10);
        assert!(schedule.remaining(first_check).is_zero());
        schedule.checked(first_check, true);
        assert_eq!(
            schedule.remaining(after(first_check, 1799)),
            Duration::from_secs(1)
        );
        assert!(schedule.remaining(after(first_check, 1800)).is_zero());
    }

    #[test]
    fn overdue_wake_skips_missed_checks_and_clock_rewind_does_not_delay_them() {
        let mut now = Time::now();
        let mut schedule = Schedule::new(now);
        schedule.checked(now, true);
        now.wall += Duration::from_secs(3 * 60 * 60);
        assert!(schedule.remaining(now).is_zero());

        schedule.checked(now, true);
        assert_eq!(schedule.remaining(now), CHECK_INTERVAL);
        now.wall -= Duration::from_secs(3 * 60 * 60);
        now.monotonic += CHECK_INTERVAL;
        assert!(schedule.remaining(now).is_zero());
    }

    #[test]
    fn gives_each_streak_of_lookup_failures_one_quick_retry() {
        let now = Time::now();
        let mut schedule = Schedule::new(now);
        schedule.checked(now, false);
        assert_eq!(schedule.remaining(now), QUICK_RETRY);

        schedule.checked(now, false);
        assert_eq!(schedule.remaining(now), CHECK_INTERVAL);
        schedule.checked(now, false);
        assert_eq!(schedule.remaining(now), CHECK_INTERVAL);

        schedule.checked(now, true);
        schedule.checked(now, false);
        assert_eq!(schedule.remaining(now), QUICK_RETRY);
    }

    #[test]
    fn manual_failures_before_a_download_retry_do_not_postpone_it() {
        let now = Time::now();
        let mut schedule = Schedule::new(now);
        schedule.checked(now, true);
        schedule.retry_download_at(Deadline::after(now, QUICK_RETRY));

        for elapsed in [60, 120] {
            let manual = after(now, elapsed);
            schedule.checking(manual);
            schedule.checked(manual, false);
        }

        let due = after(now, 600);
        assert!(schedule.remaining(due).is_zero());
        schedule.checking(due);
        schedule.checked(due, false);
        assert_eq!(schedule.remaining(due), CHECK_INTERVAL);
    }

    #[test]
    fn an_overdue_lookup_can_rearm_the_current_packages_quick_retry() {
        let now = Time::now();
        let mut schedule = Schedule::new(now);
        schedule.checked(now, true);
        let woke = after(now, 3600);
        let download_retry = Deadline::after(woke, QUICK_RETRY);
        schedule.retry_download_at(download_retry);
        assert!(schedule.remaining(woke).is_zero());

        schedule.checking(woke);
        schedule.checked(woke, true);
        schedule.retry_download_at(download_retry);
        assert_eq!(schedule.remaining(woke), QUICK_RETRY);
        assert!(schedule.remaining(after(woke, 600)).is_zero());
    }
}
