/// Per-player tick counters and once-per-tick packet state.
#[derive(Debug, Clone, Copy)]
pub(super) struct PlayerTickState {
    tick_count: i32,
    attack_strength_ticker: i32,
    take_xp_delay: i32,
    ack_block_changes_up_to: i32,
}

impl PlayerTickState {
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            tick_count: 0,
            attack_strength_ticker: 0,
            take_xp_delay: 0,
            ack_block_changes_up_to: -1,
        }
    }

    #[must_use]
    pub(super) const fn tick_count(self) -> i32 {
        self.tick_count
    }

    #[must_use]
    pub(super) const fn attack_strength_ticker(self) -> i32 {
        self.attack_strength_ticker
    }

    #[must_use]
    pub(super) const fn take_xp_delay(self) -> i32 {
        self.take_xp_delay
    }

    pub(super) const fn set_take_xp_delay(&mut self, delay: i32) {
        self.take_xp_delay = delay;
    }

    pub(super) const fn advance_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.take_xp_delay > 0 {
            self.take_xp_delay -= 1;
        }
    }

    pub(super) const fn advance_attack_strength_ticker(&mut self) {
        self.attack_strength_ticker = self.attack_strength_ticker.wrapping_add(1);
    }

    pub(super) const fn reset_attack_strength_ticker(&mut self) {
        self.attack_strength_ticker = 0;
    }

    pub(super) const fn ack_block_changes_up_to(&mut self, sequence: i32) {
        if sequence > self.ack_block_changes_up_to {
            self.ack_block_changes_up_to = sequence;
        }
    }

    pub(super) const fn take_ack_block_changes_up_to(&mut self) -> i32 {
        let sequence = self.ack_block_changes_up_to;
        self.ack_block_changes_up_to = -1;
        sequence
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerTickState;

    #[test]
    fn tick_count_advances_with_wrapping_semantics() {
        let mut state = PlayerTickState::new();

        state.advance_tick();
        state.advance_tick();

        assert_eq!(state.tick_count(), 2);
    }

    #[test]
    fn attack_strength_ticker_advances_and_resets() {
        let mut state = PlayerTickState::new();

        state.advance_attack_strength_ticker();
        state.advance_attack_strength_ticker();
        assert_eq!(state.attack_strength_ticker(), 2);

        state.reset_attack_strength_ticker();
        assert_eq!(state.attack_strength_ticker(), 0);
    }

    #[test]
    fn xp_pickup_delay_decrements_during_tick() {
        let mut state = PlayerTickState::new();
        state.set_take_xp_delay(2);

        state.advance_tick();
        assert_eq!(state.take_xp_delay(), 1);

        state.advance_tick();
        assert_eq!(state.take_xp_delay(), 0);

        state.advance_tick();
        assert_eq!(state.take_xp_delay(), 0);
    }

    #[test]
    fn block_ack_keeps_highest_sequence_until_taken() {
        let mut state = PlayerTickState::new();

        state.ack_block_changes_up_to(3);
        state.ack_block_changes_up_to(1);
        state.ack_block_changes_up_to(5);

        assert_eq!(state.take_ack_block_changes_up_to(), 5);
        assert_eq!(state.take_ack_block_changes_up_to(), -1);
    }
}
