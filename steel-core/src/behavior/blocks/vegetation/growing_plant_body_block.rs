use rand::rng;
use std::sync::Arc;
use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        block::default_can_be_replaced,
        blocks::vegetation::{
            growing_plant_can_survive, growing_plant_head_block::GrowingPlantHeadBlock,
        },
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Shared behavior for growing plant blocks.
pub struct GrowingPlantBodyBlock {
    block: BlockRef,
    growth_direction: Direction,
    schedule_fluid_ticks: bool,
    head_block: BlockRef,
    update_head_after_converted_from_body: fn(BlockStateId, BlockStateId) -> BlockStateId,
}

impl GrowingPlantBodyBlock {
    /// Creates a new growing plant body behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        growth_direction: Direction,
        schedule_fluid_ticks: bool,
        head_block: BlockRef,
    ) -> Self {
        Self {
            block,
            growth_direction,
            schedule_fluid_ticks,
            head_block,
            update_head_after_converted_from_body: Self::unchanged_converted_state,
        }
    }

    /// Configures the vanilla `updateHeadAfterConvertedFromBody` specialization.
    #[must_use]
    pub const fn with_update_head_after_converted_from_body(
        mut self,
        update: fn(BlockStateId, BlockStateId) -> BlockStateId,
    ) -> Self {
        self.update_head_after_converted_from_body = update;
        self
    }

    const fn unchanged_converted_state(
        _body_state: BlockStateId,
        head_state: BlockStateId,
    ) -> BlockStateId {
        head_state
    }
}

impl BlockBehavior for GrowingPlantBodyBlock {
    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        let default_result = default_can_be_replaced(state, context);
        if !default_result {
            return false;
        }

        context.with_item(|item| item.item() != REGISTRY.items.by_block(self.head_block))
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            self.growth_direction,
            self.head_block,
            state.get_block(),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == self.growth_direction.opposite() && !self.can_survive(state, world, pos) {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        let head_block = self.head_block;
        if direction == self.growth_direction
            && neighbor_state.get_block() != self.block
            && neighbor_state.get_block() != head_block
        {
            let mut rng = rng();
            return (self.update_head_after_converted_from_body)(
                state,
                GrowingPlantHeadBlock::get_head_state(self.head_block, &mut rng),
            );
        }
        if self.schedule_fluid_ticks {
            world.schedule_fluid_tick_default(
                pos,
                &vanilla_fluids::WATER,
                vanilla_fluids::WATER.tick_delay as i32,
            );
        }
        state
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::{
        item_stack::ItemStack, test_support::init_test_registry, vanilla_blocks, vanilla_items,
    };
    use steel_utils::{BlockPos, types::InteractionHand};

    use super::*;
    use crate::{
        behavior::{BlockHitResult, PlacementOrientation, PlacementSource, init_behaviors},
        test_support::test_world,
    };

    fn place_context(item_in_hand: &mut ItemStack) -> BlockPlaceContext<'_> {
        let hit_result = BlockHitResult {
            location: DVec3::ZERO,
            direction: Direction::Down,
            block_pos: BlockPos::ZERO,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            item_in_hand,
            PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 0.0,
            },
            false,
        );
        BlockPlaceContext::new(test_world(), source, &hit_result)
    }

    #[test]
    fn replacement_rejects_head_item_and_preserves_default_result() {
        init_test_registry();
        init_behaviors();

        let behavior = GrowingPlantBodyBlock::new(
            &vanilla_blocks::CAVE_VINES_PLANT,
            Direction::Down,
            false,
            &vanilla_blocks::CAVE_VINES,
        );
        let mut glow_berries = ItemStack::new(&vanilla_items::GLOW_BERRIES);
        let context = place_context(&mut glow_berries);
        let replaceable_state = vanilla_blocks::SHORT_GRASS.default_state();
        assert!(default_can_be_replaced(replaceable_state, &context));
        assert!(!behavior.can_be_replaced(replaceable_state, &context));

        let mut stone = ItemStack::new(&vanilla_items::STONE);
        let context = place_context(&mut stone);
        let body_state = vanilla_blocks::CAVE_VINES_PLANT.default_state();
        assert_eq!(
            behavior.can_be_replaced(body_state, &context),
            default_can_be_replaced(body_state, &context)
        );
    }
}
