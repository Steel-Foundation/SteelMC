use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::level_events;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::schedule_water_tick_if_waterlogged;
use crate::behavior::blocks::redstone::{MAX_REDSTONE_SIGNAL, MIN_REDSTONE_SIGNAL};
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::entity::ai::path::PathComputationType;
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

/// Lightning rod behavior.
#[block_behavior]
pub struct LightningRodBlock {
    block: BlockRef,
}

const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const POWERED: &BoolProperty = &BlockStateProperties::POWERED;

const ACTIVATION_TICKS: i32 = 8;

impl LightningRodBlock {
    /// Creates a lightning rod behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    fn update_neighbors(block: BlockRef, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let front = state.get_value(FACING).opposite();
        // Experimental redstone orientations are intentionally omitted.
        world.update_neighbors_at(pos.relative(front), block);
    }
    #[expect(
        dead_code,
        reason = "We need to call this function once on_lightning_strike gets implemented in BlockBehavior"
    )]
    fn on_lightning_strike(
        block: BlockRef,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) {
        world.set_block(pos, state.set_value(POWERED, true), UpdateFlags::UPDATE_ALL);
        Self::update_neighbors(block, state, world, pos);
        world.schedule_block_tick_default(pos, block, ACTIVATION_TICKS);
        world.level_event(
            level_events::PARTICLES_ELECTRIC_SPARK,
            pos,
            state.get_value(FACING).get_axis().ordinal(),
            None,
        );
    }
}

impl BlockBehavior for LightningRodBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(FACING, context.clicked_face())
                .set_value(WATERLOGGED, context.is_water_source()),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);

        state
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        if state.get_value(POWERED) {
            return MAX_REDSTONE_SIGNAL;
        }
        MIN_REDSTONE_SIGNAL
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if state.get_value(POWERED) && state.get_value(FACING) == direction {
            return MAX_REDSTONE_SIGNAL;
        }
        MIN_REDSTONE_SIGNAL
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        world.set_block(
            pos,
            state.set_value(POWERED, false),
            UpdateFlags::UPDATE_ALL,
        );
        Self::update_neighbors(self.block, state, world, pos);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        if state.get_value(POWERED) {
            Self::update_neighbors(self.block, state, world, pos);
        }
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if state.get_block() != old_state.get_block()
            && state.get_value(POWERED)
            && !world.has_scheduled_block_tick(pos, self.block)
        {
            world.schedule_block_tick_default(pos, self.block, ACTIVATION_TICKS);
        }
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    // `animateTick` emits client-local particles only.
}
