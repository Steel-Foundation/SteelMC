use super::BlockRef;
use crate::behavior::BlockPlaceContext;
use crate::behavior::blocks::vegetation::segmentable_block::{
    segmentable_can_be_replaced, segmentable_get_state_for_placement,
    segmentable_is_valid_bonemeal_target, segmentable_perform_bonemeal, segmentable_update_shape,
    segmentable_use_item_on,
};
use crate::behavior::{InteractionResult, InventoryAccess};
use crate::player::Player;
use crate::world::{ScheduledTickAccess, World};
use crate::{
    behavior::{BlockBehavior, blocks::vegetation::bonemealable::Bonemealable},
    world::LevelReader,
};
use rand::prelude::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::{
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, IntProperty},
};
use steel_registry::items::ItemRef;
use steel_registry::items::item::BlockHitResult;
use steel_utils::{BlockPos, BlockStateId, Direction, types::InteractionHand};

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

    fn get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        segmentable_get_state_for_placement(self, self.block, &SEGMENT_PROPERTY, context)
    }

    fn can_be_replaced(
        &self,
        state: BlockStateId,
        held_item: ItemRef,
        is_secondary_use_active: bool,
    ) -> bool {
        segmentable_can_be_replaced(&SEGMENT_PROPERTY, state, held_item, is_secondary_use_active)
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        segmentable_use_item_on(&SEGMENT_PROPERTY, state, world, pos, inv)
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
        segmentable_update_shape(self, state, world, pos)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for LeafLitterBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        segmentable_is_valid_bonemeal_target(self)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        segmentable_perform_bonemeal(self.block, &SEGMENT_PROPERTY, state, world, pos);
    }
}
