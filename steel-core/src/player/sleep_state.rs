#[derive(Debug, Clone, Copy)]
pub(super) struct PlayerSleepState {
    sleep_counter: i32,
}

impl PlayerSleepState {
    #[must_use]
    pub(super) const fn new() -> Self {
        Self { sleep_counter: 0 }
    }

    #[must_use]
    pub(super) const fn sleep_counter(self) -> i32 {
        self.sleep_counter
    }

    pub(super) const fn set_sleep_counter(&mut self, sleep_counter: i32) {
        self.sleep_counter = sleep_counter;
    }

    pub(super) const fn tick_sleep_counter(&mut self, is_sleeping: bool) {
        if is_sleeping {
            self.sleep_counter += 1;
            if self.sleep_counter > 100 {
                self.sleep_counter = 100;
            }
        } else if self.sleep_counter > 0 {
            self.sleep_counter += 1;
            if self.sleep_counter >= 110 {
                self.sleep_counter = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerSleepState;

    #[test]
    fn sleep_counter_matches_vanilla_wake_animation_window() {
        let mut state = PlayerSleepState::new();

        state.set_sleep_counter(99);
        state.tick_sleep_counter(true);
        state.tick_sleep_counter(true);
        assert_eq!(state.sleep_counter(), 100);

        state.tick_sleep_counter(false);
        assert_eq!(state.sleep_counter(), 101);

        for _ in 0..9 {
            state.tick_sleep_counter(false);
        }
        assert_eq!(state.sleep_counter(), 0);
    }
}
