/// Client lifecycle flags that gate gameplay packet handling.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PlayerLifecycleState {
    client_loaded: bool,
    domain_switching: bool,
}

impl PlayerLifecycleState {
    #[must_use]
    pub(super) const fn client_loaded(self) -> bool {
        self.client_loaded
    }

    pub(super) const fn set_client_loaded(&mut self, client_loaded: bool) {
        self.client_loaded = client_loaded;
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
}

#[cfg(test)]
mod tests {
    use super::PlayerLifecycleState;

    #[test]
    fn domain_switch_starts_once_until_finished() {
        let mut state = PlayerLifecycleState::default();

        assert!(state.begin_domain_switch());
        assert!(!state.begin_domain_switch());

        state.finish_domain_switch();
        assert!(state.begin_domain_switch());
    }

    #[test]
    fn client_loaded_flag_is_explicit() {
        let mut state = PlayerLifecycleState::default();

        assert!(!state.client_loaded());
        state.set_client_loaded(true);
        assert!(state.client_loaded());
        state.set_client_loaded(false);
        assert!(!state.client_loaded());
    }
}
