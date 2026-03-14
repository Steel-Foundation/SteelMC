use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, Half},
    },
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehaviour, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            vegetation_block::{
                double_plant_can_survive, vegetation_can_survive, vegetation_update_shape,
            },
        },
    },
    world::World,
};

/// Behavior for Tall Grass
pub struct TallGrassBlock {
    block: BlockRef,
}

impl TallGrassBlock {
    /// Creates a new Tall Grass Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn large_variant(state: BlockStateId) -> BlockRef {
        if state.get_block() == vanilla_blocks::FERN {
            vanilla_blocks::LARGE_FERN
        } else {
            vanilla_blocks::TALL_GRASS
        }
    }
}

impl BlockBehaviour for TallGrassBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(&context.relative_pos.below()),
            context.world,
            context.relative_pos.below(),
        ) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Vegetation for TallGrassBlock {}

impl Bonemealable for TallGrassBlock {
    fn is_bonemealable(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        double_plant_can_survive(self, Self::large_variant(state).default_state(), world, pos)
            && world.get_block_state(&pos.above()).is_air()
    }

    fn apply_bonemeal(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        let base_state = Self::large_variant(state).default_state();

        let waterlogged_state = state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .map_or(base_state, |waterlogged| {
                base_state.set_value(&BlockStateProperties::WATERLOGGED, waterlogged)
            });

        world.set_block(
            pos,
            waterlogged_state.set_value(&BlockStateProperties::HALF, Half::Bottom),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.set_block(
            pos.above(),
            waterlogged_state.set_value(&BlockStateProperties::HALF, Half::Top),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}
