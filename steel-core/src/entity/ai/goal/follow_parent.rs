use crate::entity::ai::goal::selector::{Goal, GoalControls};
use crate::entity::{PathfinderMob, SharedEntity};

use super::reduced_tick_delay;

const HORIZONTAL_SCAN_RANGE: f64 = 8.0;
const VERTICAL_SCAN_RANGE: f64 = 4.0;
const DONT_FOLLOW_IF_CLOSER_THAN_SQR: f64 = 9.0;
const STOP_FOLLOW_IF_FARTHER_THAN_SQR: f64 = 256.0;

pub struct FollowParentGoal {
    parent: Option<SharedEntity>,
    speed_modifier: f64,
    time_to_recalc_path: i32,
}

impl FollowParentGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            parent: None,
            speed_modifier,
            time_to_recalc_path: 0,
        }
    }
}

impl Goal for FollowParentGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(animal) = mob.as_animal() else {
            return false;
        };
        if animal.get_age() >= 0 {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };
        let search_box = mob.bounding_box().inflate_xyz(
            HORIZONTAL_SCAN_RANGE,
            VERTICAL_SCAN_RANGE,
            HORIZONTAL_SCAN_RANGE,
        );
        let parent = world.nearest_entity_in_aabb_matching(&search_box, mob.position(), |entity| {
            entity.uuid() != mob.uuid()
                && entity.entity_type() == mob.entity_type()
                && entity
                    .as_animal()
                    .is_some_and(|candidate| candidate.get_age() >= 0)
        });
        let Some(parent) = parent else {
            return false;
        };
        if mob.position().distance_squared(parent.position()) < DONT_FOLLOW_IF_CLOSER_THAN_SQR {
            return false;
        }

        self.parent = Some(parent);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(animal) = mob.as_animal() else {
            return false;
        };
        if animal.get_age() >= 0 {
            return false;
        }

        let Some(parent) = &self.parent else {
            return false;
        };
        if !parent.is_alive() {
            return false;
        }

        let distance_sqr = mob.position().distance_squared(parent.position());
        !(distance_sqr < DONT_FOLLOW_IF_CLOSER_THAN_SQR
            || distance_sqr > STOP_FOLLOW_IF_FARTHER_THAN_SQR)
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.time_to_recalc_path = 0;
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.parent = None;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = reduced_tick_delay(10);

        let Some(parent) = &self.parent else {
            return;
        };
        mob.move_to_pos(parent.position(), self.speed_modifier);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_parent_goal_claims_no_controls_like_vanilla() {
        let goal = FollowParentGoal::new(1.1);

        assert_eq!(goal.controls(), GoalControls::EMPTY);
    }
}
