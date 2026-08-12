use steel_protocol::packets::game::CSystemChat;
use steel_registry::vanilla_game_rules::{
    ADVANCE_TIME, ADVANCE_WEATHER, PLAYERS_SLEEPING_PERCENTAGE,
};
use steel_utils::{Identifier, translations};

use super::{World, sleep_status::SleepStatus};
use crate::entity::LivingEntity as _;

const WAKE_UP_FROM_SLEEP_TIME_MARKER: Identifier = Identifier::vanilla_static("wake_up_from_sleep");

impl World {
    /// Returns whether this world can skip night.
    #[must_use]
    pub fn can_sleep_through_nights(&self) -> bool {
        self.players_sleeping_percentage() <= 100
    }

    fn players_sleeping_percentage(&self) -> i32 {
        self.get_game_rule(&PLAYERS_SLEEPING_PERCENTAGE)
    }

    /// Updates vanilla sleeping player counts and broadcasts the sleep status overlay.
    pub fn update_sleeping_player_list(&self) {
        if self.players.is_empty() {
            return;
        }

        let mut updated_status = SleepStatus::default();
        self.players.iter_players(|_, player| {
            updated_status.add_player(player);
            true
        });

        let mut sleep_status = self.sleep_status.lock();
        let changed = sleep_status.update(updated_status);
        if changed {
            self.announce_sleep_status(*sleep_status);
        }
    }

    fn announce_sleep_status(&self, sleep_status: SleepStatus) {
        if !self.can_sleep_through_nights() {
            return;
        }

        let percentage = self.players_sleeping_percentage();
        let message = if sleep_status.are_enough_sleeping(percentage) {
            translations::SLEEP_SKIPPING_NIGHT.msg().component()
        } else {
            translations::SLEEP_PLAYERS_SLEEPING
                .message([
                    sleep_status.amount_sleeping().to_string(),
                    sleep_status.sleepers_needed(percentage).to_string(),
                ])
                .component()
        };

        self.broadcast_to_all_with(|player| CSystemChat::new(&message, true, player));
    }

    pub(super) fn tick_sleeping_players(&self) {
        if self.players.is_empty() {
            return;
        }

        let percentage = self.players_sleeping_percentage();
        let sleepers_needed = {
            let sleep_status = self.sleep_status.lock();
            if !sleep_status.are_enough_sleeping(percentage) {
                return;
            }
            sleep_status.sleepers_needed(percentage)
        };

        let mut deep_sleepers = 0;
        self.players.iter_players(|_, player| {
            if player.is_sleeping_long_enough() {
                deep_sleepers += 1;
            }
            deep_sleepers < sleepers_needed
        });

        if deep_sleepers < sleepers_needed {
            return;
        }

        if self.get_game_rule(&ADVANCE_TIME) {
            self.move_day_time_to_next_morning();
        }

        self.wake_up_all_players();
        if self.get_game_rule(&ADVANCE_WEATHER) && self.is_raining() {
            self.reset_weather_cycle();
        }
    }

    fn move_day_time_to_next_morning(&self) {
        let Some(clock) = self.dimension_type.default_clock else {
            return;
        };
        let _ = self.move_clock_to_time_marker(clock, &WAKE_UP_FROM_SLEEP_TIME_MARKER);
    }

    fn wake_up_all_players(&self) {
        self.sleep_status.lock().remove_all_sleepers();
        self.players.iter_players(|_, player| {
            if player.is_sleeping() {
                player.stop_sleep_in_bed(false, false);
            }
            true
        });
    }

    fn reset_weather_cycle(&self) {
        let mut level_data = self.level_data.write();
        level_data.set_rain_time(0);
        level_data.set_raining(false);
        level_data.set_thunder_time(0);
        level_data.set_thundering(false);
    }
}
