//! Nether portal block behavior.

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_blocks::AIR;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::{BlockBehaviour, BlockInsideEffect};
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Behavior for the nether portal block.
pub struct NetherPortalBlock {
    #[warn(unused)]
    block: BlockRef,
}
impl NetherPortalBlock {
    /// Create a new `NetherPortalBlock`
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for NetherPortalBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if neighbor_state.is_air() {
            return AIR.default_state();
        }
        state
    }

    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        None // Cannot be placed by players
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &World,
        pos: BlockPos,
    ) -> BlockInsideEffect {
        BlockInsideEffect::Portal(pos)
    }
}
