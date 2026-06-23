use glam::DVec3;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{Animal, PathfinderMob, SharedEntity};

const PARTNER_SEARCH_RANGE: f64 = 8.0;
const BREED_DISTANCE_SQR: f64 = 9.0;
const BREED_TIME: i32 = 60;

pub struct BreedGoal {
    partner: Option<SharedEntity>,
    love_time: i32,
    speed_modifier: f64,
}

impl BreedGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            partner: None,
            love_time: 0,
            speed_modifier,
        }
    }

    fn get_free_partner(mob: &dyn PathfinderMob, animal: &dyn Animal) -> Option<SharedEntity> {
        let world = mob.level()?;
        let search_box = mob.bounding_box().inflate(PARTNER_SEARCH_RANGE);
        let partner_targeting = TargetingConditions::for_non_combat()
            .range(PARTNER_SEARCH_RANGE)
            .ignore_line_of_sight();

        world.nearest_entity_in_aabb_matching(&search_box, mob.position(), |entity| {
            let Some(candidate) = entity.as_animal() else {
                return false;
            };
            if !partner_targeting.test(world.as_ref(), Some(mob), candidate) {
                return false;
            }
            if !animal.can_mate(candidate) {
                return false;
            }

            !entity
                .as_pathfinder_mob()
                .is_some_and(PathfinderMob::is_panicking)
        })
    }
}

impl Goal for BreedGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(animal) = mob.as_animal() else {
            return false;
        };
        if !animal.is_in_love() {
            return false;
        }

        self.partner = Self::get_free_partner(mob, animal);
        self.partner.is_some()
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        let Some(partner) = &self.partner else {
            return false;
        };
        if !partner.is_alive() {
            return false;
        }
        if self.love_time >= BREED_TIME {
            return false;
        }
        if partner
            .as_pathfinder_mob()
            .is_some_and(PathfinderMob::is_panicking)
        {
            return false;
        }

        partner.as_animal().is_some_and(Animal::is_in_love)
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.partner = None;
        self.love_time = 0;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(partner) = &self.partner else {
            return;
        };
        let Some(animal) = mob.as_animal() else {
            return;
        };
        let Some(partner_animal) = partner.as_animal() else {
            return;
        };

        let partner_position = partner.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(partner_position.x, partner.get_eye_y(), partner_position.z),
            10.0,
            mob.max_head_x_rot(),
        );
        mob.move_to_pos(partner_position, self.speed_modifier);

        self.love_time += 1;
        if self.love_time < reduced_tick_delay(BREED_TIME)
            || mob.position().distance_squared(partner_position) >= BREED_DISTANCE_SQR
        {
            return;
        }

        let Some(world) = mob.level() else {
            return;
        };
        animal.spawn_child_from_breeding(&world, partner_animal);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::entities::PigEntity;

    #[test]
    fn breed_goal_uses_move_and_look_controls() {
        let goal = BreedGoal::new(1.0);

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
    }

    #[test]
    fn breed_goal_requires_love_mode() {
        init_test_registry();
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let mut goal = BreedGoal::new(1.0);

        assert!(!goal.can_use(&pig));
    }
}
