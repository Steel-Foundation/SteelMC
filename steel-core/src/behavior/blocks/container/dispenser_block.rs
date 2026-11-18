//! Dispenser and Dropper block behavior implementations.
//!
//! Redstone-activated containers that can eject items into the world or
//! insert them into adjacent containers in the direction it faces.

use super::ChestBlock;
use super::dispense_behavior::{
    DISPENSE_BEHAVIORS, DefaultDispenseBehavior, DispenseItemBehavior, DispenseResult, eject_item,
    play_feedback,
};
use std::sync::{Arc, Weak};

use rand::seq::IteratorRandom as _;
use smallvec::SmallVec;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    level_events, sound_events, vanilla_block_entity_types, vanilla_blocks, vanilla_custom_stats,
    vanilla_game_events,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::entities::{DispenserBlockEntity, DropperBlockEntity};
use crate::block_entity::{BLOCK_ENTITIES, BlockEntity};
use crate::entity::Entity as _;
use crate::inventory::container::{Container, calculate_redstone_signal_from_containers};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::generic_3x3_menu;
use crate::nameable::Nameable as _;
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{SignalGetter as _, World};

pub(crate) const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const TRIGGERED: &BoolProperty = &BlockStateProperties::TRIGGERED;
const DISPENSE_DELAY_TICKS: i32 = 4;

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
        let Some(source) = world
            .get_block_entity(pos)
            .and_then(|entity| entity.downcast_ref::<DropperBlockEntity>()?.container_ref())
        else {
            return;
        };

        let source_id = source.container_id();
        let Ok(guard) = ContainerLockGuard::lock_all(&[&source]) else {
            return;
        };
        let Some(slot) = guard.get(source_id).and_then(random_slot) else {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
            return;
        };

        drop(guard);
        let facing = state.get_value(FACING);
        let targets = target_containers(world, pos.relative(facing));

        if targets.is_empty() {
            let mut guard = ContainerLockGuard::lock_storage(&[&source]);
            if let Some(mut item) = guard.remove_item(source_id, slot, 1) {
                if item.is_empty() {
                    return;
                }

                guard.run_unlocked(|| {
                    let outcome = DefaultDispenseBehavior.dispense(world, pos, state, &mut item);
                    play_feedback(world, pos, state, outcome.sound());
                });
            }

            return;
        }

        for (index, target) in targets.iter().enumerate() {
            let target_id = target.container_id();
            let Ok(mut guard) = ContainerLockGuard::lock_all(&[&source, target]) else {
                return;
            };

            if index == 0
                && let Some(second) = targets.get(1)
                && guard.get(target_id).is_some_and(Container::is_empty)
                && guard
                    .run_unlocked(|| ContainerLockGuard::lock_all(&[second]).map(drop))
                    .is_err()
            {
                return;
            }

            let Some([source, target]) = guard.get_disjoint_mut([source_id, target_id]) else {
                continue;
            };

            let mut item = source.get_item(slot).copy_with_count(1);
            if item.is_empty() {
                return;
            }

            insert_item(target, &mut item, Some(facing.opposite()));
            if item.is_empty() {
                source.remove_item(slot, 1);
                guard.set_changed(target_id);
                guard.set_changed(source_id);

                return;
            }
        }
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

        let Some(container) = block_entity.downcast_ref::<DropperBlockEntity>() else {
            return InteractionResult::Pass;
        };

        let title = container.name();
        player.award_custom_stat(&vanilla_custom_stats::INSPECT_DROPPER);
        open_menu(
            block_entity.as_ref(),
            player,
            world,
            pos,
            title,
            container.can_open(player),
        )
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
        update_trigger(world, pos, state, self.block);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        Self::dispense_from(world, pos, state);
    }

    fn affect_neighbors_after_removal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        world.update_neighbor_for_output_signal(pos, self.block);
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

    /// runs the behavior for the selected stack
    pub fn dispense_from(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let Some(source) = world.get_block_entity(pos).and_then(|entity| {
            entity
                .downcast_ref::<DispenserBlockEntity>()?
                .container_ref()
        }) else {
            return;
        };

        let source_id = source.container_id();
        let Ok(mut guard) = ContainerLockGuard::lock_all(&[&source]) else {
            return;
        };
        let Some(container) = guard.get_mut(source_id) else {
            return;
        };
        let Some(slot) = random_slot(container) else {
            guard.run_unlocked(|| {
                world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
                world.game_event(
                    &vanilla_game_events::BLOCK_ACTIVATE,
                    pos,
                    &GameEventContext::new(None, Some(state)),
                );
            });
            return;
        };

        let mut stack = container.remove_item_no_update(slot);
        let outcome = guard.run_unlocked(|| {
            DISPENSE_BEHAVIORS
                .get_behavior(&stack)
                .dispense(world, pos, state, &mut stack)
        });

        let sound = outcome.sound();
        let mut overflow = ItemStack::empty();
        if let DispenseResult::Byproduct(byproduct) = outcome {
            stack.shrink(1);
            if stack.is_empty() {
                stack = byproduct;
            } else {
                overflow = byproduct;
            }
        }

        if let Some(container) = guard.get_mut(source_id) {
            container.set_item(slot, stack);
            if !overflow.is_empty() {
                insert_item(container, &mut overflow, None);
            }
            guard.set_changed(source_id);
        }

        guard.run_unlocked(|| {
            if !overflow.is_empty() {
                eject_item(world, pos, state.get_value(FACING), overflow);
                play_feedback(world, pos, state, level_events::SOUND_DISPENSER_DISPENSE);
            }
            play_feedback(world, pos, state, sound);
        });
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

        let Some(container) = block_entity.downcast_ref::<DispenserBlockEntity>() else {
            return InteractionResult::Pass;
        };
        let title = container.name();
        player.award_custom_stat(&vanilla_custom_stats::INSPECT_DISPENSER);
        open_menu(
            block_entity.as_ref(),
            player,
            world,
            pos,
            title,
            container.can_open(player),
        )
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
        update_trigger(world, pos, state, self.block);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        Self::dispense_from(world, pos, state);
    }

    fn affect_neighbors_after_removal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        world.update_neighbor_for_output_signal(pos, self.block);
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
        get_analog_signal(world, pos)
    }
}

