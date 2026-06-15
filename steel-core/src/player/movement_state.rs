//! Movement tracking state for position validation and anti-cheat rate limiting.

use glam::DVec3;

use crate::physics::ClientAuthoredMovementState;
use crate::player::PlayerInput;

/// Internal movement tracking state, stored behind a single `SyncMutex` on `Player`.
pub struct MovementState {
    /// Vanilla validation state for client-authored body movement.
    client_movement: ClientAuthoredMovementState,
    /// Vanilla validation state for the controlled root vehicle.
    client_vehicle_movement: ClientAuthoredMovementState,
    /// Whether vanilla accepted player-authored movement during the current client tick.
    received_movement_this_tick: bool,
    /// Entity id of the controlled root vehicle tracked this tick.
    client_vehicle_id: Option<i32>,
    /// Latest vanilla client input snapshot sent by the player.
    last_client_input: PlayerInput,
}

impl MovementState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            client_movement: ClientAuthoredMovementState::new(),
            client_vehicle_movement: ClientAuthoredMovementState::new(),
            received_movement_this_tick: false,
            client_vehicle_id: None,
            last_client_input: PlayerInput::EMPTY,
        }
    }

    /// Stores the latest client input snapshot.
    pub(super) const fn set_last_client_input(&mut self, input: PlayerInput) {
        self.last_client_input = input;
    }

    /// Returns the latest client input snapshot.
    #[must_use]
    pub(super) const fn last_client_input(&self) -> PlayerInput {
        self.last_client_input
    }

    /// Resets per-tick vanilla movement validation bases.
    pub(super) const fn reset_for_tick(&mut self, position: DVec3) {
        self.client_movement.reset_for_tick(position);
    }

    /// Resets per-tick vanilla controlled-vehicle movement validation bases.
    pub(super) const fn reset_vehicle_for_tick(&mut self, vehicle_id: i32, position: DVec3) {
        self.client_vehicle_id = Some(vehicle_id);
        self.client_vehicle_movement.reset_for_tick(position);
    }

    /// Clears the active controlled-vehicle validation state for this tick.
    pub(super) const fn clear_vehicle_for_tick(&mut self) {
        self.client_vehicle_id = None;
        self.client_vehicle_movement.clear_client_floating();
    }

    /// Returns the current vanilla controlled-vehicle validation positions.
    #[must_use]
    pub(super) const fn vehicle_good_positions(&self, vehicle_id: i32) -> Option<(DVec3, DVec3)> {
        if !matches!(self.client_vehicle_id, Some(active_id) if active_id == vehicle_id) {
            return None;
        }

        Some(self.client_vehicle_movement.good_positions())
    }

    /// Resets movement validation bases after a server position sync.
    pub(super) const fn reset_for_position_sync(&mut self, position: DVec3) {
        self.client_movement.reset_for_position_sync(position);
        self.received_movement_this_tick = false;
    }

    /// Returns the current vanilla first-good and last-good validation positions.
    #[must_use]
    pub(super) const fn good_positions(&self) -> (DVec3, DVec3) {
        self.client_movement.good_positions()
    }

    /// Records a received movement packet and returns packets since the last tick.
    pub(super) const fn record_move_packet_delta(&mut self) -> i32 {
        self.client_movement.record_move_packet_delta()
    }

    /// Marks a movement target as the latest accepted vanilla last-good position.
    pub(super) const fn mark_last_good_position(&mut self, position: DVec3) {
        self.client_movement.mark_last_good_position(position);
    }

    /// Marks a controlled-vehicle target as the latest accepted vanilla last-good position.
    pub(super) const fn mark_vehicle_last_good_position(
        &mut self,
        vehicle_id: i32,
        position: DVec3,
    ) {
        if matches!(self.client_vehicle_id, Some(active_id) if active_id == vehicle_id) {
            self.client_vehicle_movement
                .mark_last_good_position(position);
        }
    }

    /// Sets the last accepted client movement vector.
    pub(super) const fn set_last_known_client_movement(&mut self, movement: DVec3) {
        self.client_movement
            .set_last_known_client_movement(movement);
        self.received_movement_this_tick = true;
    }

    /// Clears the last accepted client movement vector.
    pub(super) const fn reset_last_known_client_movement(&mut self) {
        self.client_movement.reset_last_known_client_movement();
        self.received_movement_this_tick = false;
    }

    /// Applies vanilla client-tick-end known-movement handling.
    pub(super) const fn finish_client_tick(&mut self) {
        if !self.received_movement_this_tick {
            self.client_movement.reset_last_known_client_movement();
        }
        self.received_movement_this_tick = false;
    }

    /// Returns the last accepted client movement vector.
    #[must_use]
    pub(super) const fn last_known_client_movement(&self) -> DVec3 {
        self.client_movement.last_known_client_movement()
    }

    /// Records whether the latest accepted movement made the client appear to float.
    pub(super) const fn record_client_floating(&mut self, client_is_floating: bool) {
        self.client_movement
            .record_client_floating(client_is_floating);
    }

    /// Records whether the controlled vehicle appeared to float after accepted movement.
    pub(super) const fn record_vehicle_client_floating(
        &mut self,
        vehicle_id: i32,
        client_is_floating: bool,
    ) {
        if matches!(self.client_vehicle_id, Some(active_id) if active_id == vehicle_id) {
            self.client_vehicle_movement
                .record_client_floating(client_is_floating);
        }
    }

    /// Resets the vanilla floating violation counter.
    pub(super) const fn reset_flying_ticks(&mut self) {
        self.client_movement.reset_flying_ticks();
        self.client_vehicle_movement.reset_flying_ticks();
    }

    /// Advances the vanilla floating violation tracker.
    ///
    /// Returns true once the client has exceeded the configured maximum flying ticks.
    pub(super) const fn tick_client_floating(
        &mut self,
        should_count: bool,
        maximum_flying_ticks: i32,
    ) -> bool {
        self.client_movement
            .tick_client_floating(should_count, maximum_flying_ticks)
    }

    /// Advances the vanilla controlled-vehicle floating violation tracker.
    ///
    /// Returns true once the client has exceeded the configured maximum flying ticks.
    pub(super) const fn tick_vehicle_client_floating(
        &mut self,
        vehicle_id: i32,
        maximum_flying_ticks: i32,
    ) -> bool {
        if !matches!(self.client_vehicle_id, Some(active_id) if active_id == vehicle_id) {
            self.client_vehicle_movement.clear_client_floating();
            return false;
        }

        self.client_vehicle_movement
            .tick_client_floating(true, maximum_flying_ticks)
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use crate::player::PlayerInput;

    use super::MovementState;

    #[test]
    fn movement_state_starts_with_zero_known_client_movement() {
        let state = MovementState::new();
        assert_eq!(state.last_known_client_movement(), DVec3::ZERO);
    }

    #[test]
    fn client_tick_end_clears_known_movement_without_accepted_movement() {
        let mut state = MovementState::new();
        state.set_last_known_client_movement(DVec3::new(0.1, 0.0, 0.0));
        state.finish_client_tick();

        state.finish_client_tick();

        assert_eq!(state.last_known_client_movement(), DVec3::ZERO);
    }

    #[test]
    fn client_tick_end_keeps_known_movement_after_accepted_movement() {
        let mut state = MovementState::new();
        let movement = DVec3::new(0.1, 0.0, 0.0);

        state.set_last_known_client_movement(movement);
        state.finish_client_tick();

        assert_eq!(state.last_known_client_movement(), movement);
    }

    #[test]
    fn movement_state_tracks_last_client_input() {
        let mut state = MovementState::new();
        let input = PlayerInput::from_flags(0x01 | 0x08 | 0x10 | 0x40);

        assert_eq!(state.last_client_input(), PlayerInput::EMPTY);
        state.set_last_client_input(input);

        assert_eq!(state.last_client_input(), input);
        assert_eq!(
            state.last_client_input().movement_input(),
            DVec3::new(-1.0, 0.0, 1.0)
        );
    }

    #[test]
    fn tick_reset_updates_both_good_positions_and_packet_count() {
        let mut state = MovementState::new();
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
    fn position_sync_reset_clears_packet_counts_and_known_movement() {
        let mut state = MovementState::new();
        state.record_move_packet_delta();
        state.set_last_known_client_movement(DVec3::new(0.1, 0.0, 0.0));

        state.reset_for_position_sync(DVec3::new(2.0, 3.0, 4.0));

        assert_eq!(state.good_positions().0, DVec3::new(2.0, 3.0, 4.0));
        assert_eq!(state.good_positions().1, DVec3::new(2.0, 3.0, 4.0));
        assert_eq!(state.last_known_client_movement(), DVec3::ZERO);
        assert_eq!(state.record_move_packet_delta(), 1);
    }

    #[test]
    fn floating_tracker_counts_only_while_client_is_floating() {
        let mut state = MovementState::new();
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
    fn floating_tracker_resets_when_tick_conditions_do_not_count() {
        let mut state = MovementState::new();
        state.record_client_floating(true);

        assert!(!state.tick_client_floating(true, 1));
        assert!(!state.tick_client_floating(false, 1));

        state.record_client_floating(true);
        assert!(!state.tick_client_floating(true, 1));
    }

    #[test]
    fn vehicle_tick_reset_tracks_active_vehicle_id() {
        let mut state = MovementState::new();

        state.reset_vehicle_for_tick(42, DVec3::new(1.0, 2.0, 3.0));

        assert_eq!(
            state.vehicle_good_positions(42),
            Some((DVec3::new(1.0, 2.0, 3.0), DVec3::new(1.0, 2.0, 3.0)))
        );
        assert_eq!(state.vehicle_good_positions(41), None);
        assert!(!state.tick_vehicle_client_floating(41, 0));
        assert!(!state.tick_vehicle_client_floating(42, 0));
    }

    #[test]
    fn vehicle_last_good_update_is_guarded_by_active_vehicle_id() {
        let mut state = MovementState::new();
        state.reset_vehicle_for_tick(42, DVec3::new(1.0, 2.0, 3.0));

        state.mark_vehicle_last_good_position(41, DVec3::new(9.0, 9.0, 9.0));
        assert_eq!(
            state.vehicle_good_positions(42),
            Some((DVec3::new(1.0, 2.0, 3.0), DVec3::new(1.0, 2.0, 3.0)))
        );

        state.mark_vehicle_last_good_position(42, DVec3::new(4.0, 5.0, 6.0));

        assert_eq!(
            state.vehicle_good_positions(42),
            Some((DVec3::new(1.0, 2.0, 3.0), DVec3::new(4.0, 5.0, 6.0)))
        );
    }

    #[test]
    fn vehicle_floating_update_is_guarded_by_active_vehicle_id() {
        let mut state = MovementState::new();
        state.reset_vehicle_for_tick(42, DVec3::new(1.0, 2.0, 3.0));

        state.record_vehicle_client_floating(41, true);
        assert!(!state.tick_vehicle_client_floating(42, 0));

        state.record_vehicle_client_floating(42, true);
        assert!(state.tick_vehicle_client_floating(42, 0));
    }

    #[test]
    fn vehicle_state_clears_when_no_controlled_vehicle_is_active() {
        let mut state = MovementState::new();
        state.reset_vehicle_for_tick(42, DVec3::new(1.0, 2.0, 3.0));

        state.clear_vehicle_for_tick();

        assert!(!state.tick_vehicle_client_floating(42, 0));
    }

    #[test]
    fn reset_flying_ticks_applies_to_vehicle_counter() {
        let mut state = MovementState::new();
        state.reset_vehicle_for_tick(42, DVec3::new(1.0, 2.0, 3.0));
        state.client_vehicle_movement.record_client_floating(true);

        assert!(!state.tick_vehicle_client_floating(42, 1));
        state.reset_flying_ticks();
        assert!(!state.tick_vehicle_client_floating(42, 1));
        assert!(state.tick_vehicle_client_floating(42, 1));
    }
}
