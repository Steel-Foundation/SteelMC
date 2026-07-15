//! Beehive block behavior implementation.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::world::World;

/// Behavior for beehive and bee nest blocks.
// TODO: Implement full vanilla beehive interactions, bee release, smoke/fire handling, loot/data components, and ticking.
#[block_behavior]
pub struct BeehiveBlock {
    block: BlockRef,
}

impl BeehiveBlock {
    /// Creates a new beehive block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BeehiveBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction().opposite(),
        ))
    }

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        BLOCK_ENTITIES.create(&vanilla_block_entity_types::BEEHIVE, level, pos, state)
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
    ) -> i32 {
        state.get_value(&BlockStateProperties::LEVEL_HONEY).into()
    }
}
