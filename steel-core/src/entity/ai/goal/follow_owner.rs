//! Vanilla `FollowOwnerGoal`.

use glam::DVec3;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
use crate::entity::{PathfinderMob, SharedEntity};
use steel_utils::types::GameType;

const START_DISTANCE: f64 = 10.0;
const STOP_DISTANCE: f64 = 2.0;

pub struct FollowOwnerGoal {
    owner: Option<SharedEntity>,
    speed_modifier: f64,
    time_to_recalc_path: i32,
}

impl FollowOwnerGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            owner: None,
            speed_modifier,
            time_to_recalc_path: 0,
        }
    }

    fn unable_to_move(mob: &dyn PathfinderMob) -> bool {
        mob.has_controlling_passenger() || mob.is_vehicle()
    }

    fn owner_entity(mob: &dyn PathfinderMob) -> Option<SharedEntity> {
        let tamable = mob.as_tamable()?;
        if !tamable.is_tame() || tamable.is_ordered_to_sit() {
            return None;
        }
        let uuid = tamable.owner_uuid()?;
        let world = mob.level()?;
        let owner = world.get_entity_by_uuid(&uuid)?;
        if !owner.is_alive() {
            return None;
        }
        if owner
            .as_player()
            .is_some_and(|player| player.game_mode() == GameType::Spectator)
        {
            return None;
        }
        Some(owner)
    }
}

impl Goal for FollowOwnerGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if Self::unable_to_move(mob) {
            return false;
        }
        let Some(owner) = Self::owner_entity(mob) else {
            return false;
        };
        if mob.position().distance_squared(owner.position()) < START_DISTANCE * START_DISTANCE {
            return false;
        }
        self.owner = Some(owner);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if Self::unable_to_move(mob) {
            return false;
        }
        let Some(owner) = &self.owner else {
            return false;
        };
        if !owner.is_alive() {
            return false;
        }
        let distance_sqr = mob.position().distance_squared(owner.position());
        distance_sqr > STOP_DISTANCE * STOP_DISTANCE
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.time_to_recalc_path = 0;
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.owner = None;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(owner) = &self.owner else {
            return;
        };
        let position = owner.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(position.x, owner.get_eye_y(), position.z),
            DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            DEFAULT_LOOK_X_MAX_ROT_ANGLE,
        );

        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = reduced_tick_delay(10);
        mob.move_to_pos(owner.position(), self.speed_modifier);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_owner_goal_claims_move_and_look() {
        let goal = FollowOwnerGoal::new(1.0);
        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
    }
}
