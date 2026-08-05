mod respawn;
mod spawn_sync;
mod world_transition;

#[cfg(test)]
pub(super) use spawn_sync::nullable_game_mode_id;

use super::*;
use crate::entity::PendingWorldChangeToken;

/// Client lifecycle flags that gate gameplay packet handling.
#[derive(Debug, Clone, Copy)]
pub(super) struct PlayerLifecycleState {
    joined_world: bool,
    pending_client_loaded: bool,
    client_loaded_timeout: i32,
    domain_switch: Option<DomainSwitchState>,
    respawn: Option<PendingWorldChangeToken>,
    deferred_death_respawn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DomainSwitchState {
    token: PendingWorldChangeToken,
    phase: DomainSwitchPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DomainSwitchPhase {
    Queued,
    Detached,
    TargetHandshake,
    Finalizing,
}

const CLIENT_LOADED_TIMEOUT_TICKS: i32 = 60;

impl Default for PlayerLifecycleState {
    fn default() -> Self {
        Self {
            joined_world: false,
            pending_client_loaded: false,
            client_loaded_timeout: CLIENT_LOADED_TIMEOUT_TICKS,
            domain_switch: None,
            respawn: None,
            deferred_death_respawn: false,
        }
    }
}

impl PlayerLifecycleState {
    #[must_use]
    pub(super) const fn client_loaded(self) -> bool {
        self.client_loaded_timeout <= 0
    }

    #[must_use]
    pub(super) const fn joined_world(self) -> bool {
        self.joined_world
    }

    pub(super) const fn set_joined_world(&mut self, joined_world: bool) {
        self.joined_world = joined_world;
    }

    pub(super) const fn set_client_loaded(&mut self, client_loaded: bool) {
        if !client_loaded {
            self.pending_client_loaded = false;
        }
        self.client_loaded_timeout = if client_loaded {
            0
        } else {
            CLIENT_LOADED_TIMEOUT_TICKS
        };
    }

    pub(super) const fn mark_client_loaded_from_network(&mut self) -> bool {
        if self.joined_world {
            self.set_client_loaded(true);
            return true;
        }

        self.pending_client_loaded = true;
        false
    }

    pub(super) const fn apply_pending_client_loaded(&mut self) -> bool {
        if !self.pending_client_loaded {
            return false;
        }

        self.pending_client_loaded = false;
        self.set_client_loaded(true);
        true
    }

    pub(super) const fn tick_client_load_timeout(&mut self) {
        if self.client_loaded_timeout > 0 {
            self.client_loaded_timeout -= 1;
        }
    }

    #[must_use]
    pub(super) const fn domain_switching(self) -> bool {
        self.domain_switch.is_some()
    }

    #[must_use]
    pub(super) const fn domain_switch_blocks_gameplay(self) -> bool {
        matches!(
            self.domain_switch,
            Some(DomainSwitchState {
                phase: DomainSwitchPhase::Queued
                    | DomainSwitchPhase::Detached
                    | DomainSwitchPhase::TargetHandshake,
                ..
            })
        )
    }

    #[must_use]
    pub(super) const fn gate_domain_switch_packet(
        &mut self,
        handshake_packet: bool,
        defer_death_respawn: bool,
    ) -> bool {
        match self.domain_switch {
            None
            | Some(DomainSwitchState {
                phase: DomainSwitchPhase::Finalizing,
                ..
            }) => true,
            Some(DomainSwitchState {
                phase: DomainSwitchPhase::TargetHandshake,
                ..
            }) => handshake_packet,
            Some(DomainSwitchState {
                phase: DomainSwitchPhase::Queued | DomainSwitchPhase::Detached,
                ..
            }) => {
                if defer_death_respawn && self.respawn.is_none() {
                    self.deferred_death_respawn = true;
                }
                false
            }
        }
    }

    pub(super) const fn begin_domain_switch(&mut self, token: PendingWorldChangeToken) -> bool {
        if self.domain_switch.is_some() || self.respawn.is_some() {
            return false;
        }

        self.domain_switch = Some(DomainSwitchState {
            token,
            phase: DomainSwitchPhase::Queued,
        });
        true
    }

    #[must_use]
    pub(super) fn domain_switch_queued(self, token: PendingWorldChangeToken) -> bool {
        matches!(
            self.domain_switch,
            Some(DomainSwitchState {
                token: active,
                phase: DomainSwitchPhase::Queued,
            }) if active == token
        )
    }

    #[must_use]
    pub(super) fn domain_switch_detached(self, token: PendingWorldChangeToken) -> bool {
        matches!(
            self.domain_switch,
            Some(DomainSwitchState {
                token: active,
                phase: DomainSwitchPhase::Detached,
            }) if active == token
        )
    }

    pub(super) fn mark_domain_switch_detached(&mut self, token: PendingWorldChangeToken) -> bool {
        let Some(state) = self.domain_switch.as_mut() else {
            return false;
        };
        if state.token != token || state.phase != DomainSwitchPhase::Queued {
            return false;
        }

        state.phase = DomainSwitchPhase::Detached;
        true
    }

    pub(super) fn mark_domain_switch_target_handshake(
        &mut self,
        token: PendingWorldChangeToken,
    ) -> bool {
        let Some(state) = self.domain_switch.as_mut() else {
            return false;
        };
        if state.token != token || state.phase != DomainSwitchPhase::Detached {
            return false;
        }

        state.phase = DomainSwitchPhase::TargetHandshake;
        true
    }

    pub(super) fn mark_domain_switch_live(&mut self, token: PendingWorldChangeToken) -> bool {
        let Some(state) = self.domain_switch.as_mut() else {
            return false;
        };
        if state.token != token || state.phase != DomainSwitchPhase::TargetHandshake {
            return false;
        }

        state.phase = DomainSwitchPhase::Finalizing;
        true
    }

    pub(super) fn finish_domain_switch(&mut self, token: PendingWorldChangeToken) -> bool {
        if !matches!(
            self.domain_switch,
            Some(DomainSwitchState { token: active, .. }) if active == token
        ) {
            return false;
        }

        self.domain_switch = None;
        true
    }

    pub(super) const fn begin_respawn(&mut self, token: PendingWorldChangeToken) -> bool {
        if self.respawn.is_some() || self.domain_switch_blocks_gameplay() {
            return false;
        }

        self.respawn = Some(token);
        self.deferred_death_respawn = false;
        true
    }

    pub(super) const fn defer_death_respawn(&mut self) {
        if self.respawn.is_none() {
            self.deferred_death_respawn = true;
        }
    }

    pub(super) const fn take_deferred_death_respawn(&mut self) -> bool {
        let deferred = self.deferred_death_respawn;
        self.deferred_death_respawn = false;
        deferred
    }

    #[must_use]
    pub(super) fn respawn_pending(self, token: PendingWorldChangeToken) -> bool {
        self.respawn == Some(token)
    }

    pub(super) fn finish_respawn(&mut self, token: PendingWorldChangeToken) -> bool {
        if !self.respawn_pending(token) {
            return false;
        }

        self.respawn = None;
        true
    }

    pub(super) fn finish_transition(&mut self, token: PendingWorldChangeToken) -> bool {
        if matches!(
            self.domain_switch,
            Some(DomainSwitchState { token: active, .. }) if active == token
        ) {
            self.domain_switch = None;
            return true;
        }

        self.finish_respawn(token)
    }
}

impl Player {
    /// Sets the world the player is in.
    ///
    /// This is used when the correct world isn't known at construction time
    /// (e.g., when loading saved player data determines the actual world).
    pub(crate) fn set_world(&self, world: Arc<World>) {
        self.base.set_world(Arc::downgrade(&world));
        self.world.store(world);
    }

    /// Marks the player as switching domains if they are not already in a transition.
    pub(crate) fn begin_domain_switch(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().begin_domain_switch(token)
    }

    /// Returns whether the queued domain switch still owns this token.
    pub(crate) fn is_domain_switch_queued(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().domain_switch_queued(token)
    }

    /// Returns whether the detached domain switch still owns this token.
    pub(crate) fn is_domain_switch_detached(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().domain_switch_detached(token)
    }

    /// Marks the token-owned domain switch as detached from its source world.
    pub(crate) fn mark_domain_switch_detached(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().mark_domain_switch_detached(token)
    }

    /// Opens only target-world acknowledgement packets before target insertion completes.
    pub(crate) fn mark_domain_switch_target_handshake(
        &self,
        token: PendingWorldChangeToken,
    ) -> bool {
        self.lifecycle
            .lock()
            .mark_domain_switch_target_handshake(token)
    }

    /// Marks the token-owned domain switch as live in its target world.
    pub(crate) fn mark_domain_switch_live(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().mark_domain_switch_live(token)
    }

    /// Clears a domain switch only if the caller still owns it.
    pub(crate) fn finish_domain_switch(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().finish_domain_switch(token)
    }

    /// Marks a token as owning respawn preparation if no player transition is active.
    pub(crate) fn begin_respawn_transition(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().begin_respawn(token)
    }

    /// Returns whether respawn preparation still owns this token.
    pub(crate) fn is_respawn_transition_pending(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().respawn_pending(token)
    }

    /// Clears respawn preparation only if the caller still owns it.
    pub(crate) fn finish_respawn_transition(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().finish_respawn(token)
    }

    /// Clears either player transition kind only if the caller owns its token.
    pub(crate) fn finish_player_transition(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().finish_transition(token)
    }

    pub(crate) fn defer_death_respawn(&self) {
        self.lifecycle.lock().defer_death_respawn();
    }

    pub(crate) fn retry_deferred_death_respawn(&self) {
        if !self.lifecycle.lock().take_deferred_death_respawn() {
            return;
        }
        if self.connection.closed() || self.get_health() > 0.0 {
            return;
        }
        self.respawn();
    }

    /// Returns whether this player is currently switching domains.
    pub fn is_domain_switching(&self) -> bool {
        self.lifecycle.lock().domain_switching()
    }

    /// Returns whether the current domain-switch phase blocks gameplay work.
    pub(crate) fn domain_switch_blocks_gameplay(&self) -> bool {
        self.lifecycle.lock().domain_switch_blocks_gameplay()
    }

    /// Gates a packet against the current domain phase.
    ///
    /// A dead player's one-shot respawn request is retained while a queued or
    /// detached switch blocks normal gameplay packets.
    pub(crate) fn gate_domain_switch_packet(
        &self,
        handshake_packet: bool,
        perform_respawn: bool,
    ) -> bool {
        let defer_death_respawn = perform_respawn && self.get_health() <= 0.0;
        self.lifecycle
            .lock()
            .gate_domain_switch_packet(handshake_packet, defer_death_respawn)
    }

    #[cfg(test)]
    pub(crate) fn has_deferred_death_respawn_for_test(&self) -> bool {
        self.lifecycle.lock().deferred_death_respawn
    }

    /// Returns whether the server has inserted this player into a world.
    #[must_use]
    pub fn has_joined_world(&self) -> bool {
        self.lifecycle.lock().joined_world()
    }

    /// Marks this player as inserted into a world.
    ///
    /// Returns `true` when a client-loaded acknowledgement arrived before world
    /// admission and was applied by this call.
    pub(crate) fn mark_joined_world(&self) -> bool {
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.set_joined_world(true);
        lifecycle.apply_pending_client_loaded()
    }

    /// Returns whether the client has sent its play-loaded signal.
    #[must_use]
    pub fn has_client_loaded(&self) -> bool {
        self.lifecycle.lock().client_loaded()
    }

    /// Marks whether the client has loaded into play.
    pub fn set_client_loaded(&self, client_loaded: bool) {
        self.lifecycle.lock().set_client_loaded(client_loaded);
    }

    /// Applies or buffers the client's play-loaded acknowledgement.
    ///
    /// Returns `true` when the acknowledgement can run gameplay side effects now.
    pub fn mark_client_loaded_from_network(&self) -> bool {
        self.lifecycle.lock().mark_client_loaded_from_network()
    }

    pub(super) fn tick_client_load_timeout(&self) {
        self.lifecycle.lock().tick_client_load_timeout();
    }
}
/// Why the player is being reset and spawned into a world.
///
/// Controls which packets are sent and how world add/remove is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResetReason {
    /// First time joining the server. `CLogin` was already sent, so `CRespawn` is skipped.
    InitialJoin,
    /// Respawning after death in the same world.
    Respawn,
    /// Respawning after the End credits screen with vanilla packet flags.
    EndCredits,
    /// Teleporting to a different loaded world.
    WorldChange,
}

impl ResetReason {
    pub(super) const fn respawn_data_kept(self) -> i8 {
        match self {
            Self::InitialJoin | Self::Respawn => 0x00,
            Self::EndCredits => 0x01,
            Self::WorldChange => 0x03,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CLIENT_LOADED_TIMEOUT_TICKS, PlayerLifecycleState};
    use crate::entity::PendingWorldChangeToken;

    #[test]
    fn domain_switch_requires_matching_phase_and_token() {
        let mut state = PlayerLifecycleState::default();
        let first = PendingWorldChangeToken::for_test(1);
        let second = PendingWorldChangeToken::for_test(2);

        assert!(state.begin_domain_switch(first));
        assert!(!state.begin_domain_switch(second));
        assert!(state.domain_switch_queued(first));
        assert!(!state.domain_switch_queued(second));
        assert!(state.domain_switch_blocks_gameplay());
        assert!(!state.gate_domain_switch_packet(false, false));
        assert!(!state.gate_domain_switch_packet(true, false));

        assert!(!state.mark_domain_switch_detached(second));
        assert!(state.mark_domain_switch_detached(first));
        assert!(!state.mark_domain_switch_detached(first));
        assert!(state.domain_switch_detached(first));
        assert!(!state.domain_switch_detached(second));
        assert!(state.domain_switch_blocks_gameplay());
        assert!(!state.gate_domain_switch_packet(false, true));
        assert!(state.deferred_death_respawn);

        assert!(!state.mark_domain_switch_live(second));
        assert!(!state.mark_domain_switch_live(first));
        assert!(!state.mark_domain_switch_target_handshake(second));
        assert!(state.mark_domain_switch_target_handshake(first));
        assert!(state.domain_switch_blocks_gameplay());
        assert!(!state.gate_domain_switch_packet(false, false));
        assert!(state.gate_domain_switch_packet(true, false));
        assert!(state.mark_domain_switch_live(first));
        assert!(!state.domain_switch_blocks_gameplay());
        assert!(state.gate_domain_switch_packet(false, false));
        assert!(!state.finish_domain_switch(second));
        assert!(state.domain_switching());
        assert!(state.begin_respawn(second));
        assert!(state.respawn_pending(second));
        assert!(!state.begin_domain_switch(second));
        assert!(state.finish_domain_switch(first));
        assert!(!state.domain_switching());
        assert!(state.respawn_pending(second));
        assert!(!state.begin_domain_switch(first));
        assert!(!state.finish_respawn(first));
        assert!(state.finish_respawn(second));
        assert!(state.begin_domain_switch(first));
    }

    #[test]
    fn client_loaded_flag_is_explicit() {
        let mut state = PlayerLifecycleState::default();

        assert!(!state.client_loaded());
        assert!(!state.mark_client_loaded_from_network());
        assert!(!state.client_loaded());

        state.set_joined_world(true);
        assert!(state.apply_pending_client_loaded());
        assert!(state.client_loaded());

        state.set_client_loaded(true);
        assert!(state.client_loaded());
        state.set_client_loaded(false);
        assert!(!state.client_loaded());
        assert!(!state.apply_pending_client_loaded());
    }

    #[test]
    fn client_load_timeout_eventually_marks_loaded() {
        let mut state = PlayerLifecycleState::default();

        for _ in 0..CLIENT_LOADED_TIMEOUT_TICKS {
            assert!(!state.client_loaded());
            state.tick_client_load_timeout();
        }

        assert!(state.client_loaded());
    }

    #[test]
    fn joined_world_flag_is_explicit() {
        let mut state = PlayerLifecycleState::default();

        assert!(!state.joined_world());
        state.set_joined_world(true);
        assert!(state.joined_world());
        state.set_joined_world(false);
        assert!(!state.joined_world());
    }
}
