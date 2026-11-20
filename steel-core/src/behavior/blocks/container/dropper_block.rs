//! Dropper block behavior. Vanilla `DropperBlock`.
//!
//! Like the dispenser, but the chosen item is pushed into the container in front of
//! it (hopper-style) and only dispensed as an entity when no container is there.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, Downcast};

use super::dispenser_block::{FACING, dispense_default, update_triggered};
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::BlockPlaceContext;
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::{DispenserBlockEntity, HopperBlockEntity};
use crate::inventory::lock::ContainerLockGuard;
use crate::world::World;

/// Vanilla `DropperBlock.dispenseFrom`.
fn drop_from_dropper(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    let Some(block_entity) = world.get_block_entity(pos) else {
        return;
    };
    let Some(dropper) = block_entity.downcast_ref::<DispenserBlockEntity>() else {
        return;
    };
    let Some(slot) = dropper.random_slot() else {
        world.level_event(1001, pos, 0, None);
        return;
    };
    let stack = dropper.get_slot(slot);
    let direction: Direction = state.get_value(FACING);
    let target = direction.relative(pos);

    let Some((target_ref, target_be)) =
        crate::block_entity::entities::get_block_container(world, target)
    else {
        dropper.set_slot(slot, dispense_default(world, pos, state, stack));
        return;
    };

    // Vanilla: insert one copy of the stack into the target and shrink the source only on success.
    let mut one = stack.clone();
    one.set_count(1);
    let receiver = target_be.downcast_ref::<HopperBlockEntity>();
    let remainder = {
        let Ok(mut guard) = ContainerLockGuard::lock_all(&[&target_ref]) else {
            return;
        };
        let Some(to) = guard.get_mut(target_ref.container_id()) else {
            return;
        };
        crate::block_entity::entities::insert_stack(
            to,
            one,
            Some(direction.opposite()),
            receiver,
            None,
        )
    };
    let mut remaining = stack;
    if remainder.is_empty() {
        let _ = remaining.split(1);
    }
    dropper.set_slot(slot, remaining);
}

/// Behavior for dropper blocks.
#[block_behavior]
pub struct DropperBlock {
    block: BlockRef,
}

impl DropperBlock {
    /// Creates a new dropper block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for DropperBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.get_nearest_looking_direction().opposite();
        Some(self.block.default_state().set_value(FACING, facing))
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        update_triggered(self.block, state, world, pos);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        drop_from_dropper(world, pos, state);
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::DROPPER,
            level,
            pos,
            state,
        ))
    }
}
