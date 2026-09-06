//! Vanilla server overload handling.
//!
//! When the game loop cannot finish its ticks inside the tick budget it falls
//! behind the wall clock. Vanilla does not replay that backlog: once the loop is
//! far enough behind it reports "Can't keep up" and skips the missed ticks
//! outright, so the world keeps running at its normal pace instead of
//! fast-forwarding until it catches up.
//!
//! Mirrors the overload branch of vanilla `MinecraftServer.runServer`.

use std::time::{Duration, Instant};

/// How far behind the wall clock the loop may fall before missed ticks are
/// dropped. Vanilla `MinecraftServer.OVERLOADED_THRESHOLD_NANOS`.
const OVERLOAD_THRESHOLD: Duration = Duration::from_secs(1);
/// Extra slack on that threshold, counted in ticks. Vanilla
/// `MinecraftServer.OVERLOADED_TICKS_THRESHOLD`.
const OVERLOAD_THRESHOLD_TICKS: u32 = 20;
/// Shortest gap between two "Can't keep up" reports. Vanilla
/// `MinecraftServer.OVERLOADED_WARNING_INTERVAL_NANOS`.
const OVERLOAD_WARNING_INTERVAL: Duration = Duration::from_secs(10);
/// Extra slack on that gap, counted in ticks. Vanilla
/// `MinecraftServer.OVERLOADED_TICKS_WARNING_INTERVAL`.
const OVERLOAD_WARNING_INTERVAL_TICKS: u32 = 100;

/// Tracks how long the game loop has been running behind and drops the backlog
/// when it grows past the vanilla threshold.
pub(super) struct TickOverloadGuard {
    /// When the last backlog drop was reported, used to rate-limit both the
    /// report and the drop itself, exactly as vanilla does.
    last_report: Instant,
}

impl TickOverloadGuard {
    /// Creates a guard that has just reported, so a fresh server gets a full
    /// warning interval of grace before its first report.
    pub(super) const fn new(now: Instant) -> Self {
        Self { last_report: now }
    }

    /// Forgets the recorded backlog, for when the loop deliberately stops
    /// following the wall clock. Vanilla does this while sprinting.
    pub(super) const fn reset(&mut self, now: Instant) {
        self.last_report = now;
    }

    /// Skips the ticks the loop is behind by, when it is far enough behind and
    /// the last report is old enough, and returns how many were skipped.
    ///
    /// `next_tick_time` is advanced past the skipped ticks, so the loop resumes
    /// on the wall clock instead of replaying them back to back.
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
        if behind <= threshold
            || next_tick_time.saturating_duration_since(self.last_report) < report_gap
        {
            return 0;
        }

        let behind_nanos = u64::try_from(behind.as_nanos()).unwrap_or(u64::MAX);
        let ticks_behind = behind_nanos / nanoseconds_per_tick;
        log::warn!(
            "Can't keep up! Is the server overloaded? Running {}ms or {ticks_behind} ticks behind",
            behind.as_millis()
        );

        *next_tick_time += Duration::from_nanos(ticks_behind * nanoseconds_per_tick);
        self.last_report = *next_tick_time;
        ticks_behind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NANOS_PER_TICK: u64 = 50_000_000;
    const TICK: Duration = Duration::from_nanos(NANOS_PER_TICK);

    /// How long before the late tick the guard last reported. Ten minutes is
    /// well past the report gap, so it never blocks a skip on its own.
    const SINCE_LAST_REPORT: Duration = Duration::from_secs(600);

    /// Builds a loop that is `behind` late for its next tick, returning the
    /// current time, that tick's deadline, and a guard that is free to report.
    fn running_behind(behind: Duration) -> (Instant, Instant, TickOverloadGuard) {
        let last_report = Instant::now();
        let next_tick_time = last_report + SINCE_LAST_REPORT;
        (
            next_tick_time + behind,
            next_tick_time,
            TickOverloadGuard::new(last_report),
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
        // Half a second behind, well inside the threshold, so vanilla catches up
        // by running these ticks back to back rather than dropping them.
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
    fn a_zero_length_tick_is_ignored() {
        let (now, mut next_tick_time, mut guard) = running_behind(Duration::from_secs(5));

        let skipped = guard.skip_backlog_if_overloaded(now, &mut next_tick_time, 0);

        assert_eq!(skipped, 0, "a zero tick length cannot be divided by");
    }
}
