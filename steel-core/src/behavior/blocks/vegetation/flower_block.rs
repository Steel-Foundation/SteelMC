use steel_macros::block_behavior;
<<<<<<< HEAD
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId, Direction};

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

/// Behavior for most flower blocks.
=======
use steel_registry::vanilla_block_tags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Vanilla `FlowerBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
#[block_behavior]
pub struct FlowerBlock {
    block: BlockRef,
}

impl FlowerBlock {
<<<<<<< HEAD
    /// Creates a new flower behavior.
=======
    /// Creates a new flower block behavior.
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for FlowerBlock {
<<<<<<< HEAD
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
=======
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &vanilla_block_tags::SUPPORTS_VEGETATION_TAG)
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
<<<<<<< HEAD

impl Vegetation for FlowerBlock {}
=======
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
