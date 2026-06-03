//! Movement tracking state for position validation, broadcast delta detection,
//! and anti-cheat rate limiting.

use glam::DVec3;

use crate::entity::{EntityMovementSyncState, EntityPositionSyncDecision, PackedEntityRotation};
use crate::physics::ClientAuthoredMovementState;

/// Player movement packets force a full entity position sync after this delay.
const PLAYER_FULL_SYNC_DELAY: i32 = 400;

/// Internal movement tracking state, stored behind a single `SyncMutex` on `Player`.
pub struct MovementState {
    /// Entity movement sync state used for tracking movement packets.
    entity_sync: EntityMovementSyncState,
    /// Vanilla validation state for client-authored body movement.
    client_movement: ClientAuthoredMovementState,
}

impl MovementState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entity_sync: EntityMovementSyncState::new(DVec3::ZERO, false, (0.0, 0.0), 0.0),
            client_movement: ClientAuthoredMovementState::new(),
        }
    }

    /// Returns the last absolute position used as the client's movement delta base.
    #[must_use]
    pub(super) const fn last_sent_position(&self) -> DVec3 {
        self.entity_sync.last_sent_position()
    }

    /// Resets per-tick vanilla movement validation bases.
    pub(super) const fn reset_for_tick(&mut self, position: DVec3) {
        self.client_movement.reset_for_tick(position);
    }

    /// Resets movement validation and tracking bases after a server position sync.
    pub(super) fn reset_for_position_sync(
        &mut self,
        position: DVec3,
        on_ground: bool,
        rotation: (f32, f32),
    ) {
        self.entity_sync = EntityMovementSyncState::new(position, on_ground, rotation, rotation.0);
        self.client_movement.reset_for_position_sync(position);
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

    /// Applies vanilla post-impulse movement validation grace.
    pub(super) const fn apply_post_impulse_grace_time(&mut self, ticks: i32) {
        self.client_movement.apply_post_impulse_grace_time(ticks);
    }

    /// Returns whether movement validation is inside post-impulse grace.
    #[must_use]
    pub(super) const fn is_in_post_impulse_grace_time(&self) -> bool {
        self.client_movement.is_in_post_impulse_grace_time()
    }

    /// Decrements post-impulse grace once per player tick.
    pub(super) const fn tick_post_impulse_grace_time(&mut self) {
        self.client_movement.tick_post_impulse_grace_time();
    }

    /// Sets the last accepted client movement vector.
    pub(super) const fn set_last_known_client_movement(&mut self, movement: DVec3) {
        self.client_movement
            .set_last_known_client_movement(movement);
    }

    /// Clears the last accepted client movement vector.
    pub(super) const fn reset_last_known_client_movement(&mut self) {
        self.client_movement.reset_last_known_client_movement();
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

    /// Resets the vanilla floating violation counter.
    pub(super) const fn reset_flying_ticks(&mut self) {
        self.client_movement.reset_flying_ticks();
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

    /// Selects and records the player movement sync form.
    pub(super) fn record_position_sync(
        &mut self,
        position: DVec3,
        on_ground: bool,
    ) -> EntityPositionSyncDecision {
        self.entity_sync.record_position_sync_with_full_delay(
            position,
            on_ground,
            PLAYER_FULL_SYNC_DELAY,
        )
    }

    /// Records a body rotation packet when the packed yaw or pitch changed.
    pub(super) fn record_body_rotation_sync(
        &mut self,
        rotation: (f32, f32),
    ) -> Option<PackedEntityRotation> {
        self.entity_sync.record_body_rotation(rotation)
    }

    /// Marks body rotation as sent because a full position sync includes it.
    pub(super) fn mark_body_rotation_sent(&mut self, rotation: (f32, f32)) {
        self.entity_sync.mark_body_rotation_sent(rotation);
    }

    /// Records a head-rotation packet when the packed yaw changed.
    pub(super) fn record_head_yaw_sync(&mut self, head_yaw: f32) -> Option<i8> {
        self.entity_sync.record_head_yaw(head_yaw)
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::MovementState;

    #[test]
    fn movement_state_starts_with_zero_known_client_movement() {
        let state = MovementState::new();
        assert_eq!(state.last_known_client_movement(), DVec3::ZERO);
    }

    #[test]
    fn tick_reset_updates_both_good_positions_and_packet_base() {
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
    fn position_sync_reset_clears_packet_counts_known_movement_and_rotation() {
        let mut state = MovementState::new();
        state.record_move_packet_delta();
        state.set_last_known_client_movement(DVec3::new(0.1, 0.0, 0.0));

        state.reset_for_position_sync(DVec3::new(2.0, 3.0, 4.0), true, (90.0, 45.0));

        assert_eq!(state.last_sent_position(), DVec3::new(2.0, 3.0, 4.0));
        assert_eq!(state.good_positions().0, DVec3::new(2.0, 3.0, 4.0));
        assert_eq!(state.good_positions().1, DVec3::new(2.0, 3.0, 4.0));
        assert_eq!(state.last_known_client_movement(), DVec3::ZERO);
        assert_eq!(state.record_move_packet_delta(), 1);
        assert_eq!(state.record_body_rotation_sync((90.0, 45.0)), None);
        assert_eq!(state.record_head_yaw_sync(90.0), None);
    }

    #[test]
    fn post_impulse_grace_counts_down_by_tick() {
        let mut state = MovementState::new();
        state.apply_post_impulse_grace_time(2);

        assert!(state.is_in_post_impulse_grace_time());
        state.tick_post_impulse_grace_time();
        assert!(state.is_in_post_impulse_grace_time());
        state.tick_post_impulse_grace_time();
        assert!(!state.is_in_post_impulse_grace_time());
    }

    #[test]
    fn post_impulse_grace_keeps_larger_existing_window() {
        let mut state = MovementState::new();
        state.apply_post_impulse_grace_time(5);
        state.apply_post_impulse_grace_time(2);

        for _ in 0..4 {
            state.tick_post_impulse_grace_time();
            assert!(state.is_in_post_impulse_grace_time());
        }

        state.tick_post_impulse_grace_time();
        assert!(!state.is_in_post_impulse_grace_time());
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
}
