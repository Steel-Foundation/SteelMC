use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{TaggedRegistryExt as _, vanilla_blocks, vanilla_entity_type_tags};

use super::selector::{Goal, GoalControls};
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::entity::PathfinderMob;

pub struct ClimbOnTopOfPowderSnowGoal;

impl ClimbOnTopOfPowderSnowGoal {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }

    fn is_powder_snow_walkable_entity(mob: &dyn PathfinderMob) -> bool {
        steel_registry::REGISTRY.entity_types.is_in_tag(
            mob.entity_type(),
            &vanilla_entity_type_tags::EntityTypeTag::POWDER_SNOW_WALKABLE_MOBS,
        )
    }
}

impl Goal for ClimbOnTopOfPowderSnowGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::JUMP
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let in_powder_snow = mob.was_in_powder_snow() || mob.is_in_powder_snow();
        if !in_powder_snow || !Self::is_powder_snow_walkable_entity(mob) {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };

        let above = mob.block_position().above();
        let above_state = world.get_block_state(above);
        if above_state.get_block() == &vanilla_blocks::POWDER_SNOW {
            return true;
        }

        BLOCK_BEHAVIORS
            .get_behavior(above_state.get_block())
            .get_collision_shape(
                above_state,
                world.as_ref(),
                above,
                BlockCollisionContext::empty(),
            )
            .is_empty()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        mob.mob_base().controls().lock().jump_control.jump();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::entities::PigEntity;
    use crate::entity::{Entity as _, InsideBlockEffectType, Mob as _};

    fn pig() -> PigEntity {
        PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new())
    }

    #[test]
    fn climb_on_top_of_powder_snow_goal_uses_jump_control() {
        let goal = ClimbOnTopOfPowderSnowGoal::new();

        assert_eq!(goal.controls(), GoalControls::JUMP);
        assert!(goal.requires_update_every_tick());
    }

    #[test]
    fn climb_on_top_of_powder_snow_goal_requires_powder_snow_contact() {
        init_test_registry();
        let mut goal = ClimbOnTopOfPowderSnowGoal::new();
        let mob = pig();

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn climb_on_top_of_powder_snow_goal_uses_entity_type_tag_not_equipment_walkability() {
        init_test_registry();
        let mut goal = ClimbOnTopOfPowderSnowGoal::new();
        let mob = pig();
        mob.apply_inside_block_effect(InsideBlockEffectType::Freeze);

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn climb_on_top_of_powder_snow_goal_requires_world_after_tag_and_contact() {
        init_test_registry();
        let mut goal = ClimbOnTopOfPowderSnowGoal::new();
        let mob = PigEntity::new(&vanilla_entities::RABBIT, 1, DVec3::ZERO, Weak::new());
        mob.apply_inside_block_effect(InsideBlockEffectType::Freeze);

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn climb_on_top_of_powder_snow_goal_ticks_jump_control() {
        init_test_registry();
        let mut goal = ClimbOnTopOfPowderSnowGoal::new();
        let mob = pig();

        goal.tick(&mob);

        assert!(mob.mob_base().controls().lock().jump_control.tick());
    }
}
