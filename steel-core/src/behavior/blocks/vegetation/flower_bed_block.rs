use crate::behavior::BlockPlaceContext;
use crate::behavior::blocks::vegetation::segmentable_block::{
    segmentable_can_be_replaced, segmentable_get_state_for_placement,
};
use crate::world::{ScheduledTickAccess, World};
use rand::prelude::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::{
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, IntProperty},
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::world::LevelReader;

use super::{BlockRef, survives_on_tag, vegetation_block::survival_update_shape};

const SEGMENT_PROPERTY: IntProperty = BlockStateProperties::FLOWER_AMOUNT;

/// Vanilla `FlowerBedBlock` survival.
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

impl BlockBehavior for FlowerBedBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_VEGETATION)
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

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for FlowerBedBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let amount = state.get_value(&SEGMENT_PROPERTY);
        if amount < 4 {
            world.set_block(
                pos,
                state.set_value(&SEGMENT_PROPERTY, amount + 1),
                UpdateFlags::UPDATE_CLIENTS,
            );
        } else {
            world.pop_resource(pos, ItemStack::new(REGISTRY.items.by_block(self.block)));
        }
    }
}
