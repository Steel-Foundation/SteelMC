//! Hopper block entity implementation.
//!
//! Hoppers are container block entities with 5 slots that push items toward
//! their facing, pull from the container above, and suck in item entities.

use std::{
    mem,
    sync::{Arc, Weak},
};

use glam::DVec3;
use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{
    BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey, WorldAabb,
    locks::SyncMutex,
};

use crate::block_entity::randomizable::RandomizableContainerLoot;
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::entities::ItemEntity;
use crate::entity::{Entity, RemovalReason, SharedEntity};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerId, ContainerLockGuard, ContainerRef, SharedContainer};
use crate::world::World;

/// Number of slots in a hopper.
pub const HOPPER_SLOTS: usize = 5;

/// Vanilla `MOVE_ITEM_SPEED`: ticks of cooldown after a successful transfer.
const MOVE_ITEM_SPEED: i32 = 8;

/// Vanilla `NO_COOLDOWN_TIME`: cooldown value of a freshly created hopper.
const NO_COOLDOWN_TIME: i32 = -1;

/// Hopper block entity.
///
/// A container with 5 slots that moves items between containers.
pub struct HopperBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<HopperContainer>>,
    container_ref: ContainerRef,
}

struct HopperContainer {
    items: Vec<ItemStack>,
    cooldown_time: i32,
    ticked_game_time: i64,
    loot: RandomizableContainerLoot,
}

// SAFETY: This key is owned by Steel and uniquely identifies `HopperBlockEntity`.
unsafe impl DowncastType for HopperBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/hopper");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by a hopper block entity.
unsafe impl DowncastType for HopperContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/hopper");
}

