//! The overload branch of vanilla `MinecraftServer.runServer`: once the game
//! loop falls far enough behind the wall clock it reports "Can't keep up" and
//! skips the missed ticks rather than replaying them, so the world keeps its
//! normal pace instead of fast-forwarding.

use std::time::{Duration, Instant};

// Vanilla `MinecraftServer` overload constants: how far behind before ticks are
// dropped, and the minimum gap between "Can't keep up" reports, each with tick
// slack on top.
const OVERLOAD_THRESHOLD: Duration = Duration::from_secs(1);
const OVERLOAD_THRESHOLD_TICKS: u32 = 20;
const OVERLOAD_WARNING_INTERVAL: Duration = Duration::from_secs(10);
const OVERLOAD_WARNING_INTERVAL_TICKS: u32 = 100;

/// Tracks how far behind the loop is and drops the backlog past the vanilla
/// threshold.
pub(super) struct TickOverloadGuard {
    /// Last backlog drop, rate-limiting the report and the drop. `None` until
    /// the first report so an early stall is not made to wait out a gap that
    /// never started.
    last_report: Option<Instant>,
}

impl TickOverloadGuard {
    /// A guard that has never reported, so the first large backlog drops at once.
    pub(super) const fn new() -> Self {
        Self { last_report: None }
    }

    /// Forgets the recorded backlog, for when the loop deliberately stops
    /// following the wall clock (vanilla does this while sprinting).
    pub(super) const fn reset(&mut self, now: Instant) {
        self.last_report = Some(now);
    }

    /// Skips the ticks the loop is behind by, if it is far enough behind and the
    /// last report is old enough, and returns how many were skipped.
    /// `next_tick_time` is advanced past them, so the loop resumes on the wall
    /// clock instead of replaying the backlog.
    pub(super) fn skip_backlog_if_overloaded(
        &mut self,
        now: Instant,
        next_tick_time: &mut Instant,
        nanoseconds_per_tick: u64,
    ) -> u64 {
        if nanoseconds_per_tick == 0 {
            return 0;
        }

        let tick = Duration::from_nanos(nanoseconds_per_tick);
        let behind = now.saturating_duration_since(*next_tick_time);
        let threshold = OVERLOAD_THRESHOLD + tick * OVERLOAD_THRESHOLD_TICKS;
        let report_gap = OVERLOAD_WARNING_INTERVAL + tick * OVERLOAD_WARNING_INTERVAL_TICKS;
        let reported_recently = self.last_report.is_some_and(|last_report| {
            next_tick_time.saturating_duration_since(last_report) < report_gap
        });
        if behind <= threshold || reported_recently {
            return 0;
        }

        let behind_nanos = u64::try_from(behind.as_nanos()).unwrap_or(u64::MAX);
        let ticks_behind = behind_nanos / nanoseconds_per_tick;
        log::warn!(
            "Can't keep up! Is the server overloaded? Running {}ms or {ticks_behind} ticks behind",
            behind.as_millis()
        );

        *next_tick_time += Duration::from_nanos(ticks_behind * nanoseconds_per_tick);
        self.last_report = Some(*next_tick_time);
        ticks_behind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NANOS_PER_TICK: u64 = 50_000_000;
    const TICK: Duration = Duration::from_nanos(NANOS_PER_TICK);

    /// A loop `behind` late for its next tick: (now, that tick's deadline, a
    /// fresh guard).
    fn running_behind(behind: Duration) -> (Instant, Instant, TickOverloadGuard) {
        let next_tick_time = Instant::now();
        (
            next_tick_time + behind,
            next_tick_time,
            TickOverloadGuard::new(),
        )
    }

    #[test]
    fn on_time_loop_keeps_its_schedule() {
        let (now, mut next_tick_time, mut guard) = running_behind(Duration::ZERO);
        next_tick_time += TICK;

        let skipped = guard.skip_backlog_if_overloaded(now, &mut next_tick_time, NANOS_PER_TICK);

        assert_eq!(skipped, 0, "a loop that is not behind skips nothing");
        assert_eq!(next_tick_time, now + TICK, "the schedule is left alone");
    }

    #[test]
    fn small_backlog_is_still_replayed() {
        // Well inside the threshold, so the backlog is replayed, not dropped.
        let (now, mut next_tick_time, mut guard) = running_behind(Duration::from_millis(500));

        let skipped = guard.skip_backlog_if_overloaded(now, &mut next_tick_time, NANOS_PER_TICK);

        assert_eq!(skipped, 0, "a backlog under the threshold is not dropped");
    }

    #[test]
    fn large_backlog_is_skipped_and_resyncs_the_clock() {
        let (now, mut next_tick_time, mut guard) = running_behind(Duration::from_secs(5));

        let skipped = guard.skip_backlog_if_overloaded(now, &mut next_tick_time, NANOS_PER_TICK);

        assert_eq!(skipped, 100, "five seconds at 20 ticks per second");
        assert_eq!(
            next_tick_time, now,
            "the loop resumes on the wall clock instead of replaying the backlog"
        );
    }

    #[test]
    fn a_second_report_waits_for_the_report_gap() {
        let (now, mut next_tick_time, mut guard) = running_behind(Duration::from_secs(5));
        assert_eq!(
            guard.skip_backlog_if_overloaded(now, &mut next_tick_time, NANOS_PER_TICK),
            100
        );

        // Behind again immediately, but the previous report is too recent.
        next_tick_time = now;
        let later = now + Duration::from_secs(5);
        let skipped = guard.skip_backlog_if_overloaded(later, &mut next_tick_time, NANOS_PER_TICK);

        assert_eq!(skipped, 0, "reports and skips are rate limited together");
    }

    #[test]
    fn the_first_backlog_is_dropped_without_waiting_for_a_reporting_gap() {
        let (now, mut next_tick_time, mut guard) = running_behind(Duration::from_secs(5));

        let skipped = guard.skip_backlog_if_overloaded(now, &mut next_tick_time, NANOS_PER_TICK);

        assert_eq!(skipped, 100, "the very first overload is not rate limited");
    }

    #[test]
    fn a_zero_length_tick_is_ignored() {
        let (now, mut next_tick_time, mut guard) = running_behind(Duration::from_secs(5));

        let skipped = guard.skip_backlog_if_overloaded(now, &mut next_tick_time, 0);

        assert_eq!(skipped, 0, "a zero tick length cannot be divided by");
    }
}
