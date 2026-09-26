//! Normal mob-spawner block behavior.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockPlaceContext};
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::world::World;

/// Behavior for normal mob spawner blocks.
#[block_behavior]
pub struct SpawnerBlock {
    block: BlockRef,
}

impl SpawnerBlock {
    /// Creates a spawner block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SpawnerBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::MOB_SPAWNER,
            level,
            pos,
            state,
        ))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::MOB_SPAWNER,
        )
    }

    fn trigger_event(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        event: i32,
        data: i32,
    ) -> bool {
        world
            .get_block_entity(pos)
            .is_some_and(|block_entity| block_entity.trigger_event(event, data))
    }

    fn spawn_after_break(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _tool: &ItemStack,
        drop_experience: bool,
    ) {
        if drop_experience {
            world.pop_experience(
                pos,
                15 + rand::random_range(0..15) + rand::random_range(0..15),
            );
        }
    }
}
