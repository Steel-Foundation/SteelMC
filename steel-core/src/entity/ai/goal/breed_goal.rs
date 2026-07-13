use glam::DVec3;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{PathfinderMob, SharedEntity};

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

    fn get_free_partner(mob: &mut dyn PathfinderMob) -> Option<SharedEntity> {
        let world = mob.level()?;
        let search_box = mob.bounding_box().inflate(PARTNER_SEARCH_RANGE);
        let partner_targeting = TargetingConditions::for_non_combat()
            .range(PARTNER_SEARCH_RANGE)
            .ignore_line_of_sight();

        world.nearest_entity_in_aabb_matching(&search_box, mob.position(), mob.id(), |entity| {
            let Some(candidate) = entity.as_animal() else {
                return false;
            };
            if !partner_targeting.test(world.as_ref(), Some(mob), candidate) {
                return false;
            }
            if !mob.as_animal().unwrap().can_mate(candidate) {
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

    fn can_use(&mut self, mob: &mut dyn PathfinderMob) -> bool {
        let Some(animal) = mob.as_animal() else {
            return false;
        };
        if !animal.is_in_love() {
            return false;
        }

        self.partner = Self::get_free_partner(mob);
        self.partner.is_some()
    }

    fn can_continue_to_use(&mut self, _mob: &mut dyn PathfinderMob) -> bool {
        let Some(partner) = &self.partner else {
            return false;
        };
        if !partner.with_entity(|e| e.is_alive()) {
            return false;
        }
        if self.love_time >= BREED_TIME {
            return false;
        }
        if partner
            .with_pathfinder(|partner_mob| partner_mob.is_panicking())
            .unwrap_or(false)
        {
            return false;
        }

        partner
            .with_animal(|partner_animal| partner_animal.is_in_love())
            .unwrap_or(false)
    }

    fn stop(&mut self, _mob: &mut dyn PathfinderMob) {
        self.partner = None;
        self.love_time = 0;
    }

    fn tick(&mut self, mob: &mut dyn PathfinderMob) {
        let Some(partner) = &self.partner else {
            return;
        };
        if mob.as_animal_mut().is_none() {
            return;
        };
        if partner.with_animal(|_| ()).is_none() {
            return;
        }

        let partner_position = partner.position();
        let partner_eye_y = partner.with_entity(|e| e.get_eye_y());
        let max_head_x_rot = mob.max_head_x_rot();
        mob.mob_base().controls.look_control.set_look_at(
            DVec3::new(partner_position.x, partner_eye_y, partner_position.z),
            10.0,
            max_head_x_rot,
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
        let Some(animal) = mob.as_animal_mut() else {
            return;
        };
        partner
            .with_animal(|partner_animal| animal.spawn_child_from_breeding(&world, partner_animal));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::entity::entities::Pig;

    #[test]
    fn breed_goal_uses_move_and_look_controls() {
        let goal = BreedGoal::new(1.0);

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
    }

    #[test]
    fn breed_goal_requires_love_mode() {
        init_test_registry();
        let mut pig = Pig::create(1, DVec3::ZERO, Weak::new());
        let mut goal = BreedGoal::new(1.0);

        assert!(!goal.can_use(&mut pig));
    }
}
