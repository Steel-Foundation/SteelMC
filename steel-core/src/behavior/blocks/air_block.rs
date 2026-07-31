//! `AirBlock` behavior implementation.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;

use crate::behavior::{BlockBehavior, BlockPlaceContext};

/// Behavior for air blocks.
///
/// In vanilla, this overrides voxel shape and rendering to be empty/invisible.
/// `SteelMC`'s static registry already extracts and applies empty shapes for air blocks.
#[block_behavior]
pub struct AirBlock {
    block: BlockRef,
}

impl AirBlock {
    /// Creates a new air block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for AirBlock {
    fn get_state_for_placement(
        &self,
        _context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        Some(self.block.default_state())
    }
}
