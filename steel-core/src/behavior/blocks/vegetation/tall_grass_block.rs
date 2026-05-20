use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, DoubleBlockHalf},
    },
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            default_surviving_state,
            vegetation_block::{
                double_plant_can_survive, vegetation_can_survive, vegetation_update_shape,
            },
        },
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for short grass and fern blocks.
#[block_behavior]
pub struct TallGrassBlock {
    block: BlockRef,
}

impl TallGrassBlock {
    /// Creates a new tall grass behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn large_variant(state: BlockStateId) -> BlockRef {
        if state.get_block() == &vanilla_blocks::FERN {
            &vanilla_blocks::LARGE_FERN
        } else {
            &vanilla_blocks::TALL_GRASS
        }
    }
}

impl BlockBehavior for TallGrassBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Vegetation for TallGrassBlock {}

impl Bonemealable for TallGrassBlock {
    fn is_bonemealable(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        let lower_state = Self::large_variant(state).default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        double_plant_can_survive(self, lower_state, world, pos)
            && world.get_block_state(pos.above()).is_air()
    }

    fn apply_bonemeal(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let base_state = Self::large_variant(state).default_state();
        let waterlogged_state = state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .map_or(base_state, |waterlogged| {
                base_state.set_value(&BlockStateProperties::WATERLOGGED, waterlogged)
            });

        world.set_block(
            pos,
            waterlogged_state.set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Lower,
            ),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.set_block(
            pos.above(),
            waterlogged_state.set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            ),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}
