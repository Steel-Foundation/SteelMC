//! Movement tracking state for position validation, broadcast delta detection,
//! and anti-cheat rate limiting.

use glam::DVec3;

use crate::entity::{
    EntityPositionSyncDecision, EntityPositionSyncState, EntityRotationSyncState,
    PackedEntityRotation,
};

/// Player movement packets force a full entity position sync after this delay.
const PLAYER_FULL_SYNC_DELAY: i32 = 400;

/// Internal movement tracking state, stored behind a single `SyncMutex` on `Player`.
pub struct MovementState {
    /// Position/on-ground state used for tracking movement packets.
    position_sync: EntityPositionSyncState,
    /// Packed body/head rotation state used for tracking movement packets.
    rotation_sync: EntityRotationSyncState,

    /// Last known good position (for collision rollback).
    last_good_position: DVec3,
    /// Position at start of tick (for speed validation).
    /// Matches vanilla `firstGoodX/Y/Z`.
    first_good_position: DVec3,

    /// Number of move packets received since connection started.
    received_move_packet_count: i32,
    /// Number of move packets at the last tick (for rate limiting).
    known_move_packet_count: i32,

    /// Remaining ticks for vanilla post-impulse movement validation grace.
    post_impulse_grace_time: i32,

    /// Last movement accepted from the client.
    ///
    /// Mirrors vanilla `ServerPlayer.lastKnownClientMovement`.
    last_known_client_movement: DVec3,

    /// Whether the last accepted client move appeared to be unsupported in air.
    client_is_floating: bool,
    /// Number of consecutive ticks the client has appeared to be floating.
    above_ground_tick_count: i32,
}

impl MovementState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            position_sync: EntityPositionSyncState::new(DVec3::new(0.0, 0.0, 0.0), false),
            rotation_sync: EntityRotationSyncState::new((0.0, 0.0), 0.0),
            last_good_position: DVec3::new(0.0, 0.0, 0.0),
            first_good_position: DVec3::new(0.0, 0.0, 0.0),
            received_move_packet_count: 0,
            known_move_packet_count: 0,
            post_impulse_grace_time: 0,
            last_known_client_movement: DVec3::new(0.0, 0.0, 0.0),
            client_is_floating: false,
            above_ground_tick_count: 0,
        }
    }

    /// Returns the last absolute position used as the client's movement delta base.
    #[must_use]
    pub(super) const fn last_sent_position(&self) -> DVec3 {
        self.position_sync.last_sent_position()
    }

    /// Resets per-tick vanilla movement validation bases.
    pub(super) const fn reset_for_tick(&mut self, position: DVec3) {
        self.first_good_position = position;
        self.last_good_position = position;
        self.known_move_packet_count = self.received_move_packet_count;
    }

    /// Resets movement validation and tracking bases after a server position sync.
    pub(super) fn reset_for_position_sync(
        &mut self,
        position: DVec3,
        on_ground: bool,
        rotation: (f32, f32),
    ) {
        self.position_sync = EntityPositionSyncState::new(position, on_ground);
        self.rotation_sync = EntityRotationSyncState::new(rotation, rotation.0);
        self.last_good_position = position;
        self.first_good_position = position;
        self.received_move_packet_count = 0;
        self.known_move_packet_count = 0;
        self.last_known_client_movement = DVec3::ZERO;
        self.reset_flying_ticks();
    }

    /// Returns the current vanilla first-good and last-good validation positions.
    #[must_use]
    pub(super) const fn good_positions(&self) -> (DVec3, DVec3) {
        (self.first_good_position, self.last_good_position)
    }

    /// Records a received movement packet and returns packets since the last tick.
    pub(super) const fn record_move_packet_delta(&mut self) -> i32 {
        self.received_move_packet_count += 1;
        self.received_move_packet_count - self.known_move_packet_count
    }

    /// Marks a movement target as the latest accepted vanilla last-good position.
    pub(super) const fn mark_last_good_position(&mut self, position: DVec3) {
        self.last_good_position = position;
    }

    /// Applies vanilla post-impulse movement validation grace.
    pub(super) const fn apply_post_impulse_grace_time(&mut self, ticks: i32) {
        if ticks > self.post_impulse_grace_time {
            self.post_impulse_grace_time = ticks;
        }
    }

    /// Returns whether movement validation is inside post-impulse grace.
    #[must_use]
    pub(super) const fn is_in_post_impulse_grace_time(&self) -> bool {
        self.post_impulse_grace_time > 0
    }

    /// Decrements post-impulse grace once per player tick.
    pub(super) const fn tick_post_impulse_grace_time(&mut self) {
        if self.post_impulse_grace_time > 0 {
            self.post_impulse_grace_time -= 1;
        }
    }

    /// Sets the last accepted client movement vector.
    pub(super) const fn set_last_known_client_movement(&mut self, movement: DVec3) {
        self.last_known_client_movement = movement;
    }

    /// Clears the last accepted client movement vector.
    pub(super) const fn reset_last_known_client_movement(&mut self) {
        self.last_known_client_movement = DVec3::ZERO;
    }

    /// Returns the last accepted client movement vector.
    #[must_use]
    pub(super) const fn last_known_client_movement(&self) -> DVec3 {
        self.last_known_client_movement
    }

    /// Records whether the latest accepted movement made the client appear to float.
    pub(super) const fn record_client_floating(&mut self, client_is_floating: bool) {
        self.client_is_floating = client_is_floating;
    }

    /// Resets the vanilla floating violation counter.
    pub(super) const fn reset_flying_ticks(&mut self) {
        self.above_ground_tick_count = 0;
    }

    /// Advances the vanilla floating violation tracker.
    ///
    /// Returns true once the client has exceeded the configured maximum flying ticks.
    pub(super) const fn tick_client_floating(
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

    /// Selects and records the player movement sync form.
    pub(super) fn record_position_sync(
        &mut self,
        position: DVec3,
        on_ground: bool,
    ) -> EntityPositionSyncDecision {
        let delay = self.position_sync.advance_sync_delay();
        let on_ground_changed = self.position_sync.last_sent_on_ground() != on_ground;
        let force_full = delay > PLAYER_FULL_SYNC_DELAY || on_ground_changed;
        self.position_sync
            .record_movement_sync(position, on_ground, force_full)
    }

    /// Records a body rotation packet when the packed yaw or pitch changed.
    pub(super) fn record_body_rotation_sync(
        &mut self,
        rotation: (f32, f32),
    ) -> Option<PackedEntityRotation> {
        self.rotation_sync.record_body_rotation(rotation)
    }

    /// Marks body rotation as sent because a full position sync includes it.
    pub(super) fn mark_body_rotation_sent(&mut self, rotation: (f32, f32)) {
        self.rotation_sync.mark_body_rotation_sent(rotation);
    }

    /// Records a head-rotation packet when the packed yaw changed.
    pub(super) fn record_head_yaw_sync(&mut self, head_yaw: f32) -> Option<i8> {
        self.rotation_sync.record_head_yaw(head_yaw)
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
