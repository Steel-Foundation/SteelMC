//! Local calendar dates that change gameplay behavior.

use chrono::{Datelike, Local};

const HALLOWEEN_MONTH: u32 = 10;
const HALLOWEEN_DAY: u32 = 31;

/// Returns the current local month and day.
#[must_use]
pub fn day_now() -> (u32, u32) {
    let now = Local::now();
    (now.month(), now.day())
}

/// Returns whether today is Halloween in the server's local time zone.
#[must_use]
pub fn is_halloween() -> bool {
    day_now() == (HALLOWEEN_MONTH, HALLOWEEN_DAY)
}