fn get_analog_signal(world: &World, pos: BlockPos) -> i32 {
    let Some(block_entity) = world.get_block_entity(pos) else {
        return 0;
    };
    let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
        return 0;
    };
    let Ok(guard) = ContainerLockGuard::lock_all(&[&container_ref]) else {
        // Match other randomizable containers while loot access fails.
        return 0;
    };
    calculate_redstone_signal_from_containers(guard.get(container_ref.container_id()))
}

fn update_trigger(world: &Arc<World>, pos: BlockPos, state: BlockStateId, block: BlockRef) {
    let powered = world.has_neighbor_signal(pos) || world.has_neighbor_signal(pos.above());
    if powered == state.get_value(TRIGGERED) {
        return;
    }

    if powered {
        world.schedule_block_tick_default(pos, block, DISPENSE_DELAY_TICKS);
    }
    world.set_block(
        pos,
        state.set_value(TRIGGERED, powered),
        UpdateFlags::UPDATE_CLIENTS,
    );
}

fn open_menu(
    entity: &dyn BlockEntity,
    player: &Player,
    world: &World,
    pos: BlockPos,
    title: TextComponent,
    can_open: bool,
) -> InteractionResult {
    if !can_open {
        if !player.is_spectator() {
            player.send_overlay_message(
                &translations::CONTAINER_IS_LOCKED
                    .message([title])
                    .component(),
            );
            world.play_block_sound(&sound_events::BLOCK_CHEST_LOCKED, pos, 1.0, 1.0, None);
        }

        return InteractionResult::Success;
    }

    if let Some(randomizable) = entity.as_randomizable_container()
        && let Err(error) = randomizable.unpack_loot_table(Some(player))
    {
        log::error!("Could not unpack dispenser or dropper loot at {pos:?}: {error}");
        return InteractionResult::Success;
    }

    let Some(container) = entity.container_ref() else {
        return InteractionResult::Pass;
    };

    let inventory = Arc::clone(&player.inventory);
    player.open_menu(title, move |context| {
        generic_3x3_menu(inventory, context.container_id, container)
    });

    InteractionResult::Success
}

fn random_slot(container: &dyn Container) -> Option<usize> {
    container
        .items()
        .iter()
        .enumerate()
        .filter_map(|(slot, item)| (!item.is_empty()).then_some(slot))
        .choose(&mut rand::rng())
}

fn target_containers(world: &World, pos: BlockPos) -> SmallVec<[ContainerRef; 2]> {
    let state = world.get_block_state(pos);
    if [&vanilla_blocks::CHEST, &vanilla_blocks::TRAPPED_CHEST].contains(&state.get_block()) {
        return ChestBlock::get_container(state, world, pos, true)
            .map_or_else(SmallVec::new, |chest| chest.containers);
    }

    world
        .get_block_entity(pos)
        .and_then(ContainerRef::from_block_entity)
        .into_iter()
        .collect()
}

