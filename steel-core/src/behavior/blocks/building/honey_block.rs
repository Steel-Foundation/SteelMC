//! Honey block behavior impl

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;

/// Behavior for the honey block
/// TODO: Implement HoneyBlock sliding (sounds, particles, damage) and entityInside movement. These require missing systems for entity-positioned sounds, event broadcasting, and fall-damage handling
#[block_behavior]
pub struct HoneyBlock {
    block: BlockRef,
}

impl HoneyBlock {
    /// Creates a new honey block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for HoneyBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        fall_distance: f32,
    ) {
        entity.cause_fall_damage(
            fall_distance,
            0.2,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        );
    }
}
