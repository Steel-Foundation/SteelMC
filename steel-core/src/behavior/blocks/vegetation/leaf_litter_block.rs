use super::BlockRef;
use crate::{
    behavior::{BlockBehavior, blocks::vegetation::segmentable_block::SegmentableBlock},
    world::LevelReader,
};
use steel_macros::block_behavior;
use steel_registry::blocks::{
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, Direction},
};
use steel_utils::{BlockPos, BlockStateId};

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

impl SegmentableBlock for LeafLitterBlock {
    fn block_ref(&self) -> &BlockRef {
        &self.block
    }

    fn segment_property(&self) -> &steel_registry::blocks::properties::IntProperty {
        &BlockStateProperties::SEGMENT_AMOUNT
    }
}

impl BlockBehavior for LeafLitterBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        world
            .get_block_state(below_pos)
            .is_face_sturdy_at(below_pos, Direction::Up)
    }

    fn get_state_for_placement(
        &self,
        context: &crate::behavior::BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        self.segmentable_get_state_for_placement(context)
    }

    fn use_item_on(
        &self,
        state: steel_utils::BlockStateId,
        world: &std::sync::Arc<crate::world::World>,
        pos: steel_utils::BlockPos,
        player: &crate::player::Player,
        hand: steel_utils::types::InteractionHand,
        hit_result: &steel_registry::items::item::BlockHitResult,
        inv: &mut crate::behavior::InventoryAccess,
    ) -> crate::behavior::InteractionResult {
        self.segmentable_use_item_on(state, world, pos, player, hand, hit_result, inv)
    }

    fn update_shape(
        &self,
        state: steel_utils::BlockStateId,
        world: &dyn crate::world::ScheduledTickAccess,
        pos: steel_utils::BlockPos,
        direction: Direction,
        neighbor_pos: steel_utils::BlockPos,
        neighbor_state: steel_utils::BlockStateId,
    ) -> steel_utils::BlockStateId {
        self.segmentable_update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }
}
