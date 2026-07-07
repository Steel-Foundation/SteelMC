use glam::DVec3;

use super::reduced_tick_delay;
use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
use crate::entity::ai::goal::selector::{Goal, GoalControls};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{LivingEntity, PathfinderMob, SharedEntity};
use crate::world::World;

const DEFAULT_PROBABILITY: f32 = 0.02;

type LookAtEntitySelector = Box<dyn Fn(&dyn LivingEntity, &World) -> bool + Send + Sync>;

enum LookAtTargetType {
    Player,
    LivingEntity(LookAtEntitySelector),
}

pub struct LookAtPlayerGoal {
    look_at: Option<SharedEntity>,
    look_distance: f64,
    look_time: i32,
    probability: f32,
    only_horizontal: bool,
    controls: GoalControls,
    look_at_type: LookAtTargetType,
    look_at_context: TargetingConditions,
}

impl LookAtPlayerGoal {
    #[must_use]
    pub(crate) fn new(look_distance: f64) -> Self {
        Self::new_with_probability(look_distance, DEFAULT_PROBABILITY)
    }

    #[must_use]
    pub(crate) fn new_with_probability(look_distance: f64, probability: f32) -> Self {
        Self::new_with_probability_and_horizontal(look_distance, probability, false)
    }

    #[must_use]
    pub(crate) fn new_with_probability_and_horizontal(
        look_distance: f64,
        probability: f32,
        only_horizontal: bool,
    ) -> Self {
        Self::new_for_players_with_controls(
            look_distance,
            probability,
            only_horizontal,
            GoalControls::LOOK,
        )
    }

    #[must_use]
    pub(crate) fn new_for_living_entities(
        look_distance: f64,
        probability: f32,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::new_for_living_entities_with_controls(
            look_distance,
            probability,
            false,
            GoalControls::LOOK,
            selector,
        )
    }

    #[must_use]
    pub(super) fn new_for_players_with_controls(
        look_distance: f64,
        probability: f32,
        only_horizontal: bool,
        controls: GoalControls,
    ) -> Self {
        Self {
            look_at: None,
            look_distance,
            look_time: 0,
            probability,
            only_horizontal,
            controls,
            look_at_type: LookAtTargetType::Player,
            look_at_context: TargetingConditions::for_non_combat().range(look_distance),
        }
    }

    #[must_use]
    pub(super) fn new_for_living_entities_with_controls(
        look_distance: f64,
        probability: f32,
        only_horizontal: bool,
        controls: GoalControls,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            look_at: None,
            look_distance,
            look_time: 0,
            probability,
            only_horizontal,
            controls,
            look_at_type: LookAtTargetType::LivingEntity(Box::new(selector)),
            look_at_context: TargetingConditions::for_non_combat().range(look_distance),
        }
    }
}

impl Goal for LookAtPlayerGoal {
    fn controls(&self) -> GoalControls {
        self.controls
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if rand::random::<f32>() >= self.probability {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };

        let position = mob.position();
        let origin = DVec3::new(position.x, mob.get_eye_y(), position.z);
        self.look_at = match &self.look_at_type {
            LookAtTargetType::Player => world
                .nearest_player(origin, self.look_distance, |player| {
                    !mob.has_indirect_passenger(player)
                        && self.look_at_context.test(world.as_ref(), Some(mob), player)
                })
                .map(|player| -> SharedEntity { player }),
            LookAtTargetType::LivingEntity(selector) => {
                let search_box =
                    mob.bounding_box()
                        .inflate_xyz(self.look_distance, 3.0, self.look_distance);
                world.nearest_entity_in_aabb_matching(&search_box, origin, |entity| {
                    entity.as_living_entity().is_some_and(|living| {
                        selector(living, world.as_ref())
                            && self.look_at_context.test(world.as_ref(), Some(mob), living)
                    })
                })
            }
        };

        self.look_at.is_some()
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(look_at) = &self.look_at else {
            return false;
        };
        if !look_at.is_alive() {
            return false;
        }
        if mob.position().distance_squared(look_at.position())
            > self.look_distance * self.look_distance
        {
            return false;
        }

        self.look_time > 0
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.look_time = reduced_tick_delay(40 + rand::random_range(0..40));
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.look_at = None;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(look_at) = &self.look_at else {
            return;
        };
        if !look_at.is_alive() {
            return;
        }

        let position = look_at.position();
        let target_y = if self.only_horizontal {
            mob.get_eye_y()
        } else {
            look_at.get_eye_y()
        };
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(position.x, target_y, position.z),
            DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            DEFAULT_LOOK_X_MAX_ROT_ANGLE,
        );
        self.look_time -= 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use super::*;
    use crate::entity::entities::PigEntity;
    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    #[test]
    fn look_at_player_goal_claims_only_look_control() {
        let goal = LookAtPlayerGoal::new(6.0);

        assert_eq!(goal.controls(), GoalControls::LOOK);
    }

    #[test]
    fn look_at_player_goal_can_claim_custom_controls_for_vanilla_subclasses() {
        let goal = LookAtPlayerGoal::new_for_players_with_controls(
            6.0,
            1.0,
            false,
            GoalControls::LOOK | GoalControls::MOVE,
        );

        assert_eq!(goal.controls(), GoalControls::LOOK | GoalControls::MOVE);
    }

    #[test]
    fn look_at_player_goal_supports_selector_based_living_targets() {
        let goal = LookAtPlayerGoal::new_for_living_entities(8.0, 1.0, |living, _| {
            living.entity_type() == &vanilla_entities::PIG
        });

        assert_eq!(goal.controls(), GoalControls::LOOK);
    }

    #[test]
    fn look_at_player_goal_uses_vanilla_adjusted_look_time() {
        init_test_registry();
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let mut goal = LookAtPlayerGoal::new(6.0);

        goal.start(&pig);

        assert!(
            (reduced_tick_delay(40)..=reduced_tick_delay(79)).contains(&goal.look_time),
            "look_time was {}",
            goal.look_time
        );
    }
}
