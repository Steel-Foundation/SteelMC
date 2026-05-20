use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, DoubleBlockHalf};
use steel_registry::item_stack::ItemStack;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_block_tags};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::{FluidStateExt, get_fluid_state_from_block};
use crate::player::Player;
use crate::world::{LevelReader, World};

use super::{BlockRef, DoublePlantBlock};

/// Vanilla `SmallDripleafBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct SmallDripleafBlock {
    block: BlockRef,
    double_plant: DoublePlantBlock,
}

impl SmallDripleafBlock {
    /// Creates a new small dripleaf block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            block,
            double_plant: DoublePlantBlock::new(block),
        }
    }
}

impl BlockBehavior for SmallDripleafBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Upper {
            return self.double_plant.can_survive(state, world, pos);
        }

        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        let fluid = get_fluid_state_from_block(world.get_block_state(pos));
        REGISTRY.blocks.is_in_tag(
            below.get_block(),
            &vanilla_block_tags::SUPPORTS_SMALL_DRIPLEAF_TAG,
        ) || (fluid.is_source()
            && fluid.is_water()
            && REGISTRY.blocks.is_in_tag(
                below.get_block(),
                &vanilla_block_tags::SUPPORTS_VEGETATION_TAG,
            ))
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.relative_pos.y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context
            .world
            .get_block_state(context.relative_pos.above())
            .is_replaceable()
        {
            return None;
        }
        let state = self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction.opposite(),
        );
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state.set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            ))
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: Option<&Player>,
        _item_stack: &ItemStack,
    ) {
        let upper_pos = pos.above();
        let upper_state = DoublePlantBlock::copy_waterlogged_from(
            world,
            upper_pos,
            self.block
                .default_state()
                .set_value(
                    &BlockStateProperties::DOUBLE_BLOCK_HALF,
                    DoubleBlockHalf::Upper,
                )
                .set_value(
                    &BlockStateProperties::HORIZONTAL_FACING,
                    state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
                ),
        );
        world.set_block(upper_pos, upper_state, UpdateFlags::UPDATE_ALL);
    }
}