impl HopperBlockEntity {
    /// Creates a new hopper block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::HOPPER,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(HopperContainer {
            items: vec![ItemStack::empty(); HOPPER_SLOTS],
            cooldown_time: NO_COOLDOWN_TIME,
            ticked_game_time: 0,
            loot: RandomizableContainerLoot::new(),
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
        }
    }

    /// Unpacks a pending loot table into the slots (vanilla `unpackLootTable`).
    ///
    /// Callers that surface the contents outside a transfer (menu open, break)
    /// must call this first.
    pub fn unpack_loot_table(&self) {
        let pos = self.get_block_pos();
        self.container.lock().unpack_loot_table(pos);
    }

    /// Vanilla `HopperBlockEntity.entityInside`: immediately tries to absorb an
    /// item entity intersecting the suck box, on the same cooldown rules as the tick.
    pub fn entity_inside(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        entity: &dyn Entity,
    ) {
        let Some(item_entity) = entity.downcast_ref::<ItemEntity>() else {
            return;
        };
        if item_entity.get_item().is_empty() {
            return;
        }
        if !entity.bounding_box().intersects(Self::suck_aabb(pos)) {
            return;
        }
        self.try_move_items(world, pos, state, || self.add_item_from_entity(item_entity));
    }

    /// Vanilla `Hopper.SUCK_AABB` moved to the hopper position: the full column
    /// from the bowl rim to one block above the hopper.
    fn suck_aabb(pos: BlockPos) -> WorldAabb {
        // Vanilla `Block.column(16.0, 11.0, 32.0)`, converted from sixteenths.
        const SUCK_MIN_Y: f64 = 11.0 / 16.0;
        const SUCK_MAX_Y: f64 = 32.0 / 16.0;
        let base = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        WorldAabb::from_min_max(
            base + DVec3::new(0.0, SUCK_MIN_Y, 0.0),
            base + DVec3::new(1.0, SUCK_MAX_Y, 1.0),
        )
    }

    /// Vanilla `tryMoveItems`: eject, then pull/suck, then start the cooldown.
    fn try_move_items(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        suck: impl FnOnce() -> bool,
    ) -> bool {
        if self.container.lock().cooldown_time > 0 {
            return false;
        }
        if !state.get_value(&BlockStateProperties::ENABLED) {
            return false;
        }

        let mut changed = false;
        if !self.is_empty_after_unpack(pos) {
            changed = self.eject_items(world, pos, state);
        }
        if !self.container.lock().inventory_full() {
            changed |= suck();
        }

        if changed {
            self.container.lock().cooldown_time = MOVE_ITEM_SPEED;
            self.set_changed();
            return true;
        }
        false
    }

    fn is_empty_after_unpack(&self, pos: BlockPos) -> bool {
        let mut container = self.container.lock();
        container.unpack_loot_table(pos);
        container.is_empty()
    }

    /// Vanilla `ejectItems`: moves one item into the container this hopper faces.
    fn eject_items(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> bool {
        let facing = state.get_value(&BlockStateProperties::FACING_HOPPER);
        let target_pos = facing.relative(pos);
        let Some(target_ref) = Self::container_at(world, target_pos) else {
            return false;
        };

        let self_ref = self.container_ref.clone();
        let (self_id, target_id) = (self_ref.container_id(), target_ref.container_id());
        let mut guard = ContainerLockGuard::lock_all(&[&self_ref, &target_ref]);
        Self::unpack_in_guard(&mut guard, self_id, pos);
        Self::unpack_in_guard(&mut guard, target_id, target_pos);

        // Vanilla passes the insertion face for WorldlyContainer (sided) slot
        // selection; Steel has no sided containers yet, so the flat slot order
        // below is the only path.
        let Some(target) = guard.get(target_id) else {
            return false;
        };
        if Self::is_full_container(target) {
            return false;
        }

        for slot in 0..HOPPER_SLOTS {
            let Some(source) = guard.get(self_id) else {
                return false;
            };
            let stack = source.get_item(slot).clone();
            if stack.is_empty() {
                continue;
            }

            let original_count = stack.count();
            let Some(source) = guard.get_mut(self_id) else {
                return false;
            };
            let taken = source.remove_item(slot, 1);
            let leftover = Self::add_item_into(&mut guard, Some(self_id), target_id, taken);
            if leftover.is_empty() {
                return true;
            }

            // Nothing fit; restore the slot exactly as vanilla does.
            let Some(source) = guard.get_mut(self_id) else {
                return false;
            };
            if original_count == 1 {
                source.set_item(slot, stack);
            } else {
                source.get_item_mut(slot).set_count(original_count);
            }
        }
        false
    }

    /// Vanilla `suckInItems`: pull from the container above, or absorb item
    /// entities when nothing blocks the suck box.
    fn suck_in_items(&self, world: &Arc<World>, pos: BlockPos) -> bool {
        let above_pos = pos.above();
        if let Some(source_ref) = Self::container_at(world, above_pos) {
            let self_ref = self.container_ref.clone();
            let (self_id, source_id) = (self_ref.container_id(), source_ref.container_id());
            let mut guard = ContainerLockGuard::lock_all(&[&self_ref, &source_ref]);
            Self::unpack_in_guard(&mut guard, self_id, pos);
            Self::unpack_in_guard(&mut guard, source_id, above_pos);

            let source_size = guard
                .get(source_id)
                .map_or(0, Container::get_container_size);
            for slot in 0..source_size {
                if Self::try_take_in_item_from_slot(&mut guard, source_id, self_id, slot) {
                    return true;
                }
            }
            return false;
        }

        // Item-entity hoppers only work when the block above does not fully
        // block the suck box. Minecart hoppers additionally skip this check.
        let above_state = world.get_block_state(above_pos);
        let blocked = world.is_collision_shape_full_block_at(above_pos, above_state)
            && !above_state
                .get_block()
                .has_tag(&BlockTag::DOES_NOT_BLOCK_HOPPERS);
        if blocked {
            return false;
        }

        for entity in Self::items_at_and_above(world, pos) {
            let Some(item_entity) = entity.downcast_ref::<ItemEntity>() else {
                continue;
            };
            if self.add_item_from_entity(item_entity) {
                return true;
            }
        }
        false
    }

    /// Vanilla `getItemsAtAndAbove`: live item entities intersecting the suck box.
    fn items_at_and_above(world: &Arc<World>, pos: BlockPos) -> Vec<SharedEntity> {
        world.get_entities_in_aabb_matching(&Self::suck_aabb(pos), |entity| {
            !entity.is_removed() && entity.downcast_ref::<ItemEntity>().is_some()
        })
    }

    /// Vanilla `addItem(Container, ItemEntity)`: absorbs the entity's stack and
    /// discards the entity when everything fit.
    fn add_item_from_entity(&self, item_entity: &ItemEntity) -> bool {
        let pos = self.get_block_pos();
        let self_ref = self.container_ref.clone();
        let self_id = self_ref.container_id();
        let mut guard = ContainerLockGuard::lock_all(&[&self_ref]);
        Self::unpack_in_guard(&mut guard, self_id, pos);
        let leftover = Self::add_item_into(&mut guard, None, self_id, item_entity.get_item());
        drop(guard);

        if leftover.is_empty() {
            item_entity.set_item(ItemStack::empty());
            item_entity.set_removed(RemovalReason::Discarded);
            true
        } else {
            item_entity.set_item(leftover);
            false
        }
    }

    /// Vanilla `tryTakeInItemFromSlot`: pulls one item out of `from` into `to`.
    fn try_take_in_item_from_slot(
        guard: &mut ContainerLockGuard,
        from_id: ContainerId,
        to_id: ContainerId,
        slot: usize,
    ) -> bool {
        let (Some(from), Some(to)) = (guard.get(from_id), guard.get(to_id)) else {
            return false;
        };
        let stack = from.get_item(slot).clone();
        if stack.is_empty() || !from.can_take_item(to, slot, &stack) {
            return false;
        }

        let original_count = stack.count();
        let Some(from) = guard.get_mut(from_id) else {
            return false;
        };
        let taken = from.remove_item(slot, 1);
        let leftover = Self::add_item_into(guard, Some(from_id), to_id, taken);
        if leftover.is_empty() {
            guard.set_changed(from_id);
            return true;
        }

        let Some(from) = guard.get_mut(from_id) else {
            return false;
        };
        if original_count == 1 {
            from.set_item(slot, stack);
        } else {
            from.get_item_mut(slot).set_count(original_count);
        }
        false
    }

    /// Vanilla `addItem(Container, Container, ItemStack, Direction)` over flat slots.
    fn add_item_into(
        guard: &mut ContainerLockGuard,
        from_id: Option<ContainerId>,
        to_id: ContainerId,
        mut stack: ItemStack,
    ) -> ItemStack {
        let size = guard.get(to_id).map_or(0, Container::get_container_size);
        for slot in 0..size {
            if stack.is_empty() {
                break;
            }
            stack = Self::try_move_in_item(guard, from_id, to_id, stack, slot);
        }
        stack
    }

    /// Vanilla `tryMoveInItem`: one slot's insertion attempt, including the
    /// cooldown hand-off when filling an empty hopper.
    fn try_move_in_item(
        guard: &mut ContainerLockGuard,
        from_id: Option<ContainerId>,
        to_id: ContainerId,
        mut stack: ItemStack,
        slot: usize,
    ) -> ItemStack {
        let was_empty;
        let success;
        {
            let Some(target) = guard.get_mut(to_id) else {
                return stack;
            };
            if !target.can_place_item(slot, &stack) {
                return stack;
            }
            was_empty = target.is_empty();
            if target.get_item(slot).is_empty() {
                target.set_item(slot, mem::take(&mut stack));
                success = true;
            } else {
                let (mergeable, space) = {
                    let current = target.get_item(slot);
                    (
                        Self::can_merge_items(current, &stack),
                        stack.max_stack_size() - current.count(),
                    )
                };
                let moved = if mergeable {
                    stack.count().min(space)
                } else {
                    0
                };
                if moved > 0 {
                    stack.shrink(moved);
                    target.get_item_mut(slot).grow(moved);
                }
                success = moved > 0;
            }
        }

        if success {
            let source_time = from_id
                .and_then(|id| guard.get_typed::<HopperContainer>(id))
                .map(|source| source.ticked_game_time);
            if was_empty
                && let Some(destination) = guard.get_typed_mut::<HopperContainer>(to_id)
                && !destination.is_on_custom_cooldown()
            {
                let skip_tick =
                    i32::from(source_time.is_some_and(|t| destination.ticked_game_time >= t));
                destination.cooldown_time = MOVE_ITEM_SPEED - skip_tick;
            }
            guard.set_changed(to_id);
        }
        stack
    }

    /// Vanilla `canMergeItems`.
    fn can_merge_items(target: &ItemStack, source: &ItemStack) -> bool {
        target.count() <= target.max_stack_size()
            && ItemStack::is_same_item_same_components(target, source)
    }

    /// Vanilla `isFullContainer`: every slot at its own item's stack limit.
    fn is_full_container(container: &dyn Container) -> bool {
        for slot in 0..container.get_container_size() {
            let stack = container.get_item(slot);
            if stack.count() < stack.max_stack_size() {
                return false;
            }
        }
        true
    }

    /// Vanilla `getContainerAt` for block containers.
    ///
    /// Vanilla additionally finds `WorldlyContainerHolder` blocks (composter)
    /// and container entities (chest minecarts); neither exists in Steel yet.
    fn container_at(world: &Arc<World>, pos: BlockPos) -> Option<ContainerRef> {
        world
            .get_block_entity(pos)
            .and_then(ContainerRef::from_block_entity)
    }

    fn unpack_in_guard(guard: &mut ContainerLockGuard, id: ContainerId, pos: BlockPos) {
        if let Some(container) = guard.get_typed_mut::<HopperContainer>(id) {
            container.unpack_loot_table(pos);
        }
    }
}

