use steel_utils::types::GameType;

/// Current and previous player game mode stored as one coherent state.
#[derive(Debug, Clone, Copy)]
pub(super) struct PlayerGameModeState {
    current: GameType,
    previous: GameType,
}

impl PlayerGameModeState {
    #[must_use]
    pub(super) const fn new(initial: GameType) -> Self {
        Self {
            current: initial,
            previous: initial,
        }
    }

    #[must_use]
    pub(super) const fn current(self) -> GameType {
        self.current
    }

    #[must_use]
    pub(super) const fn previous(self) -> GameType {
        self.previous
    }

    pub(super) const fn set_pair(&mut self, current: GameType, previous: GameType) {
        self.current = current;
        self.previous = previous;
    }

    pub(super) fn change_current(&mut self, game_mode: GameType) -> bool {
        if self.current == game_mode {
            return false;
        }

        self.previous = self.current;
        self.current = game_mode;
        true
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::types::GameType;

    use super::PlayerGameModeState;

    #[test]
    fn changing_current_records_previous_mode() {
        let mut state = PlayerGameModeState::new(GameType::Survival);

        assert!(state.change_current(GameType::Creative));

        assert_eq!(state.current(), GameType::Creative);
        assert_eq!(state.previous(), GameType::Survival);
    }

    #[test]
    fn setting_same_mode_keeps_previous_mode() {
        let mut state = PlayerGameModeState::new(GameType::Survival);
        state.change_current(GameType::Creative);

        assert!(!state.change_current(GameType::Creative));

        assert_eq!(state.current(), GameType::Creative);
        assert_eq!(state.previous(), GameType::Survival);
    }

    #[test]
    fn persistent_restore_sets_current_and_previous() {
        let mut state = PlayerGameModeState::new(GameType::Survival);

        state.set_pair(GameType::Adventure, GameType::Creative);

        assert_eq!(state.current(), GameType::Adventure);
        assert_eq!(state.previous(), GameType::Creative);
    }
}
