//! Movement tracking state for position validation, broadcast delta detection,
//! and anti-cheat rate limiting.

use glam::DVec3;

use crate::entity::EntityPositionSyncState;

/// Internal movement tracking state, stored behind a single `SyncMutex` on `Player`.
pub struct MovementState {
    /// Position/on-ground state used for tracking movement packets.
    pub position_sync: EntityPositionSyncState,
    /// The previous rotation for movement broadcasts.
    pub prev_rotation: (f32, f32),

    /// Last known good position (for collision rollback).
    pub last_good_position: DVec3,
    /// Position at start of tick (for speed validation).
    /// Matches vanilla `firstGoodX/Y/Z`.
    pub first_good_position: DVec3,

    /// Number of move packets received since connection started.
    pub received_move_packet_count: i32,
    /// Number of move packets at the last tick (for rate limiting).
    pub known_move_packet_count: i32,

    /// Tick when last impulse was applied (knockback, etc.).
    pub last_impulse_tick: i32,

    /// Last movement accepted from the client.
    ///
    /// Mirrors vanilla `ServerPlayer.lastKnownClientMovement`.
    pub last_known_client_movement: DVec3,
}

impl MovementState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            position_sync: EntityPositionSyncState::new(DVec3::new(0.0, 0.0, 0.0), false),
            prev_rotation: (0.0, 0.0),
            last_good_position: DVec3::new(0.0, 0.0, 0.0),
            first_good_position: DVec3::new(0.0, 0.0, 0.0),
            received_move_packet_count: 0,
            known_move_packet_count: 0,
            last_impulse_tick: 0,
            last_known_client_movement: DVec3::new(0.0, 0.0, 0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::MovementState;

    #[test]
    fn movement_state_starts_with_zero_known_client_movement() {
        let state = MovementState::new();
        assert_eq!(state.last_known_client_movement, DVec3::ZERO);
    }
}
