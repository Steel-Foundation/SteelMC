use super::BlockRef;
use crate::behavior::BlockBehavior;
use crate::behavior::BlockPlaceContext;
use crate::behavior::blocks::vegetation::segmentable_block::{
    segmentable_can_be_replaced, segmentable_get_state_for_placement,
};
use crate::world::{LevelReader, ScheduledTickAccess};
use steel_macros::block_behavior;
use steel_registry::blocks::{
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, IntProperty},
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::vegetation_block::survival_update_shape;

const SEGMENT_PROPERTY: IntProperty = BlockStateProperties::SEGMENT_AMOUNT;

/// Vanilla `LeafLitterBlock` uses sturdy top-face support, not the vegetation tag.
#[block_behavior]
pub struct LeafLitterBlock {
    block: BlockRef,
}

impl LeafLitterBlock {
    /// Creates a new leaf-litter block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for LeafLitterBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        world
            .get_block_state(below_pos)
            .is_face_sturdy_at(below_pos, Direction::Up)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(segmentable_get_state_for_placement(
            self.block,
            &SEGMENT_PROPERTY,
            context,
        ))
    }

    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        segmentable_can_be_replaced(&SEGMENT_PROPERTY, state, context)
    }

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
}
