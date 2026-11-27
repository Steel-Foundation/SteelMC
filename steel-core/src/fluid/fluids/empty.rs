//! Empty fluid implementation.
//!
//! Represents the absence of fluid in a block space.
//!

use std::sync::Arc;

use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::FluidRef;
use steel_registry::items::ItemRef;
use steel_registry::{vanilla_blocks, vanilla_fluids, vanilla_items};
use steel_utils::types::{BlockPos, BlockStateId};

use crate::fluid::FluidBehavior;
use crate::fluid::FluidState;
use crate::world::World;

/// Empty fluid behavior - represents the absence of fluid.
pub struct EmptyFluid;

impl FluidBehavior for EmptyFluid {
    fn create_legacy_block(&self, _fluid_state: FluidState) -> BlockStateId {
        vanilla_blocks::AIR.default_state()
    }

    fn bucket(&self) -> ItemRef {
        &vanilla_items::AIR
    }

    fn fluid_type(&self) -> FluidRef {
        &vanilla_fluids::EMPTY
    }

    fn tick(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _block_state: BlockStateId,
        _fluid_state: FluidState,
    ) {
        // Vanilla: nothing
    }

    fn spread(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _block_state: BlockStateId,
        _fluid_state: FluidState,
    ) {
        // Vanilla: nothing
    }

    fn tick_delay(&self, _world: &Arc<World>) -> i32 {
        0
    }

    fn drop_off(&self, _world: &Arc<World>) -> u8 {
        0
    }

    fn slope_find_distance(&self, _world: &Arc<World>) -> u8 {
        0
    }

    /// Returns true if empty can be replaced by another fluid.
    /// Based on vanilla `EmptyFluid.canBeReplacedWith()`.
    /// Empty can always be replaced.
    fn can_be_replaced_with(
        &self,
        _fluid_state: FluidState,
        _world: &Arc<World>,
        _pos: BlockPos,
        _other_fluid: FluidRef,
        _direction: Direction,
    ) -> bool {
        // Empty can always be replaced by any fluid
        true
    }
}
