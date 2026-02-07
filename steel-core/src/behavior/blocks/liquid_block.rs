//! Liquid block behavior implementation for water and lava.

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_utils::BlockStateId;

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;

/// Behavior for liquid blocks (water and lava).
///
/// Liquid blocks have a LEVEL property (0-15) that determines the fluid state:
/// - LEVEL 0 = source block (full fluid)
/// - LEVEL 1-7 = flowing fluid with decreasing height
/// - LEVEL 8-15 = falling fluid
pub struct LiquidBlock {
    block: BlockRef,
    fluid: FluidRef,
}

impl LiquidBlock {
    /// Creates a new liquid block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, fluid: FluidRef) -> Self {
        Self { block, fluid }
    }
}

impl BlockBehaviour for LiquidBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn get_fluid_state(&self, state: BlockStateId) -> FluidState {
        let level = state.get_value(&BlockStateProperties::LEVEL);
        FluidState::from_block_level(self.fluid, level)
    }
}
