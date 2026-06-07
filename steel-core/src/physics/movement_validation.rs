//! Shared vanilla movement-validation helpers for client-authored movement.

use glam::DVec3;

/// Movement error threshold for anti-cheat validation (squared distance).
/// Vanilla uses 0.0625 (1/16 block squared).
pub const MOVEMENT_ERROR_THRESHOLD: f64 = 0.0625;

/// Y-axis tolerance value used by vanilla's movement-error branch.
///
/// Vanilla currently uses `yDist > -0.5 || yDist < 0.5`, which zeroes every
/// finite Y residual before the moved-wrongly check.
pub const Y_TOLERANCE: f64 = 0.5;

/// Collision state used to decide whether a client-authored movement is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementCollisionValidation {
    /// Whether the entity bypasses collision physics.
    pub no_physics: bool,
    /// Whether the simulated server position differs too much from the client target.
    pub moved_wrongly: bool,
    /// Whether the entity already intersected collision before the movement.
    pub old_collision: bool,
    /// Whether accepting the target would newly intersect collision.
    pub new_collision: bool,
}

impl MovementCollisionValidation {
    /// Returns true when vanilla rejects this movement and rolls back the entity.
    #[must_use]
    pub const fn rejects(self) -> bool {
        !self.no_physics && ((self.moved_wrongly && !self.old_collision) || self.new_collision)
    }
}

/// State shared by vanilla client-authored movement validation.
///
/// Vanilla stores parallel copies of this state in `ServerGamePacketListenerImpl`
/// for the player body and for the controlled root vehicle. Steel keeps it as a
/// reusable value so vehicle movement can use the same bookkeeping instead of
/// growing a second player-local copy.
#[derive(Debug)]
pub(crate) struct ClientAuthoredMovementState {
    /// Last known good position for collision rollback.
    last_good_position: DVec3,
    /// Position at the start of the tick for speed validation.
    first_good_position: DVec3,
    /// Number of move packets received since connection start.
    received_move_packet_count: i32,
    /// Number of move packets at the last tick.
    known_move_packet_count: i32,
    /// Last movement accepted from the client.
    last_known_client_movement: DVec3,
    /// Whether the last accepted client move appeared unsupported in air.
    client_is_floating: bool,
    /// Number of consecutive ticks the client has appeared unsupported in air.
    above_ground_tick_count: i32,
}

impl ClientAuthoredMovementState {
    /// Creates empty client-authored movement state.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            last_good_position: DVec3::ZERO,
            first_good_position: DVec3::ZERO,
            received_move_packet_count: 0,
            known_move_packet_count: 0,
            last_known_client_movement: DVec3::ZERO,
            client_is_floating: false,
            above_ground_tick_count: 0,
        }
    }

    /// Resets per-tick vanilla movement validation bases.
    pub(crate) const fn reset_for_tick(&mut self, position: DVec3) {
        self.first_good_position = position;
        self.last_good_position = position;
        self.known_move_packet_count = self.received_move_packet_count;
    }

    /// Resets validation bases after a server position sync.
    pub(crate) const fn reset_for_position_sync(&mut self, position: DVec3) {
        self.last_good_position = position;
        self.first_good_position = position;
        self.received_move_packet_count = 0;
        self.known_move_packet_count = 0;
        self.last_known_client_movement = DVec3::ZERO;
        self.above_ground_tick_count = 0;
    }

    /// Returns the current vanilla first-good and last-good validation positions.
    #[must_use]
    pub(crate) const fn good_positions(&self) -> (DVec3, DVec3) {
        (self.first_good_position, self.last_good_position)
    }

    /// Records a received movement packet and returns packets since the last tick.
    pub(crate) const fn record_move_packet_delta(&mut self) -> i32 {
        self.received_move_packet_count += 1;
        self.received_move_packet_count - self.known_move_packet_count
    }

    /// Marks a movement target as the latest accepted vanilla last-good position.
    pub(crate) const fn mark_last_good_position(&mut self, position: DVec3) {
        self.last_good_position = position;
    }

    /// Sets the last accepted client movement vector.
    pub(crate) const fn set_last_known_client_movement(&mut self, movement: DVec3) {
        self.last_known_client_movement = movement;
    }

    /// Clears the last accepted client movement vector.
    pub(crate) const fn reset_last_known_client_movement(&mut self) {
        self.last_known_client_movement = DVec3::ZERO;
    }

    /// Returns the last accepted client movement vector.
    #[must_use]
    pub(crate) const fn last_known_client_movement(&self) -> DVec3 {
        self.last_known_client_movement
    }

    /// Records whether the latest accepted movement made the client appear to float.
    pub(crate) const fn record_client_floating(&mut self, client_is_floating: bool) {
        self.client_is_floating = client_is_floating;
    }

    /// Clears vanilla floating violation state.
    pub(crate) const fn clear_client_floating(&mut self) {
        self.client_is_floating = false;
        self.above_ground_tick_count = 0;
    }

    /// Resets the vanilla floating violation counter.
    pub(crate) const fn reset_flying_ticks(&mut self) {
        self.above_ground_tick_count = 0;
    }

    /// Advances the vanilla floating violation tracker.
    ///
    /// Returns true once the client has exceeded the configured maximum flying ticks.
    pub(crate) const fn tick_client_floating(
        &mut self,
        should_count: bool,
        maximum_flying_ticks: i32,
    ) -> bool {
        if self.client_is_floating && should_count {
            self.above_ground_tick_count = self.above_ground_tick_count.saturating_add(1);
            return self.above_ground_tick_count > maximum_flying_ticks;
        }

        self.client_is_floating = false;
        self.above_ground_tick_count = 0;
        false
    }
}

