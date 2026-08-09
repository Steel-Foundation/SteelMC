use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::fluid::FluidStateExt;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `CoralBlock` survival.
#[block_behavior]
pub struct CoralBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "dead_block")]
    dead_block: BlockRef,
}

const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

impl CoralBlock {
    /// Creates a new live coral block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, dead_block: BlockRef) -> Self {
        Self { block, dead_block }
    }

    fn dead_state(&self) -> BlockStateId {
        self.dead_block.default_state()
    }
    /// Vanilla `BaseCoralPlantTypeBlock.canSurvive` (also `BaseCoralFanBlock`,
    /// `CoralPlantBlock`, `CoralFanBlock`).
    ///
    /// The block below must be face-sturdy on its UP face.
    pub(super) fn coral_plant_can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        world.is_face_sturdy(below, below_pos, Direction::Up)
    }

    /// Vanilla `BaseCoralPlantTypeBlock.scanForWater`.
    pub(super) fn scan_for_water(
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        if state.try_get_value(&WATERLOGGED) == Some(true) {
            return true;
        }

        Direction::ALL.iter().any(|direction| {
            world
                .get_block_state(pos.relative(*direction))
                .get_fluid_state()
                .is_water()
        })
    }

    /// Vanilla `BaseCoralPlantTypeBlock.tryScheduleDieTick`.
    pub(super) fn schedule_die_tick(
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        block: BlockRef,
    ) {
        if Self::scan_for_water(state, world, pos) {
            return;
        }

        // Intentional Steel divergence: incidental runtime timing does not use world RNG.
        let delay = 60 + rand::random_range(0..40);
        let _ = world.schedule_block_tick_default(pos, block, delay);
    }
}

impl BlockBehavior for CoralBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        Self::schedule_die_tick(state, world, pos, self.block);

        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        Self::schedule_die_tick(state, context.world, context.place_pos(), self.block);
        Some(state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !Self::scan_for_water(state, world, pos) {
            world.set_block(pos, self.dead_state(), UpdateFlags::UPDATE_CLIENTS);
        }
    }
}
