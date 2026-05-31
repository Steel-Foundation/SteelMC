//! `ConcretePowderBlock` behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_fluids;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::entities::falling_block::is_free;
use crate::world::{ScheduledTickAccess, World};

use super::{schedule_fall_tick, spawn_falling_entity};

/// Behavior for all 16 concrete powder variants.
///
/// Concrete powder falls like other falling blocks. In addition:
/// - When it lands next to or in water it converts to concrete (`on_land`).
/// - When a neighboring water source appears it converts immediately (`update_shape`).
/// - `get_state_for_placement` checks if the placement position already touches water
///   and places concrete directly if so.
///
/// Vanilla: `ConcretePowderBlock extends FallingBlock implements Fallable`.
#[block_behavior(class = "ConcretePowderBlock")]
pub struct ConcretePowderBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "concrete")]
    concrete: BlockRef,
}

impl ConcretePowderBlock {
    /// Creates a new `ConcretePowderBlock` behavior.
    #[must_use]
    pub const fn new(block: BlockRef, concrete: BlockRef) -> Self {
        Self { block, concrete }
    }

    /// Converts to concrete if the block at `pos` or any neighbor contains water.
    ///
    /// Vanilla: `ConcretePowderBlock.shouldSolidify()`.
    fn should_solidify(
        &self,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        replaced: BlockStateId,
    ) -> bool {
        can_solidify(replaced) || touches_liquid(world, pos)
    }
}

impl BlockBehavior for ConcretePowderBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.clicked_pos;
        let replaced = context.world.get_block_state(pos);
        if self.should_solidify(context.world, pos, replaced) {
            Some(self.concrete.default_state())
        } else {
            Some(self.block.default_state())
        }
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
        if touches_liquid(world, pos) {
            self.concrete.default_state()
        } else {
            schedule_fall_tick(world, pos, self.block);
            state
        }
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let below = world.get_block_state(pos.below());
        if is_free(below) && pos.y() >= world.get_min_y() {
            spawn_falling_entity(world, pos, state);
        }
    }

    fn on_land(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        _placed_state: BlockStateId,
        replaced_state: BlockStateId,
        _entity: &dyn Entity,
    ) {
        if self.should_solidify(world, pos, replaced_state) {
            world.set_block(pos, self.concrete.default_state(), UpdateFlags::UPDATE_ALL);
        }
    }

    fn is_concrete_powder(&self) -> bool {
        true
    }
}

/// Returns true if the fluid state at `state` is water.
///
/// Vanilla: `ConcretePowderBlock.canSolidify()`.
fn can_solidify(state: BlockStateId) -> bool {
    let fluid = crate::fluid::state::get_fluid_state_from_block(state);
    fluid.fluid_id == &vanilla_fluids::WATER
}

/// Returns true if any face-adjacent block at `pos` contains water, with no sturdy face
/// blocking the contact.
///
/// Vanilla: `ConcretePowderBlock.touchesLiquid()`.
///
/// Differs from vanilla in loop structure but produces identical results: vanilla uses a
/// mutable BlockPos that is read *before* offset on each iteration — for the DOWN direction
/// this means it reads the concrete powder block itself, which can only solidify if the
/// powder is waterlogged, so DOWN is effectively always skipped for dry powder.
fn touches_liquid(world: &dyn ScheduledTickAccess, pos: BlockPos) -> bool {
    for dir in Direction::ALL {
        // Vanilla skips DOWN unless the concrete powder block itself can solidify
        // (i.e., it's waterlogged). Check the block at `pos`, not the neighbor.
        if dir == Direction::Down && !can_solidify(world.get_block_state(pos)) {
            continue;
        }

        let neighbor_pos = pos.relative(dir);
        let neighbor_state = world.get_block_state(neighbor_pos);
        if can_solidify(neighbor_state) && !neighbor_state.is_face_sturdy(dir.opposite()) {
            return true;
        }
    }

    false
}
