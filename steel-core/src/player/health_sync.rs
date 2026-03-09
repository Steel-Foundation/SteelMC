//! Delta-tracking state for `CSetHealth` packet deduplication.
//!
//! Tracks the last health/food/saturation values sent to the client so we only
//! send `CSetHealth` when something actually changes.
//!
//! Vanilla: `ServerPlayer.lastSentHealth`, `lastSentFood`, `lastFoodSaturationZero`.

/// Tracks the last health/food/saturation values sent to the client.
pub struct HealthSyncState {
    /// Last health value sent to the client.
    pub last_health: f32,
    /// Last food level sent to the client.
    pub last_food: i32,
    /// Whether saturation was zero last time we sent health.
    pub saturation_zero: bool,
}

impl HealthSyncState {
    /// Creates a new state that will trigger a send on the first tick.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_health: -1.0,
            last_food: -1,
            saturation_zero: true,
        }
    }

    /// Returns true if the given values differ from what was last sent.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn needs_update(&self, health: f32, food: i32, saturation_zero: bool) -> bool {
        self.last_health != health
            || self.last_food != food
            || self.saturation_zero != saturation_zero
    }

    /// Records that we just sent the given values to the client.
    pub const fn record_sent(&mut self, health: f32, food: i32, saturation_zero: bool) {
        self.last_health = health;
        self.last_food = food;
        self.saturation_zero = saturation_zero;
    }

    /// Invalidates the state so the next tick will re-send.
    ///
    /// Vanilla: `resetSentInfo`.
    pub const fn invalidate(&mut self) {
        self.last_health = -1.0e8;
    }

    /// Resets to respawn defaults (forces re-send on next tick).
    pub const fn reset_for_respawn(&mut self) {
        self.last_health = -1.0;
        self.last_food = -1;
    }
}
