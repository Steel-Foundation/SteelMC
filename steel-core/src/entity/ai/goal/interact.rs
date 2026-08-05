use super::look_at_player::LookAtPlayerGoal;
use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;

pub struct InteractGoal {
    look_at: LookAtPlayerGoal,
}

impl InteractGoal {
    #[must_use]
    pub(crate) fn new_player(look_distance: f64, probability: f32) -> Self {
        Self {
            look_at: LookAtPlayerGoal::new_for_players_with_controls(
                look_distance,
                probability,
                false,
                GoalControls::LOOK | GoalControls::MOVE,
            ),
        }
    }
}

impl Goal for InteractGoal {
    fn controls(&self) -> GoalControls {
        self.look_at.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.look_at.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.look_at.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.look_at.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.look_at.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.look_at.tick(mob);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interact_goal_claims_look_and_move_controls_like_vanilla() {
        let goal = InteractGoal::new_player(3.0, 1.0);

        assert_eq!(goal.controls(), GoalControls::LOOK | GoalControls::MOVE);
    }
}
