use steel_macros::block_behavior;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::{
    block::BlockBehavior, blocks::vegetation::segmentable_block::SegmentableBlock,
};
use crate::world::LevelReader;

use super::{BlockRef, survives_on_tag};

/// Vanilla `FlowerBedBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct FlowerBedBlock {
    block: BlockRef,
}

impl FlowerBedBlock {
    /// Creates a new flower-bed block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl SegmentableBlock for FlowerBedBlock {
    fn block_ref(&self) -> &BlockRef {
        &self.block
    }

    fn segment_property(&self) -> &steel_registry::blocks::properties::IntProperty {
        &BlockStateProperties::FLOWER_AMOUNT
    }
}

impl BlockBehavior for FlowerBedBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_VEGETATION)
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