impl HopperContainer {
    fn unpack_loot_table(&mut self, pos: BlockPos) {
        if let Some(pending) = self.loot.take_pending() {
            pending.fill_container(pos, self);
        }
    }

    /// Vanilla `inventoryFull`: reads raw items without unpacking loot.
    fn inventory_full(&self) -> bool {
        self.items
            .iter()
            .all(|stack| !stack.is_empty() && stack.count() == stack.max_stack_size())
    }

    /// Vanilla `isOnCustomCooldown`: a cooldown longer than the transfer speed.
    const fn is_on_custom_cooldown(&self) -> bool {
        self.cooldown_time > MOVE_ITEM_SPEED
    }
}

impl BlockEntity for HopperBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            // Vanilla drops loot-table contents on break through `getItem`'s unpack.
            container.unpack_loot_table(pos);
            mem::replace(&mut container.items, vec![ItemStack::empty(); HOPPER_SLOTS])
        };
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());

        // Vanilla: saved items and a pending loot table are mutually exclusive.
        if !container.loot.try_load(&nbt_view)
            && let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                // Each item has a "Slot" byte and item data
                if let Some(slot) = compound.byte("Slot") {
                    let slot = slot as usize;
                    if slot < HOPPER_SLOTS
                        && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                    {
                        container.items[slot] = item;
                    }
                }
            }
        }

        container.cooldown_time = nbt_view.int("TransferCooldown").unwrap_or(NO_COOLDOWN_TIME);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
        if !container.loot.try_save(nbt) {
            // Save items to NBT (only non-empty slots)
            let mut items: Vec<NbtCompound> = Vec::new();
            for (slot, item) in container.items.iter().enumerate() {
                if !item.is_empty()
                    && let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag()
                {
                    item_nbt.insert("Slot", slot as i8);
                    items.push(item_nbt);
                }
            }
            nbt.insert("Items", NbtList::Compound(items));
        }
        nbt.insert("TransferCooldown", container.cooldown_time);
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        None
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    /// Vanilla `pushItemsTick`: cooldown bookkeeping, then one transfer attempt.
    fn tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let state = self.get_block_state();
        let on_cooldown = {
            let mut container = self.container.lock();
            container.cooldown_time -= 1;
            container.ticked_game_time = world.game_time();
            if container.cooldown_time > 0 {
                true
            } else {
                container.cooldown_time = 0;
                false
            }
        };
        if !on_cooldown {
            self.try_move_items(world, pos, state, || self.suck_in_items(world, pos));
        }
    }
}