/// Returns the residual between a client target and the server-simulated position.
#[must_use]
pub fn movement_error_delta(target_pos: DVec3, simulated_pos: DVec3) -> DVec3 {
    let error_x = target_pos.x - simulated_pos.x;
    let mut error_y = target_pos.y - simulated_pos.y;
    if error_y > -Y_TOLERANCE || error_y < Y_TOLERANCE {
        error_y = 0.0;
    }
    let error_z = target_pos.z - simulated_pos.z;
    DVec3::new(error_x, error_y, error_z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_error_delta_matches_vanilla_y_branch() {
        let delta = movement_error_delta(DVec3::new(10.0, 120.0, -5.0), DVec3::new(8.0, 0.0, -8.0));

        assert_eq!(delta, DVec3::new(2.0, 0.0, 3.0));
    }

    #[test]
    fn movement_validation_accepts_no_physics_even_with_new_collision() {
        assert!(
            !MovementCollisionValidation {
                no_physics: true,
                moved_wrongly: true,
                old_collision: false,
                new_collision: true,
            }
            .rejects()
        );
    }

    #[test]
    fn movement_validation_rejects_new_collision_for_physical_entity() {
        assert!(
            MovementCollisionValidation {
                no_physics: false,
                moved_wrongly: false,
                old_collision: false,
                new_collision: true,
            }
            .rejects()
        );
    }

    #[test]
    fn client_movement_tick_reset_updates_good_positions_and_packet_base() {
        let mut state = ClientAuthoredMovementState::new();
        state.mark_last_good_position(DVec3::new(1.0, 2.0, 3.0));
        state.record_move_packet_delta();
        state.record_move_packet_delta();

        state.reset_for_tick(DVec3::new(4.0, 5.0, 6.0));

        assert_eq!(
            state.good_positions(),
            (DVec3::new(4.0, 5.0, 6.0), DVec3::new(4.0, 5.0, 6.0))
        );
        assert_eq!(state.record_move_packet_delta(), 1);
    }

    #[test]
    fn client_movement_position_sync_reset_clears_packet_counts_and_known_movement() {
        let mut state = ClientAuthoredMovementState::new();
        state.record_move_packet_delta();
        state.set_last_known_client_movement(DVec3::new(0.1, 0.0, 0.0));

        state.reset_for_position_sync(DVec3::new(2.0, 3.0, 4.0));

        assert_eq!(
            state.good_positions(),
            (DVec3::new(2.0, 3.0, 4.0), DVec3::new(2.0, 3.0, 4.0))
        );
        assert_eq!(state.last_known_client_movement(), DVec3::ZERO);
        assert_eq!(state.record_move_packet_delta(), 1);
    }

    #[test]
    fn client_movement_floating_tracker_counts_only_while_floating() {
        let mut state = ClientAuthoredMovementState::new();
        state.record_client_floating(true);

        assert!(!state.tick_client_floating(true, 2));
        assert!(!state.tick_client_floating(true, 2));
        assert!(state.tick_client_floating(true, 2));

        state.record_client_floating(false);
        assert!(!state.tick_client_floating(true, 2));

        state.record_client_floating(true);
        assert!(!state.tick_client_floating(true, 2));
    }

    #[test]
    fn client_movement_floating_tracker_resets_when_tick_conditions_do_not_count() {
        let mut state = ClientAuthoredMovementState::new();
        state.record_client_floating(true);

        assert!(!state.tick_client_floating(true, 1));
        assert!(!state.tick_client_floating(false, 1));

        state.record_client_floating(true);
        assert!(!state.tick_client_floating(true, 1));
    }

    #[test]
    fn client_movement_clear_floating_resets_flag_and_counter() {
        let mut state = ClientAuthoredMovementState::new();
        state.record_client_floating(true);
        assert!(!state.tick_client_floating(true, 1));

        state.clear_client_floating();

        assert!(!state.tick_client_floating(true, 1));
        assert!(!state.tick_client_floating(true, 1));
    }

    #[test]
    fn client_movement_reset_flying_ticks_preserves_current_floating_status() {
        let mut state = ClientAuthoredMovementState::new();
        state.record_client_floating(true);

        assert!(!state.tick_client_floating(true, 1));
        state.reset_flying_ticks();
        assert!(!state.tick_client_floating(true, 1));
        assert!(state.tick_client_floating(true, 1));
    }
}
