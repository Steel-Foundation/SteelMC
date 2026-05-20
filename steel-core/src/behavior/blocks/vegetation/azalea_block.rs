use steel_macros::block_behavior;
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
use steel_registry::{
    TaggedRegistryExt,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags,
};
use steel_utils::{BlockPos, BlockStateId, Direction};
<<<<<<< HEAD

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation, default_surviving_state,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    world::{LevelReader, ScheduledTickAccess},
};

/// Behavior for azalea blocks.
=======
use steel_registry::vanilla_block_tags;
use steel_utils::{BlockPos, BlockStateId};
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation, default_surviving_state,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    world::{LevelReader, ScheduledTickAccess},
};

<<<<<<< HEAD
use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Vanilla `AzaleaBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
/// Behavior for azalea blocks.
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
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
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
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
<<<<<<< HEAD
=======
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &vanilla_block_tags::SUPPORTS_AZALEA_TAG)
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))

impl Vegetation for AzaleaBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        steel_registry::REGISTRY
            .blocks
            .is_in_tag(state.get_block(), &vanilla_block_tags::SUPPORTS_AZALEA_TAG)
    }
}
<<<<<<< HEAD
=======
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
