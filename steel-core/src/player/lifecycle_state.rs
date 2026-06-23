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