fn insert_item(container: &mut dyn Container, item: &mut ItemStack, face: Option<Direction>) {
    let slots = face.and_then(|face| container.slots_for_face(face));
    let size = container.get_container_size();
    let insert_at = |slot| {
        if !container.can_place_item(slot, item)
            || face.is_some_and(|face| !container.can_place_item_through_face(slot, item, face))
        {
            return false;
        }

        let stored = container.get_item(slot);
        let capacity = container.get_max_stack_size_for_item(item);
        if stored.is_empty() {
            let count = item.count().min(capacity);
            if count <= 0 {
                return false;
            }

            container.set_item(slot, item.copy_with_count(count));
            let inserted = container.get_item(slot);

            if ItemStack::is_same_item_same_components(inserted, item) {
                item.shrink(inserted.count().min(count));
            }
        } else if ItemStack::is_same_item_same_components(stored, item) && stored.count() < capacity
        {
            let count = item.count().min(capacity - stored.count());
            container.get_item_mut(slot).grow(count);
            item.shrink(count);
        }

        item.is_empty()
    };

    if let Some(slots) = slots {
        slots.iter().copied().any(insert_at);
    } else {
        (0..size).any(insert_at);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::entity::{entities::ItemEntity, init_entities};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::{BlockLocalAabb, ChunkPos};

    pub(crate) fn with_inventory<T>(
        entity: &dyn BlockEntity,
        access: impl FnOnce(&mut dyn Container) -> T,
    ) -> T {
        let container = entity.container_ref().expect("test container");
        let mut guard = ContainerLockGuard::lock_all(&[&container]).expect("test inventory");
        access(
            guard
                .get_mut(container.container_id())
                .expect("locked test inventory"),
        )
    }

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

        let world_fixture = fresh_test_world("dispenser_test");
        let world = &world_fixture.world;
        let pos1 = BlockPos::new(0, 64, 0);
        let pos2 = BlockPos::new(2, 64, 0);
        let _holder = insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos1));

        let created_dropper =
            dropper_block.new_block_entity(Arc::downgrade(world), pos1, dropper_state);
        let created_dispenser =
            dispenser_block.new_block_entity(Arc::downgrade(world), pos2, dispenser_state);

        assert!(created_dropper.into_created().is_some());
        assert!(created_dispenser.into_created().is_some());

        assert_eq!(
            dropper_block.get_analog_output_signal(dropper_state, world, pos1, Direction::North),
            0
        );
        assert_eq!(
            dispenser_block.get_analog_output_signal(
                dispenser_state,
                world,
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

        let world_fixture = fresh_test_world("dropper_transfer_test");
        let world = &world_fixture.world;
        let dropper_pos = BlockPos::new(0, 64, 0);
        let target_pos = BlockPos::new(0, 64, 1);
        let _holder = insert_ready_full_chunk(world, ChunkPos::from_block_pos(dropper_pos));

        let dropper_state = vanilla_blocks::DROPPER
            .default_state()
            .set_value(FACING, Direction::South);

        let dropper_entity = Arc::new(DropperBlockEntity::new(
            Arc::downgrade(world),
            dropper_pos,
            dropper_state,
        ));
        with_inventory(dropper_entity.as_ref(), |inventory| {
            inventory.set_item(0, ItemStack::with_count(&vanilla_items::DIAMOND, 5));
        });

        let target_entity = Arc::new(DropperBlockEntity::new(
            Arc::downgrade(world),
            target_pos,
            vanilla_blocks::DROPPER.default_state(),
        ));

        world.set_block(dropper_pos, dropper_state, UpdateFlags::UPDATE_ALL);
        world.set_block_entity(Arc::<DropperBlockEntity>::clone(&dropper_entity));
        world.set_block(
            target_pos,
            vanilla_blocks::DROPPER.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
        world.set_block_entity(Arc::<DropperBlockEntity>::clone(&target_entity));

        DropperBlock::dispense_from(world, dropper_pos, dropper_state);

        assert_eq!(
            with_inventory(dropper_entity.as_ref(), |inventory| inventory
                .get_item(0)
                .count()),
            4
        );
        assert_eq!(
            with_inventory(target_entity.as_ref(), |inventory| inventory
                .get_item(0)
                .count()),
            1
        );
        assert_eq!(
            with_inventory(target_entity.as_ref(), |inventory| inventory
                .get_item(0)
                .item()),
            &*vanilla_items::DIAMOND
        );
    }

    #[test]
    fn dispenser_ejects_arrow_item() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();
        init_entities();

        let world_fixture = fresh_test_world("dispenser_eject_test");
        let world = &world_fixture.world;
        let pos = BlockPos::new(8, 64, 8);
        let _holder = insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));

        let state = vanilla_blocks::DISPENSER
            .default_state()
            .set_value(FACING, Direction::North);

        let entity = Arc::new(DispenserBlockEntity::new(Arc::downgrade(world), pos, state));
        with_inventory(entity.as_ref(), |inventory| {
            inventory.set_item(0, ItemStack::with_count(&vanilla_items::ARROW, 10));
        });

        world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
        world.set_block_entity(Arc::<DispenserBlockEntity>::clone(&entity));

        DispenserBlock::dispense_from(world, pos, state);

        assert_eq!(
            with_inventory(entity.as_ref(), |inventory| inventory.get_item(0).count()),
            9
        );
        assert_eq!(
            with_inventory(entity.as_ref(), |inventory| inventory.get_item(0).item()),
            &*vanilla_items::ARROW
        );

        let target = pos.relative(Direction::North);
        let bounds = BlockLocalAabb::FULL_BLOCK.at_block(target);
        let items: Vec<_> = world
            .get_entities_in_aabb(&bounds)
            .iter()
            .filter_map(|entity| {
                entity
                    .downcast_ref::<ItemEntity>()
                    .map(ItemEntity::get_item)
            })
            .collect();
        assert_eq!(items, [ItemStack::new(&vanilla_items::ARROW)]);
    }
}
