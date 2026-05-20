use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    TaggedRegistryExt,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt as _,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            default_surviving_state,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for azalea blocks.
#[block_behavior]
pub struct AzaleaBlock {
    block: BlockRef,
}

impl AzaleaBlock {
    /// Creates a new azalea block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for AzaleaBlock {
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

impl Vegetation for AzaleaBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        steel_registry::REGISTRY
            .blocks
            .is_in_tag(state.get_block(), &vanilla_block_tags::SUPPORTS_AZALEA_TAG)
    }
}

impl Bonemealable for AzaleaBlock {
    fn is_bonemealable(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        world
            .get_block_state(pos.above())
            .get_fluid_state()
            .is_empty()
    }

    fn random_success(&self) -> bool {
        rand::random_bool(0.45f64)
    }

    fn apply_bonemeal(&self, _state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) {
        // TODO: grow azalea tree
    }
}
