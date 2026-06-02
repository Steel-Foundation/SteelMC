//! Shared movement synchronization state for tracked entities.

use glam::DVec3;
use steel_protocol::packets::game::{PackedEntityDelta, calc_delta};

/// Squared position delta needed before vanilla considers a movement worth syncing.
pub const POSITION_SYNC_THRESHOLD: f64 = 7.629_394_5e-6;

/// Per-entity position sync state shared by player and entity tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityPositionSyncState {
    last_sent_position: DVec3,
    last_sent_on_ground: bool,
    sync_delay: i32,
}

impl EntityPositionSyncState {
    /// Creates sync state at the position/on-ground state already known to clients.
    #[must_use]
    pub const fn new(position: DVec3, on_ground: bool) -> Self {
        Self {
            last_sent_position: position,
            last_sent_on_ground: on_ground,
            sync_delay: 0,
        }
    }

    /// Returns the last absolute position used as the client's delta base.
    #[must_use]
    pub const fn last_sent_position(self) -> DVec3 {
        self.last_sent_position
    }

    /// Returns the last on-ground state sent to tracking clients.
    #[must_use]
    pub const fn last_sent_on_ground(self) -> bool {
        self.last_sent_on_ground
    }

    /// Returns the current delay since the last full position sync.
    #[must_use]
    pub const fn sync_delay(self) -> i32 {
        self.sync_delay
    }

    /// Increments the full-sync delay and returns the previous value.
    pub fn advance_sync_delay(&mut self) -> i32 {
        let delay = self.sync_delay;
        self.sync_delay += 1;
        delay
    }

    /// Returns whether `current_position` moved far enough to sync.
    #[must_use]
    pub fn position_changed(self, current_position: DVec3) -> bool {
        let diff = current_position - self.last_sent_position;
        diff.length_squared() >= POSITION_SYNC_THRESHOLD
    }

    /// Encodes the delta from the last sent position to `current_position`.
    ///
    /// Returns `None` when any component overflows the protocol delta range and
    /// the caller must send a full position sync instead.
    #[must_use]
    pub fn packed_delta(
        self,
        current_position: DVec3,
    ) -> Option<(PackedEntityDelta, PackedEntityDelta, PackedEntityDelta)> {
        let dx = calc_delta(current_position.x, self.last_sent_position.x)?;
        let dy = calc_delta(current_position.y, self.last_sent_position.y)?;
        let dz = calc_delta(current_position.z, self.last_sent_position.z)?;
        Some((dx, dy, dz))
    }

    /// Marks a delta movement packet as sent.
    pub fn mark_delta_sent(&mut self, position: DVec3, on_ground: bool) {
        self.last_sent_position = position;
        self.last_sent_on_ground = on_ground;
    }

    /// Marks a full position sync packet as sent and resets the full-sync delay.
    pub fn mark_full_sent(&mut self, position: DVec3, on_ground: bool) {
        self.last_sent_position = position;
        self.last_sent_on_ground = on_ground;
        self.sync_delay = 0;
    }
}
