use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _, shapes::VoxelShape};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{vanilla_entities, vanilla_game_rules};
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId};

use crate::{
    behavior::{
        BlockBehavior, BlockCollisionContext, BlockPlaceContext, EntityFallDamage,
        EntityFallOnContext,
    },
    entity::ai::path::PathComputationType,
    entity::{Entity, InsideBlockEffectCollector, InsideBlockEffectType},
    world::{LevelReader, World},
};

const IN_BLOCK_SPEED_MULTIPLIER: DVec3 = DVec3::new(0.9, 1.5, 0.9);
const NUM_BLOCKS_TO_FALL_INTO_BLOCK: f64 = 2.5;
const MIN_FALL_DISTANCE_FOR_SOUND: f64 = 4.0;
const MIN_FALL_DISTANCE_FOR_BIG_SOUND: f64 = 7.0;
const FALLING_COLLISION_BOXES: &[BlockLocalAabb] =
    &[BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.9, 1.0)];
const FALLING_COLLISION_SHAPE: VoxelShape = VoxelShape::from_boxes(FALLING_COLLISION_BOXES);

/// Behavior for powder snow blocks.
///
/// Vanilla handles several powder-snow collision variants in one class; Steel
/// keeps those pieces here so the movement pipeline has a single block-behavior
/// entry point for powder snow.
#[block_behavior]
pub struct PowderSnowBlock {
    block: BlockRef,
}

impl PowderSnowBlock {
    /// Creates a powder snow block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    fn fall_sound(context: EntityFallOnContext<'_>) -> Option<SoundEventRef> {
        if context.fall_distance < MIN_FALL_DISTANCE_FOR_SOUND || !context.entity.is_living_entity {
            return None;
        }

        let (small, big) = context.entity.fall_sounds;
        Some(if context.fall_distance < MIN_FALL_DISTANCE_FOR_BIG_SOUND {
            small
        } else {
            big
        })
    }
}

