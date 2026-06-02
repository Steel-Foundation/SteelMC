//! Shared movement synchronization state for tracked entities.

use glam::DVec3;
use steel_protocol::packets::game::{PackedEntityDelta, calc_delta};

/// Squared position delta needed before vanilla considers a movement worth syncing.
pub const POSITION_SYNC_THRESHOLD: f64 = 7.629_394_5e-6;

/// Encoded position sync selected for an entity movement update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityPositionSyncDecision {
    /// Delta-encoded movement update.
    Delta {
        /// Packed X delta.
        dx: PackedEntityDelta,
        /// Packed Y delta.
        dy: PackedEntityDelta,
        /// Packed Z delta.
        dz: PackedEntityDelta,
    },
    /// Full absolute position sync.
    Full,
}

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
    pub const fn advance_sync_delay(&mut self) -> i32 {
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
    pub const fn mark_delta_sent(&mut self, position: DVec3, on_ground: bool) {
        self.last_sent_position = position;
        self.last_sent_on_ground = on_ground;
    }

    /// Marks a full position sync packet as sent and resets the full-sync delay.
    pub const fn mark_full_sent(&mut self, position: DVec3, on_ground: bool) {
        self.last_sent_position = position;
        self.last_sent_on_ground = on_ground;
        self.sync_delay = 0;
    }

    /// Selects and records the next movement sync form.
    ///
    /// Callers decide whether a sync is needed and whether vanilla forces a full
    /// sync for their tracking mode. This method owns the shared protocol delta
    /// overflow fallback and updates the sync base consistently.
    pub fn record_movement_sync(
        &mut self,
        position: DVec3,
        on_ground: bool,
        force_full: bool,
    ) -> EntityPositionSyncDecision {
        if !force_full && let Some((dx, dy, dz)) = self.packed_delta(position) {
            self.mark_delta_sent(position, on_ground);
            return EntityPositionSyncDecision::Delta { dx, dy, dz };
        }

        self.mark_full_sent(position, on_ground);
        EntityPositionSyncDecision::Full
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_protocol::packets::game::calc_delta;

    use super::{EntityPositionSyncDecision, EntityPositionSyncState};

    #[test]
    fn movement_sync_records_delta_when_packed_delta_fits() {
        let mut state = EntityPositionSyncState::new(DVec3::ZERO, false);
        state.advance_sync_delay();

        let position = DVec3::new(0.25, -0.125, 0.5);
        let decision = state.record_movement_sync(position, true, false);

        assert_eq!(
            decision,
            EntityPositionSyncDecision::Delta {
                dx: calc_delta(position.x, 0.0).expect("delta should fit"),
                dy: calc_delta(position.y, 0.0).expect("delta should fit"),
                dz: calc_delta(position.z, 0.0).expect("delta should fit"),
            }
        );
        assert_eq!(state.last_sent_position(), position);
        assert!(state.last_sent_on_ground());
        assert_eq!(state.sync_delay(), 1);
    }

    #[test]
    fn movement_sync_records_full_when_forced() {
        let mut state = EntityPositionSyncState::new(DVec3::ZERO, false);
        state.advance_sync_delay();

        let decision = state.record_movement_sync(DVec3::new(0.25, 0.0, 0.0), true, true);

        assert_eq!(decision, EntityPositionSyncDecision::Full);
        assert_eq!(state.last_sent_position(), DVec3::new(0.25, 0.0, 0.0));
        assert!(state.last_sent_on_ground());
        assert_eq!(state.sync_delay(), 0);
    }

    #[test]
    fn movement_sync_records_full_when_delta_overflows() {
        let mut state = EntityPositionSyncState::new(DVec3::ZERO, false);

        let decision = state.record_movement_sync(DVec3::new(10.0, 0.0, 0.0), false, false);

        assert_eq!(decision, EntityPositionSyncDecision::Full);
        assert_eq!(state.last_sent_position(), DVec3::new(10.0, 0.0, 0.0));
    }
}
