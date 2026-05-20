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
            && double_plant_can_survive(self, lower_state, world, pos)
            && world.get_block_state(above_pos).is_air()
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
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

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, Registry};

    use super::*;

    struct OneBlockTallLevel;

    impl LevelReader for OneBlockTallLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == BlockPos::ZERO.below() {
                vanilla_blocks::DIRT.default_state()
            } else {
                vanilla_blocks::AIR.default_state()
            }
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            0
        }

        fn height(&self) -> i32 {
            1
        }
    }

    fn init_registry() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    }

    #[test]
    fn tall_grass_bonemeal_rejects_top_build_height() {
        init_registry();
        let behavior = TallGrassBlock::new(&vanilla_blocks::SHORT_GRASS);
        let state = vanilla_blocks::SHORT_GRASS.default_state();

        assert!(!behavior.is_valid_bonemeal_target(state, &OneBlockTallLevel, BlockPos::ZERO));
    }
}
