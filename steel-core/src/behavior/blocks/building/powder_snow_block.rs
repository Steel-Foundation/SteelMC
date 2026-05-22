//! Powder snow block behavior impl

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::world::World;

/// Behavior for powder snow
/// TODO: fix fall sound
#[block_behavior]
pub struct PowderSnowBlock {
    block: BlockRef,
}

impl PowderSnowBlock {
    /// Creates a new powder snow block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for PowderSnowBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _entity: &dyn Entity,
        _fall_distance: f32,
    ) {
    }
}
