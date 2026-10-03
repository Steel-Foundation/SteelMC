//! Dispenser and Dropper block behavior implementations.
//!
//! Redstone-activated containers that can eject items into the world or
//! insert them into adjacent containers in the direction it faces.

use crate::behavior::blocks::container::dispense_behavior::DISPENSE_BEHAVIORS;
use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::{level_events, vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::{DispenserBlockEntity, DropperBlockEntity};
use crate::inventory::container::{Container, calculate_redstone_signal_from_container};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::generic_3x3_menu;
use crate::player::Player;
use crate::world::{LevelReader, SignalGetter as _, World};

pub(crate) const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const TRIGGERED: &BoolProperty = &BlockStateProperties::TRIGGERED;

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

    /// Ejects an item or pushes it into an adjacent container.
    pub fn dispense_from(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let Some(dropper) = block_entity.downcast_ref::<DropperBlockEntity>() else {
            return;
        };

        let Some(slot) = dropper.state.get_random_non_empty_slot() else {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
            return;
        };
        let item = dropper.state.take_single_item(slot);
        if item.is_empty() {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
            return;
        }

        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);

        if let Some(target_block_entity) = world.get_block_entity(target_pos)
            && let Some(target_container_ref) = ContainerRef::from_block_entity(target_block_entity)
        {
            let mut guard = ContainerLockGuard::lock_all(&[&target_container_ref]);
            if let Some(target_container) = guard.get_mut(target_container_ref.container_id()) {
                let remaining = insert_item_into_container(&mut *target_container, item);
                if remaining.is_empty() {
                    world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);
                    return;
                }
                world.drop_item_stack(target_pos, remaining);
                world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);
                return;
            }
        }

        world.drop_item_stack(target_pos, item);
        world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);
    }
}

impl BlockBehavior for DropperBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.get_nearest_looking_direction().opposite();
        Some(
            self.block
                .default_state()
                .set_value(FACING, facing)
                .set_value(TRIGGERED, false),
        )
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };

        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            return InteractionResult::Pass;
        };

        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_DROPPER.msg()),
            move |context| generic_3x3_menu(inventory, context.container_id, container_ref),
        );

        player.award_custom_stat(&vanilla_custom_stats::INSPECT_DROPPER);

        InteractionResult::Success
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

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        let is_powered = world.has_neighbor_signal(pos) || world.has_neighbor_signal(pos.above());
        let is_triggered = state.get_value(TRIGGERED);

        if is_powered && !is_triggered {
            world.schedule_block_tick_default(pos, self.block, 4);
            world.set_block(
                pos,
                state.set_value(TRIGGERED, true),
                UpdateFlags::UPDATE_CLIENTS,
            );
        } else if !is_powered && is_triggered {
            world.set_block(
                pos,
                state.set_value(TRIGGERED, false),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        Self::dispense_from(world, pos, state);
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        get_analog_signal(world, pos)
    }
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

    /// Ejects an item or pushes it into an adjacent container.
    pub fn dispense_from(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let Some(dispenser) = block_entity.downcast_ref::<DispenserBlockEntity>() else {
            return;
        };

        let Some(slot) = dispenser.state.get_random_non_empty_slot() else {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
            return;
        };
        let item = dispenser.state.take_single_item(slot);
        if item.is_empty() {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
            return;
        }

        let behavior = DISPENSE_BEHAVIORS.get_behavior(item.item());
        let remaining = behavior.dispense(world, pos, state, item);

        if !remaining.is_empty() {
            let mut guard = ContainerLockGuard::lock_all(&[&dispenser.state.container_ref]);
            if let Some(container) = guard.get_mut(dispenser.state.container_ref.container_id()) {
                let dropped = insert_item_into_container(&mut *container, remaining);
                if !dropped.is_empty() {
                    let facing = state.get_value(FACING);
                    world.drop_item_stack(pos.relative(facing), dropped);
                }
            }
        }
    }
}

impl BlockBehavior for DispenserBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.get_nearest_looking_direction().opposite();
        Some(
            self.block
                .default_state()
                .set_value(FACING, facing)
                .set_value(TRIGGERED, false),
        )
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };

        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            return InteractionResult::Pass;
        };

        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_DISPENSER.msg()),
            move |context| generic_3x3_menu(inventory, context.container_id, container_ref),
        );

        player.award_custom_stat(&vanilla_custom_stats::INSPECT_DISPENSER);

        InteractionResult::Success
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

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        let is_powered = world.has_neighbor_signal(pos) || world.has_neighbor_signal(pos.above());
        let is_triggered = state.get_value(TRIGGERED);

        if is_powered && !is_triggered {
            world.schedule_block_tick_default(pos, self.block, 4);
            world.set_block(
                pos,
                state.set_value(TRIGGERED, true),
                UpdateFlags::UPDATE_CLIENTS,
            );
        } else if !is_powered && is_triggered {
            world.set_block(
                pos,
                state.set_value(TRIGGERED, false),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        Self::dispense_from(world, pos, state);
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        get_analog_signal(world, pos)
    }
}

fn get_analog_signal(world: &dyn LevelReader, pos: BlockPos) -> i32 {
    let Some(block_entity) = world.get_block_entity(pos) else {
        return 0;
    };
    let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
        return 0;
    };
    let guard = ContainerLockGuard::lock_all(&[&container_ref]);
    guard
        .get(container_ref.container_id())
        .map_or(0, |container| {
            calculate_redstone_signal_from_container(container)
        })
}

