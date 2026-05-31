//! `ColoredFallingBlock` and `SandBlock` behaviors.
//!
//! Vanilla: `SandBlock extends ColoredFallingBlock`. Server-side both are identical —
//! the dust particle color is client-only. The two Rust types are kept separate to
//! preserve the 1:1 class mapping required by the codegen (`#[block_behavior(class)]`).

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::entities::falling_block::is_free;
use crate::world::{ScheduledTickAccess, World};

use super::{schedule_fall_tick, spawn_falling_entity};

// ─── ColoredFallingBlock ──────────────────────────────────────────────────────

/// Behavior for falling blocks with a dust particle color (gravel).
///
/// Vanilla: `ColoredFallingBlock`. The dust color is client-side only and not
/// reproduced server-side; behavior is identical to `FallingBlock`.
#[block_behavior(class = "ColoredFallingBlock")]
pub struct ColoredFallingBlock {
    block: BlockRef,
}

impl ColoredFallingBlock {
    /// Creates a new `ColoredFallingBlock` behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for ColoredFallingBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        schedule_fall_tick(world, pos, self.block);
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
        schedule_fall_tick(world, pos, self.block);
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let below = world.get_block_state(pos.below());
        if is_free(below) && pos.y() >= world.get_min_y() {
            spawn_falling_entity(world, pos, state);
        }
    }
}

/// Behavior for sand and red sand.
///
/// Vanilla: `SandBlock extends ColoredFallingBlock`. Server-side behavior is
/// identical to `ColoredFallingBlock`; the dust color is client-only.
#[block_behavior(class = "SandBlock")]
pub struct SandBlock {
    block: BlockRef,
}

impl SandBlock {
    /// Creates a new `SandBlock` behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SandBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        schedule_fall_tick(world, pos, self.block);
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
        schedule_fall_tick(world, pos, self.block);
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let below = world.get_block_state(pos.below());
        if is_free(below) && pos.y() >= world.get_min_y() {
            spawn_falling_entity(world, pos, state);
        }
    }
}
