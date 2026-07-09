use super::BlockRef;
use crate::{
    behavior::{
        BlockBehavior,
        blocks::vegetation::{bonemealable::Bonemealable, segmentable_block::SegmentableBlock},
    },
    world::LevelReader,
};
use steel_macros::block_behavior;
use steel_registry::blocks::{
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, IntProperty},
};
use steel_registry::items::ItemRef;
use steel_registry::items::item::BlockHitResult;
use steel_utils::{BlockPos, BlockStateId, Direction, types::InteractionHand};
use std::sync::Arc;
use crate::behavior::BlockPlaceContext;
use crate::behavior::{InventoryAccess, InteractionResult};
use crate::player::Player;
use crate::world::{World, ScheduledTickAccess};
use rand::prelude::Rng;

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

    fn segment_property(&self) -> &IntProperty {
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
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        self.segmentable_get_state_for_placement(context)
    }

    fn can_be_replaced(
        &self,
        state: BlockStateId,
        held_item: ItemRef,
        is_secondary_use_active: bool,
    ) -> bool {
        self.segmentable_can_be_replaced(state, held_item, is_secondary_use_active)
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        self.segmentable_use_item_on(state, world, pos, player, hand, hit_result, inv)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.segmentable_update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for LeafLitterBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        self.segmentable_is_valid_bonemeal_target(state, world, pos)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.segmentable_perform_bonemeal(state, world, rng, pos);
    }
}
