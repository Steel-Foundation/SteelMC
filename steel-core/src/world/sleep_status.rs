use crate::{
    entity::{Entity, LivingEntity},
    player::Player,
};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SleepStatus {
    active_players: i32,
    sleeping_players: i32,
}

impl SleepStatus {
    #[must_use]
    pub(super) fn are_enough_sleeping(self, sleep_percentage_needed: i32) -> bool {
        self.sleeping_players >= self.sleepers_needed(sleep_percentage_needed)
    }

    #[must_use]
    pub(super) fn sleepers_needed(self, sleep_percentage_needed: i32) -> i32 {
        let sleepers =
            ((self.active_players as f32 * sleep_percentage_needed as f32) / 100.0).ceil() as i32;
        sleepers.max(1)
    }

    pub(super) const fn remove_all_sleepers(&mut self) {
        self.sleeping_players = 0;
    }

    #[must_use]
    pub(super) const fn amount_sleeping(self) -> i32 {
        self.sleeping_players
    }

    pub(super) fn update<'a>(&mut self, players: impl IntoIterator<Item = &'a Player>) -> bool {
        let old_active_players = self.active_players;
        let old_sleeping_players = self.sleeping_players;
        self.active_players = 0;
        self.sleeping_players = 0;

        for player in players {
            if player.is_spectator() {
                continue;
            }
            self.active_players += 1;
            if player.is_sleeping() {
                self.sleeping_players += 1;
            }
        }

        (old_sleeping_players > 0 || self.sleeping_players > 0)
            && (old_active_players != self.active_players
                || old_sleeping_players != self.sleeping_players)
    }
}

#[cfg(test)]
mod tests {
    use super::SleepStatus;

    #[test]
    fn sleepers_needed_matches_vanilla_percentage_rule() {
        let status = SleepStatus {
            active_players: 3,
            sleeping_players: 0,
        };

        assert_eq!(status.sleepers_needed(0), 1);
        assert_eq!(status.sleepers_needed(1), 1);
        assert_eq!(status.sleepers_needed(50), 2);
        assert_eq!(status.sleepers_needed(100), 3);
        assert_eq!(status.sleepers_needed(101), 4);
    }
}
