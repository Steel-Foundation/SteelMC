//! Falling block behavior implementations.
//!
//! All blocks in this module extend vanilla's `FallingBlock` (schedules a tick on placement
//! that spawns a `FallingBlockEntity` when the block below is free).

mod colored_falling_block;
mod concrete_powder_block;

pub use colored_falling_block::{ColoredFallingBlock, SandBlock};
pub use concrete_powder_block::ConcretePowderBlock;

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::entities::falling_block::FallingBlockEntity;
use crate::entity::next_entity_id;
use crate::world::{ScheduledTickAccess, World};

/// Schedules the fall tick (2 ticks, vanilla default).
pub(crate) fn schedule_fall_tick(world: &dyn ScheduledTickAccess, pos: BlockPos, block: BlockRef) {
    world.schedule_block_tick_default(pos, block, 2);
}

/// Spawns a `FallingBlockEntity` for the block at `pos`, applying `FallingEntityConfig`.
pub(crate) fn spawn_falling_entity(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    let id = next_entity_id();
    let entity = FallingBlockEntity::fall(id, world, pos, state);
    let config = BLOCK_BEHAVIORS
        .get_behavior(state.get_block())
        .falling_entity_config();
    if config.hurt_entities {
        entity.set_hurts_entities(config.fall_damage_per_distance, config.fall_damage_max);
    }
    world.add_entity(Arc::new(entity));
}