impl Container for HopperContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        HOPPER_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < HOPPER_SLOTS {
            let max_stack_size = self.get_max_stack_size_for_item(&stack);
            if !stack.is_empty() && stack.count() > max_stack_size {
                stack.set_count(max_stack_size);
            }
            self.items[slot] = stack;
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        64
    }

    fn set_changed(&mut self) {}
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn test_hopper() -> HopperBlockEntity {
        init_vanilla_registry();
        HopperBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::HOPPER.default_state(),
        )
    }

    fn load_from_owned_nbt(entity: &HopperBlockEntity, nbt: &NbtCompound) {
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test nbt should reborrow");
        entity.load_additional(&borrowed);
    }

    fn hopper_world(key: &'static str, pos: BlockPos) -> Arc<World> {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world(key);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        world
    }

    fn container_item(world: &Arc<World>, pos: BlockPos, slot: usize) -> ItemStack {
        let container_ref = ContainerRef::from_block_entity(
            world
                .get_block_entity(pos)
                .expect("container block should have its block entity"),
        )
        .expect("container block should expose a container");
        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        guard
            .get(container_ref.container_id())
            .expect("container should be locked")
            .get_item(slot)
            .clone()
    }

    fn set_container_item(world: &Arc<World>, pos: BlockPos, slot: usize, stack: ItemStack) {
        let container_ref = ContainerRef::from_block_entity(
            world
                .get_block_entity(pos)
                .expect("container block should have its block entity"),
        )
        .expect("container block should expose a container");
        let mut guard = ContainerLockGuard::lock_all(&[&container_ref]);
        guard
            .get_mut(container_ref.container_id())
            .expect("container should be locked")
            .set_item(slot, stack);
    }

    #[test]
    fn set_item_limits_stack_to_vanilla_container_maximum() {
        let hopper = test_hopper();
        hopper
            .container
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 100));

        assert_eq!(hopper.container.lock().get_item(0).count(), 64);
    }

    #[test]
    fn pre_remove_preserves_slots_for_existing_menu_references() {
        let hopper = test_hopper();
        hopper
            .container
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::STONE));

        hopper.pre_remove_side_effects(
            BlockPos::new(1, 2, 3),
            vanilla_blocks::HOPPER.default_state(),
        );

        let container = hopper.container.lock();
        assert_eq!(container.items.len(), HOPPER_SLOTS);
        assert!(container.items.iter().all(ItemStack::is_empty));
    }

    #[test]
    fn transfer_cooldown_round_trips_and_defaults_to_no_cooldown() {
        let hopper = test_hopper();
        let mut nbt = NbtCompound::new();
        nbt.insert("TransferCooldown", 7_i32);
        load_from_owned_nbt(&hopper, &nbt);
        let mut saved = NbtCompound::new();
        hopper.save_additional(&mut saved);
        assert_eq!(saved.int("TransferCooldown"), Some(7));

        let fresh = test_hopper();
        load_from_owned_nbt(&fresh, &NbtCompound::new());
        let mut saved = NbtCompound::new();
        fresh.save_additional(&mut saved);
        assert_eq!(saved.int("TransferCooldown"), Some(-1));
    }

    #[test]
    fn pending_loot_table_round_trips_and_suppresses_items() {
        let hopper = test_hopper();
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:chests/simple_dungeon");
        nbt.insert("LootTableSeed", 99_i64);
        nbt.insert(
            "Items",
            NbtList::Compound(vec![{
                let NbtTag::Compound(mut item) = ItemStack::new(&vanilla_items::STONE).to_nbt_tag()
                else {
                    panic!("item stack should serialize to a compound");
                };
                item.insert("Slot", 0_i8);
                item
            }]),
        );
        load_from_owned_nbt(&hopper, &nbt);

        assert!(hopper.container.lock().is_empty());
        let mut saved = NbtCompound::new();
        hopper.save_additional(&mut saved);
        assert_eq!(
            saved.string("LootTable").map(ToString::to_string),
            Some("minecraft:chests/simple_dungeon".to_owned())
        );
        assert_eq!(saved.long("LootTableSeed"), Some(99));
        assert!(saved.list("Items").is_none());
    }

    #[test]
    fn seeded_unpack_is_deterministic_and_clears_the_loot_table() {
        let unpacked = || {
            let hopper = test_hopper();
            let mut nbt = NbtCompound::new();
            nbt.insert("LootTable", "minecraft:chests/simple_dungeon");
            nbt.insert("LootTableSeed", 42_i64);
            load_from_owned_nbt(&hopper, &nbt);
            hopper.unpack_loot_table();
            hopper
        };
        let first = unpacked();
        let second = unpacked();

        let first_items = first.container.lock().items.clone();
        let second_items = second.container.lock().items.clone();
        assert!(first_items.iter().any(|item| !item.is_empty()));
        for (a, b) in first_items.iter().zip(&second_items) {
            assert_eq!(a.count(), b.count());
            assert!(a.is_empty() || ItemStack::is_same_item_same_components(a, b));
        }

        let mut saved = NbtCompound::new();
        first.save_additional(&mut saved);
        assert!(saved.string("LootTable").is_none());
        assert!(saved.list("Items").is_some());
    }

    #[test]
    fn tick_pushes_one_item_into_the_facing_container_and_starts_cooldown() {
        let pos = BlockPos::new(8, 64, 8);
        let world = hopper_world("hopper_push", pos);
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::BARREL.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let hopper = HopperBlockEntity::new(
            Arc::downgrade(&world),
            pos,
            vanilla_blocks::HOPPER.default_state(),
        );
        hopper
            .container
            .lock()
            .set_item(2, ItemStack::with_count(&vanilla_items::STONE, 3));

        hopper.tick(&world);

        assert_eq!(container_item(&world, pos.below(), 0).count(), 1);
        assert_eq!(hopper.container.lock().get_item(2).count(), 2);
        assert_eq!(hopper.container.lock().cooldown_time, MOVE_ITEM_SPEED);
    }

    #[test]
    fn tick_pulls_one_item_from_the_container_above() {
        let pos = BlockPos::new(8, 64, 8);
        let world = hopper_world("hopper_pull", pos);
        assert!(world.set_block(
            pos.above(),
            vanilla_blocks::BARREL.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        set_container_item(
            &world,
            pos.above(),
            5,
            ItemStack::with_count(&vanilla_items::DIRT, 2),
        );
        let hopper = HopperBlockEntity::new(
            Arc::downgrade(&world),
            pos,
            vanilla_blocks::HOPPER.default_state(),
        );

        hopper.tick(&world);

        assert_eq!(hopper.container.lock().get_item(0).count(), 1);
        assert!(hopper.container.lock().get_item(0).is(&vanilla_items::DIRT));
        assert_eq!(container_item(&world, pos.above(), 5).count(), 1);
        assert_eq!(hopper.container.lock().cooldown_time, MOVE_ITEM_SPEED);
    }

    #[test]
    fn locked_hopper_moves_nothing_while_its_cooldown_still_counts_down() {
        let pos = BlockPos::new(8, 64, 8);
        let world = hopper_world("hopper_locked", pos);
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::BARREL.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let state = vanilla_blocks::HOPPER
            .default_state()
            .set_value(&BlockStateProperties::ENABLED, false);
        let hopper = HopperBlockEntity::new(Arc::downgrade(&world), pos, state);
        hopper
            .container
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::STONE));
        hopper.container.lock().cooldown_time = 5;

        hopper.tick(&world);
        assert_eq!(hopper.container.lock().cooldown_time, 4);

        // Once the cooldown runs out it clamps to zero, but the lock still
        // prevents any transfer.
        for _ in 0..6 {
            hopper.tick(&world);
        }
        assert_eq!(hopper.container.lock().cooldown_time, 0);
        assert_eq!(hopper.container.lock().get_item(0).count(), 1);
        assert!(container_item(&world, pos.below(), 0).is_empty());
    }

    #[test]
    fn full_collision_block_above_blocks_item_entity_suction() {
        let pos = BlockPos::new(8, 64, 8);
        let world = hopper_world("hopper_suck_blocked", pos);
        assert!(world.set_block(
            pos.above(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let item = world
            .spawn_item_with_velocity(
                DVec3::new(8.5, 65.2, 8.5),
                ItemStack::new(&vanilla_items::STONE),
                DVec3::ZERO,
            )
            .expect("item entity should spawn");
        let hopper = HopperBlockEntity::new(
            Arc::downgrade(&world),
            pos,
            vanilla_blocks::HOPPER.default_state(),
        );

        hopper.tick(&world);

        assert!(!item.is_removed());
        assert!(hopper.container.lock().get_item(0).is_empty());
    }

    #[test]
    fn suck_box_ends_exactly_two_blocks_above_the_hopper_base() {
        let pos = BlockPos::new(8, 64, 8);
        let world = hopper_world("hopper_suck_boundary", pos);
        let too_high = world
            .spawn_item_with_velocity(
                DVec3::new(8.5, 66.05, 8.5),
                ItemStack::new(&vanilla_items::STONE),
                DVec3::ZERO,
            )
            .expect("item entity should spawn");
        let in_range = world
            .spawn_item_with_velocity(
                DVec3::new(8.5, 65.9, 8.5),
                ItemStack::with_count(&vanilla_items::DIRT, 3),
                DVec3::ZERO,
            )
            .expect("item entity should spawn");
        let hopper = HopperBlockEntity::new(
            Arc::downgrade(&world),
            pos,
            vanilla_blocks::HOPPER.default_state(),
        );

        hopper.tick(&world);

        assert!(!too_high.is_removed());
        assert_eq!(too_high.get_item().count(), 1);
        assert!(in_range.is_removed());
        assert_eq!(hopper.container.lock().get_item(0).count(), 3);
    }

    #[test]
    fn tick_sucks_an_item_entity_from_the_box_above() {
        let pos = BlockPos::new(8, 64, 8);
        let world = hopper_world("hopper_suck", pos);
        let entity = world
            .spawn_item_with_velocity(
                DVec3::new(8.5, 65.2, 8.5),
                ItemStack::with_count(&vanilla_items::STONE, 2),
                DVec3::ZERO,
            )
            .expect("item entity should spawn");
        let hopper = HopperBlockEntity::new(
            Arc::downgrade(&world),
            pos,
            vanilla_blocks::HOPPER.default_state(),
        );

        hopper.tick(&world);

        assert_eq!(hopper.container.lock().get_item(0).count(), 2);
        assert!(entity.get_item().is_empty());
        assert!(entity.is_removed());
        assert_eq!(hopper.container.lock().cooldown_time, MOVE_ITEM_SPEED);
    }
}
