use glam::DVec3;

use super::random_pos::default_random_pos;
use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;

const RANDOM_STROLL_DEFAULT_INTERVAL: i32 = 120;

pub struct RandomStrollGoal {
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
    interval: i32,
    force_trigger: bool,
    check_no_action_time: bool,
}

impl RandomStrollGoal {
    #[must_use]
    pub const fn new(speed_modifier: f64) -> Self {
        Self::with_interval(speed_modifier, RANDOM_STROLL_DEFAULT_INTERVAL)
    }

    #[must_use]
    pub const fn with_interval(speed_modifier: f64, interval: i32) -> Self {
        Self::with_interval_and_no_action_time_check(speed_modifier, interval, true)
    }

    #[must_use]
    pub const fn with_interval_and_no_action_time_check(
        speed_modifier: f64,
        interval: i32,
        check_no_action_time: bool,
    ) -> Self {
        Self {
            wanted_position: None,
            speed_modifier,
            interval,
            force_trigger: false,
            check_no_action_time,
        }
    }

    pub const fn trigger(&mut self) {
        self.force_trigger = true;
    }

    pub const fn set_interval(&mut self, interval: i32) {
        self.interval = interval;
    }

    pub(super) fn can_use_with_position(
        &mut self,
        mob: &dyn PathfinderMob,
        mut get_position: impl FnMut(&dyn PathfinderMob) -> Option<DVec3>,
    ) -> bool {
        if mob.has_controlling_passenger() {
            return false;
        }

        if !self.force_trigger {
            if self.check_no_action_time && mob.no_action_time() >= 100 {
                return false;
            }

            let should_skip = rand::random_range(0..reduced_tick_delay(self.interval)) != 0;
            if should_skip {
                return false;
            }
        }

        let Some(position) = get_position(mob) else {
            return false;
        };

        self.wanted_position = Some(position);
        self.force_trigger = false;
        true
    }
}

impl Goal for RandomStrollGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.can_use_with_position(mob, |mob| default_random_pos(mob, 10, 7))
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.mob_base().navigation().lock().is_done() && !mob.has_controlling_passenger()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(wanted_position) = self.wanted_position {
            mob.move_to_pos(wanted_position, self.speed_modifier);
        }
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        mob.mob_base().navigation().lock().stop();
    }
}
