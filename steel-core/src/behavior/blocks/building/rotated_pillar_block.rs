//! Rotated pillar block behavior implementation.
//!
//! Pillar blocks (like logs) have an axis property that determines their orientation.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, EnumProperty};
use steel_utils::BlockStateId;
use steel_utils::math::Axis;

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;

/// Behavior for rotated pillar blocks (logs, pillars, etc.).
///
/// These blocks have an axis property that is set based on which face
/// was clicked during placement.
#[block_behavior]
pub struct RotatedPillarBlock {
    block: BlockRef,
}

impl RotatedPillarBlock {
    /// Axis property for the pillar orientation.
    pub const AXIS: EnumProperty<Axis> = BlockStateProperties::AXIS;

    /// Creates a new rotated pillar block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for RotatedPillarBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(&Self::AXIS, context.clicked_face.get_axis()),
        )
    }
}
