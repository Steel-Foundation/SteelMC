use steel_macros::block_behavior;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::items::ItemRef;
use steel_registry::items::item::BlockHitResult;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction, types::InteractionHand};
use std::sync::Arc;
use crate::behavior::BlockPlaceContext;
use crate::behavior::{InventoryAccess, InteractionResult};
use crate::player::Player;
use crate::world::{World, ScheduledTickAccess};
use rand::prelude::Rng;

use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
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

    fn segment_property(&self) -> &IntProperty {
        &BlockStateProperties::FLOWER_AMOUNT
    }
}

impl BlockBehavior for FlowerBedBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_VEGETATION)
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

impl Bonemealable for FlowerBedBlock {
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
