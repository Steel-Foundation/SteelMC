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
        blocks::{
            DoublePlantBlock,
            vegetation::{
                Vegetation,
                bonemealable::Bonemealable,
                default_surviving_state,
                vegetation_block::{survival_update_shape, vegetation_can_survive},
            },
        },
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for short grass and fern blocks.
#[block_behavior]
pub struct TallGrassBlock {
    base: DoublePlantBlock,
}

impl TallGrassBlock {
    /// Creates a new tall grass behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: DoublePlantBlock::new(block),
        }
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
        survival_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.base.block, self, context)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Vegetation for TallGrassBlock {}

impl Bonemealable for TallGrassBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let above_pos = pos.above();
        let lower_state = Self::large_variant(state).default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        !world.is_outside_build_height(above_pos.y())
            && self.base.can_survive(lower_state, world, pos)
            && world.get_block_state(above_pos).is_air()
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        DoublePlantBlock::place_at(
            world,
            Self::large_variant(state).default_state(),
            pos,
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use crate::test_support::TestLevel;

    use super::*;

    #[test]
    fn tall_grass_bonemeal_rejects_top_build_height() {
        init_vanilla_registry();
        let behavior = TallGrassBlock::new(&vanilla_blocks::SHORT_GRASS);
        let state = vanilla_blocks::SHORT_GRASS.default_state();
        let level = TestLevel::default()
            .with_min_y(0)
            .with_height(1)
            .with_block(BlockPos::ZERO.below(), vanilla_blocks::DIRT.default_state());

        assert!(!behavior.is_valid_bonemeal_target(state, &level, BlockPos::ZERO));
    }
}
