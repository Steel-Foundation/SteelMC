//! fire block behavior implementation.

use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::portal::portal_shape::{PortalShape, nether_portal_config};
use crate::world::World;

/// Behavior for fire blocks.
///
/// Fire burns, makes hot, and hurts
#[block_behavior]
pub struct FireBlock {
    block: BlockRef,
}

impl FireBlock {
    /// Creates a new fire block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for FireBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if let Some(tester) = PortalShape::find_portal_shape(world, pos, &nether_portal_config()) {
            tester.place_portal_blocks(world);
            // TODO: Play ignite sound, damage item
        }
    }
}