/// Helper function to insert an item into a container respecting slot limits and stack sizes.
fn insert_item_into_container(container: &mut dyn Container, mut item: ItemStack) -> ItemStack {
    if item.is_empty() {
        return ItemStack::empty();
    }
    let size = container.get_container_size();

    for slot in 0..size {
        let current = container.get_item(slot);
        if !current.is_empty() && ItemStack::is_same_item_same_components(current, &item) {
            let max_stack = container
                .get_max_stack_size_for_item(current)
                .min(current.max_stack_size());
            let available = max_stack - current.count();
            if available > 0 {
                let to_add = item.count().min(available);
                let mut updated = current.clone();
                updated.set_count(current.count() + to_add);
                container.set_item(slot, updated);
                item.set_count(item.count() - to_add);
                if item.is_empty() {
                    return ItemStack::empty();
                }
            }
        }
    }

    for slot in 0..size {
        let current = container.get_item(slot);
        if current.is_empty() {
            let max_stack = container
                .get_max_stack_size_for_item(&item)
                .min(item.max_stack_size());
            if item.count() <= max_stack {
                container.set_item(slot, item);
                return ItemStack::empty();
            }
            let split = item.split(max_stack);
            container.set_item(slot, split);
            if item.is_empty() {
                return ItemStack::empty();
            }
        }
    }

    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::ChunkPos;

    #[test]
    fn dropper_and_dispenser_placement_and_analog_output() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();

        let dropper_block = DropperBlock::new(&vanilla_blocks::DROPPER);
        let dispenser_block = DispenserBlock::new(&vanilla_blocks::DISPENSER);

        let dropper_state = vanilla_blocks::DROPPER.default_state();
        let dispenser_state = vanilla_blocks::DISPENSER.default_state();

        assert!(dropper_block.has_analog_output_signal(dropper_state));
        assert!(dispenser_block.has_analog_output_signal(dispenser_state));

        let world = fresh_test_world("dispenser_test");
        let pos1 = BlockPos::new(0, 64, 0);
        let pos2 = BlockPos::new(2, 64, 0);
        let _holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos1));

        let created_dropper =
            dropper_block.new_block_entity(Arc::downgrade(&world), pos1, dropper_state);
        let created_dispenser =
            dispenser_block.new_block_entity(Arc::downgrade(&world), pos2, dispenser_state);

        assert!(created_dropper.into_created().is_some());
        assert!(created_dispenser.into_created().is_some());

        assert_eq!(
            dropper_block.get_analog_output_signal(dropper_state, &*world, pos1, Direction::North),
            0
        );
        assert_eq!(
            dispenser_block.get_analog_output_signal(
                dispenser_state,
                &*world,
                pos2,
                Direction::North
            ),
            0
        );
    }

    #[test]
    fn dropper_dispense_into_adjacent_container() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();

        let world = fresh_test_world("dropper_transfer_test");
        let dropper_pos = BlockPos::new(0, 64, 0);
        let target_pos = BlockPos::new(0, 64, 1);
        let _holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(dropper_pos));

        let dropper_state = vanilla_blocks::DROPPER
            .default_state()
            .set_value(FACING, Direction::South);

        let dropper_entity = Arc::new(DropperBlockEntity::new(
            Arc::downgrade(&world),
            dropper_pos,
            dropper_state,
        ));
        dropper_entity
            .state
            .container()
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::DIAMOND, 5));

        let target_entity = Arc::new(DropperBlockEntity::new(
            Arc::downgrade(&world),
            target_pos,
            vanilla_blocks::DROPPER.default_state(),
        ));

        world.set_block(dropper_pos, dropper_state, UpdateFlags::UPDATE_ALL);
        world.set_block_entity(dropper_entity.clone());
        world.set_block(
            target_pos,
            vanilla_blocks::DROPPER.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
        world.set_block_entity(target_entity.clone());

        DropperBlock::dispense_from(&world, dropper_pos, dropper_state);

        assert_eq!(
            dropper_entity.state.container().lock().get_item(0).count(),
            4
        );
        assert_eq!(
            target_entity.state.container().lock().get_item(0).count(),
            1
        );
        assert_eq!(
            target_entity.state.container().lock().get_item(0).item(),
            &*vanilla_items::DIAMOND
        );
    }

    #[test]
    fn dispenser_eject_into_world() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();

        let world = fresh_test_world("dispenser_eject_test");
        let pos = BlockPos::new(0, 64, 0);
        let _holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let state = vanilla_blocks::DISPENSER
            .default_state()
            .set_value(FACING, Direction::North);

        let entity = Arc::new(DispenserBlockEntity::new(
            Arc::downgrade(&world),
            pos,
            state,
        ));
        entity
            .state
            .container()
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::ARROW, 10));

        world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
        world.set_block_entity(entity.clone());

        DispenserBlock::dispense_from(&world, pos, state);

        assert_eq!(entity.state.container().lock().get_item(0).count(), 9);
        assert_eq!(
            entity.state.container().lock().get_item(0).item(),
            &*vanilla_items::ARROW
        );
    }
}
