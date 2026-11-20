//! Dispenser block behavior. Vanilla `DispenserBlock`.
//!
//! A redstone pulse schedules a delayed tick that dispenses one item from a random
//! slot. Only the default dispense behavior is implemented: the item is spawned in
//! front of the block. Projectile, bucket and other item-specific behaviors are not.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast};

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::DispenserBlockEntity;
use crate::inventory::container::calculate_redstone_signal_from_containers;
use crate::inventory::lock::ContainerLockGuard;
use crate::player::Player;
use crate::world::{LevelReader, SignalGetter, World};

pub(crate) const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
pub(crate) const TRIGGERED: &BoolProperty = &BlockStateProperties::TRIGGERED;
/// Vanilla `DispenserBlock.TRIGGER_DURATION`.
const TRIGGER_DURATION: i32 = 4;
/// Vanilla `DefaultDispenseItemBehavior.DEFAULT_ACCURACY`.
const DEFAULT_ACCURACY: f64 = 6.0;

/// Vanilla `DispenserBlock.neighborChanged`: schedules a dispense on a rising signal.
pub(crate) fn update_triggered(
    block: BlockRef,
    state: BlockStateId,
    world: &Arc<World>,
    pos: BlockPos,
) {
    let should_trigger = world.has_neighbor_signal(pos) || world.has_neighbor_signal(pos.above());
    let is_triggered = state.get_value(TRIGGERED);
    if should_trigger && !is_triggered {
        world.schedule_block_tick_default(pos, block, TRIGGER_DURATION);
        world.set_block(
            pos,
            state.set_value(TRIGGERED, true),
            UpdateFlags::UPDATE_CLIENTS,
        );
    } else if !should_trigger && is_triggered {
        world.set_block(
            pos,
            state.set_value(TRIGGERED, false),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}

/// Vanilla `DefaultDispenseItemBehavior.dispense`: spawns one item in front of the
/// block with a randomized velocity, then plays the dispense sound and smoke animation.
/// Returns the stack that remains in the slot.
pub(crate) fn dispense_default(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    mut stack: ItemStack,
) -> ItemStack {
    let direction = state.get_value(FACING);
    let step = direction.relative(pos);
    let (sx, sy, sz) = (
        f64::from(step.x() - pos.x()),
        f64::from(step.y() - pos.y()),
        f64::from(step.z() - pos.z()),
    );
    // Vanilla `getDispensePosition`: block center, pushed 0.7 blocks toward the facing.
    let mut spawn = DVec3::new(
        f64::from(pos.x()) + 0.5 + 0.7 * sx,
        f64::from(pos.y()) + 0.5 + 0.7 * sy,
        f64::from(pos.z()) + 0.5 + 0.7 * sz,
    );
    spawn.y -= if sy != 0.0 { 0.125 } else { 0.15625 };

    let spread = 0.0172275 * DEFAULT_ACCURACY;
    let pow = rand::random::<f64>() * 0.1 + 0.2;
    let velocity = DVec3::new(
        triangle(sx * pow, spread),
        triangle(0.2, spread),
        triangle(sz * pow, spread),
    );

    let one = stack.split(1);
    world.spawn_item_with_velocity(spawn, one, velocity);
    world.level_event(1000, pos, 0, None);
    world.level_event(2000, pos, direction.get_3d_data_value(), None);
    stack
}

/// Vanilla `RandomSource.triangle`.
fn triangle(mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rand::random::<f64>() - rand::random::<f64>())
}

/// Vanilla `DispenserBlock.dispenseFrom`: dispenses from a random non-empty slot.
pub(crate) fn dispense_from_dispenser(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    let Some(block_entity) = world.get_block_entity(pos) else {
        log::warn!(
            "Ignoring dispensing attempt for Dispenser without matching block entity at {pos:?}"
        );
        return;
    };
    let Some(dispenser) = block_entity.downcast_ref::<DispenserBlockEntity>() else {
        log::warn!(
            "Ignoring dispensing attempt for Dispenser without matching block entity at {pos:?}"
        );
        return;
    };
    let Some(slot) = dispenser.random_slot() else {
        world.level_event(1001, pos, 0, None);
        return;
    };
    let remaining = dispense_default(world, pos, state, dispenser.get_slot(slot));
    dispenser.set_slot(slot, remaining);
}

/// Behavior for dispenser blocks.
#[block_behavior]
pub struct DispenserBlock {
    block: BlockRef,
}

impl DispenserBlock {
    /// Creates a new dispenser block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for DispenserBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.get_nearest_looking_direction().opposite();
        Some(self.block.default_state().set_value(FACING, facing))
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        // TODO: Open the dispenser menu (vanilla player.openMenu) and award INSPECT_DISPENSER.
        InteractionResult::Success
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
        dispense_from_dispenser(world, pos, state);
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::DISPENSER,
            level,
            pos,
            state,
        ))
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return 0;
        };
        let Some(container_ref) = block_entity.container_ref() else {
            return 0;
        };
        let Ok(guard) = ContainerLockGuard::lock_all(&[&container_ref]) else {
            return 0;
        };
        calculate_redstone_signal_from_containers(guard.get(container_ref.container_id()))
    }
}
