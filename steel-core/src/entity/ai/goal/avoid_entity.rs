use steel_utils::BlockPos;

use super::random_pos::default_random_pos_away;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::path::Path;
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{LivingEntity, PathfinderMob, SharedEntity};
use crate::world::World;

pub struct AvoidEntityGoal {
    to_avoid: Option<SharedEntity>,
    path: Option<Path>,
    max_dist: f32,
    walk_speed_modifier: f64,
    sprint_speed_modifier: f64,
    avoid_entity_targeting: TargetingConditions,
}

impl AvoidEntityGoal {
    #[must_use]
    pub(crate) fn new(max_dist: f32, walk_speed_modifier: f64, sprint_speed_modifier: f64) -> Self {
        Self::with_selector(
            max_dist,
            walk_speed_modifier,
            sprint_speed_modifier,
            |target, _| no_creative_or_spectator(target),
        )
    }

    #[must_use]
    pub(crate) fn with_selector(
        max_dist: f32,
        walk_speed_modifier: f64,
        sprint_speed_modifier: f64,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            to_avoid: None,
            path: None,
            max_dist,
            walk_speed_modifier,
            sprint_speed_modifier,
            avoid_entity_targeting: TargetingConditions::for_combat()
                .range(f64::from(max_dist))
                .selector(selector),
        }
    }
}

impl Goal for AvoidEntityGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };

        let search_box =
            mob.bounding_box()
                .inflate_xyz(f64::from(self.max_dist), 3.0, f64::from(self.max_dist));
        let Some(to_avoid) =
            world.nearest_entity_in_aabb_matching(&search_box, mob.position(), |entity| {
                entity.as_living_entity().is_some_and(|living| {
                    self.avoid_entity_targeting
                        .test(world.as_ref(), Some(mob), living)
                })
            })
        else {
            return false;
        };

        let Some(position) = default_random_pos_away(mob, 16, 7, to_avoid.position()) else {
            return false;
        };
        if to_avoid.position().distance_squared(position)
            < to_avoid.position().distance_squared(mob.position())
        {
            return false;
        }

        let path = mob.create_path_to(BlockPos::containing(position.x, position.y, position.z), 0);
        if path.is_none() {
            return false;
        }

        self.to_avoid = Some(to_avoid);
        self.path = path;
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.mob_base().navigation().lock().is_done()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        mob.move_to_path(self.path.take(), self.walk_speed_modifier);
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.to_avoid = None;
        self.path = None;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(to_avoid) = &self.to_avoid else {
            return;
        };

        let speed_modifier = if mob.position().distance_squared(to_avoid.position()) < 49.0 {
            self.sprint_speed_modifier
        } else {
            self.walk_speed_modifier
        };
        mob.mob_base()
            .navigation()
            .lock()
            .set_speed_modifier(speed_modifier);
    }
}

fn no_creative_or_spectator(target: &dyn LivingEntity) -> bool {
    target
        .as_player()
        .is_none_or(|player| !target.is_spectator() && !player.has_infinite_materials())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::{Mob, entities::PigEntity};

    #[test]
    fn avoid_entity_goal_uses_move_control() {
        let goal = AvoidEntityGoal::new(8.0, 1.0, 1.2);

        assert_eq!(goal.controls(), GoalControls::MOVE);
    }

    #[test]
    fn avoid_entity_default_selector_allows_non_player_living_entities() {
        init_test_registry();
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(no_creative_or_spectator(&pig));
    }

    #[test]
    fn avoid_entity_goal_requires_world() {
        init_test_registry();
        let mut goal = AvoidEntityGoal::new(8.0, 1.0, 1.2);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn avoid_entity_goal_sprints_when_close_to_avoided_entity() {
        init_test_registry();
        let mut goal = AvoidEntityGoal::new(8.0, 1.0, 1.2);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        goal.to_avoid = Some(Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(2.0, 0.0, 0.0),
            Weak::new(),
        )));

        goal.tick(&mob);

        assert_eq!(
            mob.mob_base()
                .navigation()
                .lock()
                .speed_modifier()
                .to_bits(),
            1.2_f64.to_bits()
        );
    }

    #[test]
    fn avoid_entity_goal_walks_when_far_from_avoided_entity() {
        init_test_registry();
        let mut goal = AvoidEntityGoal::new(8.0, 1.0, 1.2);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        goal.to_avoid = Some(Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(8.0, 0.0, 0.0),
            Weak::new(),
        )));

        goal.tick(&mob);

        assert_eq!(
            mob.mob_base()
                .navigation()
                .lock()
                .speed_modifier()
                .to_bits(),
            1.0_f64.to_bits()
        );
    }
}
