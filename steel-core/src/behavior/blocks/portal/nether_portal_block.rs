//! Nether portal block behavior.

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::portal::portal_shape::{PortalShape, nether_portal_config};
use crate::world::World;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks::AIR;
use steel_utils::math::Axis;
use steel_utils::{BlockPos, BlockStateId, Direction};

/// Behavior for the nether portal block.
#[block_behavior]
pub struct NetherPortalBlock {
    block: BlockRef,
}
impl NetherPortalBlock {
    /// Create a new `NetherPortalBlock`
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for NetherPortalBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let update_axis = direction.get_axis();
        let axis: Axis = state.get_value(&BlockStateProperties::HORIZONTAL_AXIS);
        let wrong_axis = axis != update_axis && update_axis != Axis::Y;

        if !wrong_axis
            && neighbor_state.get_block() != self.block
            && !PortalShape::find_any_shape(world, pos, axis, &nether_portal_config())
                .is_some_and(|s| s.is_complete())
        {
            return AIR.default_state();
        }
        state
    }

    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        None // TODO: add this functionality but has low priority
    }
}
