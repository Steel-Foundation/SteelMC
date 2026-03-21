//! Fire block behavior implementation.
//!
//! Vanilla splits fire into `BaseFireBlock` (portal logic, placement checks) and `FireBlock`
//! (spreading, aging). This combines the portal-relevant parts from `BaseFireBlock`.

use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_dimension_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::portal::portal_shape::{PortalShape, nether_portal_config};
use crate::world::World;

/// Behavior for fire blocks.
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

/// Returns true if the dimension supports nether portal creation (Overworld or Nether).
fn in_portal_dimension(world: &World) -> bool {
    let key = &world.dimension.key;
    *key == vanilla_dimension_types::OVERWORLD.key
        || *key == vanilla_dimension_types::THE_NETHER.key
}

impl BlockBehavior for FireBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        // Only attempt portal creation when fire is newly placed, not when replacing itself
        if old_state.get_block() == state.get_block() {
            return;
        }

        if in_portal_dimension(world)
            && let Some(shape) =
                PortalShape::find_portal_shape(world, pos, &nether_portal_config())
        {
            shape.place_portal_blocks(world);
        }

        // TODO: Remove fire if it can't survive at this position (canSurvive check)
    }
}
