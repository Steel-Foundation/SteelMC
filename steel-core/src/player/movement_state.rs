//! Movement tracking state for position validation, broadcast delta detection,
//! and anti-cheat rate limiting.

use steel_utils::math::Vector3;

/// Internal movement tracking state, stored behind a single `SyncMutex` on `Player`.
pub struct MovementState {
    /// The previous position for delta movement calculations.
    pub prev_position: Vector3<f64>,
    /// The previous rotation for movement broadcasts.
    pub prev_rotation: (f32, f32),

    /// Player's current velocity (delta movement per tick).
    pub delta_movement: Vector3<f64>,

    /// Last known good position (for collision rollback).
    pub last_good_position: Vector3<f64>,
    /// Position at start of tick (for speed validation).
    /// Matches vanilla `firstGoodX/Y/Z`.
    pub first_good_position: Vector3<f64>,

    /// Number of move packets received since connection started.
    pub received_move_packet_count: i32,
    /// Number of move packets at the last tick (for rate limiting).
    pub known_move_packet_count: i32,

    /// Tick when last impulse was applied (knockback, etc.).
    pub last_impulse_tick: i32,

    /// Tick counter for forced position sync (resets to 0 after sync).
    pub position_sync_delay: i32,

    /// Last `on_ground` state sent to tracking players (for detecting changes).
    pub last_sent_on_ground: bool,
}

impl MovementState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            prev_position: Vector3::new(0.0, 0.0, 0.0),
            prev_rotation: (0.0, 0.0),
            delta_movement: Vector3::new(0.0, 0.0, 0.0),
            last_good_position: Vector3::new(0.0, 0.0, 0.0),
            first_good_position: Vector3::new(0.0, 0.0, 0.0),
            received_move_packet_count: 0,
            known_move_packet_count: 0,
            last_impulse_tick: 0,
            position_sync_delay: 0,
            last_sent_on_ground: false,
        }
    }

    /// Returns the squared length of `delta_movement`.
    #[must_use]
    pub fn delta_movement_length_sq(&self) -> f64 {
        let dm = &self.delta_movement;
        dm.x * dm.x + dm.y * dm.y + dm.z * dm.z
    }
}
