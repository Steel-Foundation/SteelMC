//! Chain block behavior implementation.
//!
//! Chains are oriented blocks with an axis property that determines their direction.
//! They can also be waterlogged.

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, EnumProperty};
use steel_utils::BlockStateId;
use steel_utils::math::Axis;

/// Behavior for chain blocks (iron chain, waxed copper chains).
///
/// Chains have an axis property that is set based on which face was clicked
/// during placement, and can be waterlogged.
#[block_behavior]
pub struct ChainBlock {
    block: BlockRef,
}

impl ChainBlock {
    /// Axis property for the chain orientation.
    pub const AXIS: EnumProperty<Axis> = BlockStateProperties::AXIS;
    /// Waterlogged property.
    pub const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

    /// Creates a new chain block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for ChainBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(&Self::AXIS, context.clicked_face.get_axis())
                .set_value(&Self::WATERLOGGED, context.is_water_source()),
        )
    }
}
