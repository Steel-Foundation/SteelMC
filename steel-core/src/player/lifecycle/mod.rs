mod respawn;
mod spawn_sync;
mod world_transition;

#[cfg(test)]
pub(super) use spawn_sync::nullable_game_mode_id;

use super::*;

/// Client lifecycle flags that gate gameplay packet handling.
#[derive(Debug, Clone, Copy)]
pub(super) struct PlayerLifecycleState {
    joined_world: bool,
    pending_client_loaded: bool,
    client_loaded_timeout: i32,
    domain_switching: bool,
    pending_respawn: bool,
}

const CLIENT_LOADED_TIMEOUT_TICKS: i32 = 60;

impl Default for PlayerLifecycleState {
    fn default() -> Self {
        Self {
            joined_world: false,
            pending_client_loaded: false,
            client_loaded_timeout: CLIENT_LOADED_TIMEOUT_TICKS,
            domain_switching: false,
            pending_respawn: false,
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
        self.domain_switching
    }

    pub(super) const fn begin_domain_switch(&mut self) -> bool {
        if self.domain_switching {
            return false;
        }

        self.domain_switching = true;
        true
    }

    pub(super) const fn finish_domain_switch(&mut self) {
        self.domain_switching = false;
    }

    pub(super) const fn begin_respawn(&mut self) -> bool {
        if self.pending_respawn {
            return false;
        }

        self.pending_respawn = true;
        true
    }

    pub(super) const fn finish_respawn(&mut self) {
        self.pending_respawn = false;
    }

    #[cfg(test)]
    pub(super) const fn respawn_pending(self) -> bool {
        self.pending_respawn
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
    pub(crate) fn begin_domain_switch(&self) -> bool {
        self.lifecycle.lock().begin_domain_switch()
    }

    /// Clears the domain-switch transition marker.
    pub(crate) fn finish_domain_switch(&self) {
        self.lifecycle.lock().finish_domain_switch();
    }

    /// Returns whether this player is currently switching domains.
    pub fn is_domain_switching(&self) -> bool {
        self.lifecycle.lock().domain_switching()
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

    #[test]
    fn domain_switch_starts_once_until_finished() {
        let mut state = PlayerLifecycleState::default();

        assert!(state.begin_domain_switch());
        assert!(!state.begin_domain_switch());

        state.finish_domain_switch();
        assert!(state.begin_domain_switch());
    }

    #[test]
    fn respawn_starts_once_until_finished() {
        let mut state = PlayerLifecycleState::default();

        assert!(!state.respawn_pending());
        assert!(state.begin_respawn());
        assert!(state.respawn_pending());
        assert!(!state.begin_respawn());

        state.finish_respawn();
        assert!(!state.respawn_pending());
        assert!(state.begin_respawn());
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
