use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _, shapes::VoxelShape};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::{vanilla_entities, vanilla_game_rules};
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockCollisionContext, BlockPlaceContext},
    entity::{Entity, InsideBlockEffectCollector, InsideBlockEffectType},
    world::{LevelReader, World},
};

const IN_BLOCK_SPEED_MULTIPLIER: DVec3 = DVec3::new(0.9, 1.5, 0.9);
const NUM_BLOCKS_TO_FALL_INTO_BLOCK: f64 = 2.5;
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
}

impl BlockBehavior for PowderSnowBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
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
                .with_can_walk_on_powder_snow(entity.can_walk_on_powder_snow()),
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
        if context.can_walk_on_powder_snow()
            && context.is_above(VoxelShape::FULL_BLOCK, pos, false)
            && !context.is_descending()
        {
            return self.default_get_collision_shape(state, world, pos, context);
        }

        VoxelShape::EMPTY
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if !entity.is_living_entity() || entity.in_block_state(world).get_block() == self.block {
            entity.make_stuck_in_block(IN_BLOCK_SPEED_MULTIPLIER);
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

    use steel_registry::{test_support, vanilla_blocks};

    struct EmptyLevel;

    impl LevelReader for EmptyLevel {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            vanilla_blocks::AIR.default_state()
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            0
        }

        fn height(&self) -> i32 {
            384
        }
    }

    fn powder_snow() -> PowderSnowBlock {
        PowderSnowBlock::new(&vanilla_blocks::POWDER_SNOW)
    }

    fn powder_snow_state() -> BlockStateId {
        vanilla_blocks::POWDER_SNOW.default_state()
    }

    #[test]
    fn falling_entities_collide_with_lower_powder_snow_shape() {
        test_support::init_test_registry();
        let behavior = powder_snow();
        let state = powder_snow_state();
        let pos = BlockPos::new(0, 64, 0);

        let shape = behavior.get_collision_shape(
            state,
            &EmptyLevel,
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

        let shape = behavior.get_collision_shape(state, &EmptyLevel, pos, context);

        assert_eq!(
            shape,
            behavior.default_get_collision_shape(state, &EmptyLevel, pos, context)
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
            &EmptyLevel,
            pos,
            BlockCollisionContext::entity(65.0, false),
        );
        let descending_shape = behavior.get_collision_shape(
            state,
            &EmptyLevel,
            pos,
            BlockCollisionContext::entity(65.0, true).with_can_walk_on_powder_snow(true),
        );

        assert_eq!(non_walkable_shape, VoxelShape::EMPTY);
        assert_eq!(descending_shape, VoxelShape::EMPTY);
    }

    #[test]
    fn placement_context_has_no_powder_snow_collision() {
        test_support::init_test_registry();
        let behavior = powder_snow();
        let state = powder_snow_state();
        let pos = BlockPos::new(0, 64, 0);

        let shape = behavior.get_collision_shape(
            state,
            &EmptyLevel,
            pos,
            BlockCollisionContext::pre_move(65.0, false)
                .with_can_walk_on_powder_snow(true)
                .with_fall_distance(NUM_BLOCKS_TO_FALL_INTO_BLOCK + 0.01),
        );

        assert_eq!(shape, VoxelShape::EMPTY);
    }
}
