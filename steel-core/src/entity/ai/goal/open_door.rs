use super::door_interact::DoorInteractGoal;
use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;

const FORGET_TICKS: i32 = 20;

pub struct OpenDoorGoal {
    door_interact: DoorInteractGoal,
    close_door: bool,
    forget_time: i32,
}

impl OpenDoorGoal {
    #[must_use]
    pub(crate) const fn new(close_door_after: bool) -> Self {
        Self {
            door_interact: DoorInteractGoal::new(),
            close_door: close_door_after,
            forget_time: 0,
        }
    }
}

impl Goal for OpenDoorGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.door_interact.can_use(mob)
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        self.close_door && self.forget_time > 0 && self.door_interact.can_continue_to_use()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.forget_time = FORGET_TICKS;
        self.door_interact.set_open(mob, true);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.door_interact.set_open(mob, false);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.forget_time -= 1;
        self.door_interact.tick(mob);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::entities::PigEntity;

    fn pig() -> PigEntity {
        PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new())
    }

    #[test]
    fn open_door_goal_claims_no_controls_like_vanilla() {
        let goal = OpenDoorGoal::new(true);

        assert_eq!(goal.controls(), GoalControls::EMPTY);
        assert!(goal.requires_update_every_tick());
    }

    #[test]
    fn open_door_goal_continue_requires_close_door_flag() {
        init_test_registry();
        let mut goal = OpenDoorGoal::new(false);
        goal.forget_time = 1;

        assert!(!goal.can_continue_to_use(&pig()));
    }

    #[test]
    fn open_door_goal_uses_vanilla_forget_time() {
        init_test_registry();
        let mut goal = OpenDoorGoal::new(true);
        let mob = pig();

        goal.start(&mob);

        assert_eq!(goal.forget_time, FORGET_TICKS);
        assert!(goal.can_continue_to_use(&mob));

        goal.tick(&mob);

        assert_eq!(goal.forget_time, FORGET_TICKS - 1);
    }
}
