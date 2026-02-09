//! Barrel block behavior implementation.
//!
//! Opens a 27-slot container menu when right-clicked.

use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
use crate::portal::portal_shape::PortalShape;
use crate::world::World;

/// Behavior for barrel blocks.
///
/// Barrels are container block entities with 27 slots (3x9 grid).
/// They use the same menu as chests but cannot form double containers.
pub struct FireBlock {
    block: BlockRef,
}

impl FireBlock {
    /// Creates a new barrel block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for FireBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if let Some(shape) = PortalShape::find_portal_shape(world, pos) {
            shape.place_portal_blocks(world);
            // TODO: Play ignite sound, damage item
        } else {
        }
    }
}