impl BlockBehavior for PowderSnowBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        true
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        if let Some(sound) = Self::fall_sound(context)
            && let Some(entity) = context.source_entity()
        {
            entity.play_sound(sound, 1.0, 1.0);
        }

        None
    }

    fn get_entity_inside_collision_shape(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        entity: &dyn Entity,
    ) -> VoxelShape {
        let collision_shape = self.get_collision_shape(
            state,
            world,
            pos,
            BlockCollisionContext::entity(entity.position().y, entity.is_descending())
                .with_fall_distance(entity.fall_distance())
                .with_can_walk_on_powder_snow(entity.can_walk_on_powder_snow())
                .with_falling_block(entity.entity_type() == &vanilla_entities::FALLING_BLOCK),
        );
        if collision_shape.is_empty() {
            self.default_get_entity_inside_collision_shape(state, world, pos, entity)
        } else {
            collision_shape
        }
    }

    fn get_collision_shape(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        context: BlockCollisionContext,
    ) -> VoxelShape {
        if context.is_placement() {
            return VoxelShape::EMPTY;
        }
        if context.fall_distance() > NUM_BLOCKS_TO_FALL_INTO_BLOCK {
            return FALLING_COLLISION_SHAPE;
        }
        if context.is_falling_block()
            || (context.can_walk_on_powder_snow()
                && context.is_above(VoxelShape::FULL_BLOCK, pos, false)
                && !context.is_descending())
        {
            return self.default_get_collision_shape(state, world, pos, context);
        }

        VoxelShape::EMPTY
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if !entity.is_living_entity() || entity.in_block_state(world).get_block() == self.block {
            entity.make_stuck_in_block(state, IN_BLOCK_SPEED_MULTIPLIER);
        }

        let world = Arc::clone(world);
        effect_collector.run_before(
            InsideBlockEffectType::Extinguish,
            Box::new(move |entity| {
                if !entity.is_on_fire() {
                    return;
                }

                let mob_griefing = world.get_game_rule(&vanilla_game_rules::MOB_GRIEFING)
                    == GameRuleValue::Bool(true);
                if (mob_griefing || entity.entity_type() == &vanilla_entities::PLAYER)
                    && entity.may_interact(world.as_ref(), pos)
                {
                    world.destroy_block(pos, false);
                }
            }),
        );
        effect_collector.apply(InsideBlockEffectType::Freeze);
        effect_collector.apply(InsideBlockEffectType::Extinguish);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use steel_registry::{sound_events, test_support, vanilla_blocks, vanilla_entities};

    use crate::behavior::EntityFallOnFacts;
    use crate::test_support::TestLevel;

    fn empty_level() -> TestLevel {
        TestLevel::default().with_min_y(0)
    }

    fn powder_snow() -> PowderSnowBlock {
        PowderSnowBlock::new(&vanilla_blocks::POWDER_SNOW)
    }

    fn powder_snow_state() -> BlockStateId {
        vanilla_blocks::POWDER_SNOW.default_state()
    }

    fn fall_context(fall_distance: f64, is_living_entity: bool) -> EntityFallOnContext<'static> {
        EntityFallOnContext::new(
            fall_distance,
            false,
            EntityFallOnFacts::new(
                &vanilla_entities::PLAYER,
                is_living_entity,
                0.6,
                1.8,
                (
                    &sound_events::ENTITY_PLAYER_SMALL_FALL,
                    &sound_events::ENTITY_PLAYER_BIG_FALL,
                ),
            ),
            None,
        )
    }

    #[test]
    fn powder_snow_fall_sound_uses_vanilla_living_thresholds() {
        test_support::init_test_registry();
        assert!(PowderSnowBlock::fall_sound(fall_context(3.99, true)).is_none());
        assert_eq!(
            PowderSnowBlock::fall_sound(fall_context(4.0, true)),
            Some(&sound_events::ENTITY_PLAYER_SMALL_FALL)
        );
        assert_eq!(
            PowderSnowBlock::fall_sound(fall_context(7.0, true)),
            Some(&sound_events::ENTITY_PLAYER_BIG_FALL)
        );
        assert!(PowderSnowBlock::fall_sound(fall_context(7.0, false)).is_none());
    }

    #[test]
    fn falling_entities_collide_with_lower_powder_snow_shape() {
        test_support::init_test_registry();
        let behavior = powder_snow();
        let state = powder_snow_state();
        let pos = BlockPos::new(0, 64, 0);

        let shape = behavior.get_collision_shape(
            state,
            &empty_level(),
            pos,
            BlockCollisionContext::entity(64.0, false)
                .with_fall_distance(NUM_BLOCKS_TO_FALL_INTO_BLOCK + 0.01),
        );

        assert_eq!(shape, FALLING_COLLISION_SHAPE);
    }

    #[test]
    fn walkable_entities_use_default_powder_snow_collision_shape_when_above() {
        test_support::init_test_registry();
        let behavior = powder_snow();
        let state = powder_snow_state();
        let pos = BlockPos::new(0, 64, 0);
        let context = BlockCollisionContext::entity(65.0, false).with_can_walk_on_powder_snow(true);

        let shape = behavior.get_collision_shape(state, &empty_level(), pos, context);

        assert_eq!(
            shape,
            behavior.default_get_collision_shape(state, &empty_level(), pos, context)
        );
    }

    #[test]
    fn non_walkable_or_descending_entities_have_no_powder_snow_collision() {
        test_support::init_test_registry();
        let behavior = powder_snow();
        let state = powder_snow_state();
        let pos = BlockPos::new(0, 64, 0);

        let non_walkable_shape = behavior.get_collision_shape(
            state,
            &empty_level(),
            pos,
            BlockCollisionContext::entity(65.0, false),
        );
        let descending_shape = behavior.get_collision_shape(
            state,
            &empty_level(),
            pos,
            BlockCollisionContext::entity(65.0, true).with_can_walk_on_powder_snow(true),
        );

        assert_eq!(non_walkable_shape, VoxelShape::EMPTY);
        assert_eq!(descending_shape, VoxelShape::EMPTY);
    }

    #[test]
    fn falling_blocks_use_default_powder_snow_collision_shape() {
        test_support::init_test_registry();
        let behavior = powder_snow();
        let state = powder_snow_state();
        let pos = BlockPos::new(0, 64, 0);
        let context = BlockCollisionContext::entity(64.0, true).with_falling_block(true);

        let shape = behavior.get_collision_shape(state, &empty_level(), pos, context);

        assert_eq!(
            shape,
            behavior.default_get_collision_shape(state, &empty_level(), pos, context)
        );
    }

    #[test]
    fn placement_context_has_no_powder_snow_collision() {
        test_support::init_test_registry();
        let behavior = powder_snow();
        let state = powder_snow_state();
        let pos = BlockPos::new(0, 64, 0);

        let shape = behavior.get_collision_shape(
            state,
            &empty_level(),
            pos,
            BlockCollisionContext::pre_move(65.0, false)
                .with_can_walk_on_powder_snow(true)
                .with_fall_distance(NUM_BLOCKS_TO_FALL_INTO_BLOCK + 0.01),
        );

        assert_eq!(shape, VoxelShape::EMPTY);
    }
}
