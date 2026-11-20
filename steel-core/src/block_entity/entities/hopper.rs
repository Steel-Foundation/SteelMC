//! Hopper block entity implementation.
//!
//! Port of Vanilla `HopperBlockEntity`: five slots and a transfer cooldown. Each
//! tick it pushes into the container on its facing side, or pulls from the
//! container above or from item entities in its suck box.
//!
//! Known gaps against Vanilla: minecart (entity) containers are not Steel
//! containers yet, double chests are not combined, and the hopper menu is not
//! implemented, so right-click opens nothing.

use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_block_tags::BlockTag;

use steel_utils::{
    BlockPos, BlockStateId, Downcast, DowncastType, DowncastTypeKey, WorldAabb, locks::SyncMutex,
};

use crate::block_entity::item_transfer::{
    add_item, can_take_from, exposed_slots, is_full_container,
};
use crate::block_entity::{BlockEntity, BlockEntityBase, SharedBlockEntity};
use crate::entity::entities::ItemEntity;
use crate::entity::{Entity, RemovalReason};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef, SharedContainer};
use crate::world::World;

/// Number of slots in a hopper.
pub const HOPPER_SLOTS: usize = 5;
/// Vanilla `HopperBlockEntity.MOVE_ITEM_SPEED`: ticks between transfers.
const MOVE_ITEM_SPEED: i32 = 8;
/// Vanilla `HopperBlockEntity.NO_COOLDOWN_TIME`.
const NO_COOLDOWN_TIME: i32 = -1;

/// Vanilla `FACING` property of `HopperBlock`.
pub const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING_HOPPER;
/// Vanilla `ENABLED` property of `HopperBlock`.
pub const ENABLED: &BoolProperty = &BlockStateProperties::ENABLED;

/// Vanilla `Hopper.SUCK_AABB` (`Block.column(16, 11, 32)`) relative to the hopper's block.
fn suck_aabb(pos: BlockPos) -> WorldAabb {
    let (x, y, z) = (f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
    WorldAabb::new(x, y + 11.0 / 16.0, z, x + 1.0, y + 2.0, z + 1.0)
}

/// Hopper block entity.
pub struct HopperBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<HopperContainer>>,
    container_ref: ContainerRef,
    cooldown_time: AtomicI32,
    ticked_game_time: AtomicI64,
}

struct HopperContainer {
    items: Vec<ItemStack>,
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
        }));
        let shared_container: SharedContainer =
            Arc::<SyncMutex<HopperContainer>>::clone(&container);
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
            cooldown_time: AtomicI32::new(NO_COOLDOWN_TIME),
            ticked_game_time: AtomicI64::new(0),
        }
    }

    fn cooldown(&self) -> i32 {
        self.cooldown_time.load(Ordering::Relaxed)
    }

    fn set_cooldown(&self, time: i32) {
        self.cooldown_time.store(time, Ordering::Relaxed);
    }

    fn is_on_cooldown(&self) -> bool {
        self.cooldown() > 0
    }

    fn is_on_custom_cooldown(&self) -> bool {
        self.cooldown() > MOVE_ITEM_SPEED
    }

    fn ticked_game_time(&self) -> i64 {
        self.ticked_game_time.load(Ordering::Relaxed)
    }

    fn is_empty(&self) -> bool {
        self.container.lock().items.iter().all(ItemStack::is_empty)
    }

    /// Vanilla `HopperBlockEntity.inventoryFull`.
    fn inventory_full(&self) -> bool {
        self.container
            .lock()
            .items
            .iter()
            .all(|stack| !stack.is_empty() && stack.count() == stack.max_stack_size())
    }

    /// Vanilla `HopperBlockEntity.pushItemsTick`.
    fn push_items_tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let state = self.get_block_state();
        self.cooldown_time.fetch_sub(1, Ordering::Relaxed);
        self.ticked_game_time
            .store(world.game_time(), Ordering::Relaxed);
        if !self.is_on_cooldown() {
            self.set_cooldown(0);
            self.try_move_items(world, pos, state, || self.suck_in_items(world, pos));
        }
    }

    /// Vanilla `HopperBlockEntity.tryMoveItems`.
    fn try_move_items(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        action: impl FnOnce() -> bool,
    ) -> bool {
        if self.is_on_cooldown() || !state.get_value(ENABLED) {
            return false;
        }
        let mut changed = false;
        if !self.is_empty() {
            changed = self.eject_items(world, pos, state.get_value(FACING));
        }
        if !self.inventory_full() {
            changed |= action();
        }
        if changed {
            self.set_cooldown(MOVE_ITEM_SPEED);
            self.set_changed();
        }
        changed
    }

    /// Vanilla `HopperBlockEntity.ejectItems`: pushes one item into the attached container.
    fn eject_items(&self, world: &Arc<World>, pos: BlockPos, facing: Direction) -> bool {
        let Some((target_ref, target_be)) = get_block_container(world, facing.relative(pos)) else {
            return false;
        };
        let direction = facing.opposite();
        let ids = [self.container_ref.container_id(), target_ref.container_id()];
        let Ok(mut guard) = ContainerLockGuard::lock_all(&[&self.container_ref, &target_ref]) else {
            return false;
        };
        let Some([from, to]) = guard.get_disjoint_mut(ids) else {
            return false;
        };
        if is_full_container(to, direction) {
            return false;
        }
        let receiver = target_be.downcast_ref::<HopperBlockEntity>();
        let from_time = Some(self.ticked_game_time());
        for slot in 0..HOPPER_SLOTS {
            if !from.get_item(slot).is_empty()
                && move_one(from, to, slot, Some(direction), receiver, from_time)
            {
                return true;
            }
        }
        false
    }

    /// Vanilla `HopperBlockEntity.suckInItems`: pulls from the container above, or
    /// from item entities in the suck box when the block above does not block it.
    fn suck_in_items(&self, world: &Arc<World>, pos: BlockPos) -> bool {
        let above = Direction::Up.relative(pos);
        if let Some((source_ref, source_be)) = get_block_container(world, above) {
            return self.take_from_container(&source_ref, &source_be);
        }
        let above_state = world.get_block_state(above);
        let blocked = world.is_collision_shape_full_block_at(above, above_state)
            && !above_state
                .get_block()
                .has_tag(&BlockTag::DOES_NOT_BLOCK_HOPPERS);
        if blocked {
            return false;
        }
        let entities = world.get_entities_in_aabb_matching(&suck_aabb(pos), |entity| {
            entity.is_alive() && entity.downcast_ref::<ItemEntity>().is_some()
        });
        entities.iter().any(|entity| {
            entity
                .downcast_ref::<ItemEntity>()
                .is_some_and(|item| self.absorb_item(item))
        })
    }

    /// Vanilla `HopperBlockEntity.tryTakeInItemFromSlot` over every exposed slot.
    fn take_from_container(
        &self,
        source_ref: &ContainerRef,
        source_be: &SharedBlockEntity,
    ) -> bool {
        let ids = [source_ref.container_id(), self.container_ref.container_id()];
        let Ok(mut guard) = ContainerLockGuard::lock_all(&[source_ref, &self.container_ref]) else {
            return false;
        };
        let Some([from, to]) = guard.get_disjoint_mut(ids) else {
            return false;
        };
        let from_time = source_be
            .downcast_ref::<HopperBlockEntity>()
            .map(HopperBlockEntity::ticked_game_time);
        for &slot in exposed_slots(&*from, Some(Direction::Down)).iter() {
            let taken_allowed = {
                let stack = from.get_item(slot);
                !stack.is_empty() && can_take_from(&*to, &*from, slot, stack, Direction::Down)
            };
            if taken_allowed && move_one(from, to, slot, None, Some(self), from_time) {
                return true;
            }
        }
        false
    }

    /// Vanilla `HopperBlockEntity.addItem(container, ItemEntity)`: absorbs what fits
    /// from a dropped item and discards it when fully consumed.
    fn absorb_item(&self, item: &ItemEntity) -> bool {
        let Ok(mut guard) = ContainerLockGuard::lock_all(&[&self.container_ref]) else {
            return false;
        };
        let Some(to) = guard.get_mut(self.container_ref.container_id()) else {
            return false;
        };
        let remainder = insert_stack(to, item.get_item(), None, Some(self), None);
        drop(guard);
        if remainder.is_empty() {
            item.set_item(ItemStack::empty());
            item.set_removed(RemovalReason::Discarded);
            true
        } else {
            item.set_item(remainder);
            false
        }
    }

    /// Vanilla `HopperBlockEntity.entityInside`, called by `HopperBlock` for entities
    /// that are inside its block.
    pub(crate) fn entity_inside(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        entity: &dyn Entity,
        hopper: &HopperBlockEntity,
    ) {
        let Some(item) = entity.downcast_ref::<ItemEntity>() else {
            return;
        };
        if item.get_item().is_empty() || !entity.bounding_box().intersects(suck_aabb(pos)) {
            return;
        }
        hopper.try_move_items(world, pos, state, || hopper.absorb_item(item));
    }

    /// Vanilla `HopperBlockEntity.onItemsReceived` cooldown rule: a hopper that
    /// received into an empty inventory waits one tick less when its source has
    /// already ticked this game tick.
    fn on_received(&self, was_empty: bool, from_time: Option<i64>) {
        if !was_empty || self.is_on_custom_cooldown() {
            return;
        }
        let skip = from_time.is_some_and(|from| self.ticked_game_time() >= from);
        self.set_cooldown(MOVE_ITEM_SPEED - i32::from(skip));
    }
}

/// Vanilla `HopperBlockEntity.getContainerAt` for block containers.
fn get_block_container(
    world: &Arc<World>,
    pos: BlockPos,
) -> Option<(ContainerRef, SharedBlockEntity)> {
    let block_entity = world.get_block_entity(pos)?;
    let container = ContainerRef::from_block_entity(Arc::clone(&block_entity))?;
    Some((container, block_entity))
}

/// Vanilla `HopperBlockEntity.addItem(from, container, stack, direction)` plus the
/// receiving hopper's cooldown bookkeeping from `tryMoveInItem`.
fn insert_stack(
    to: &mut dyn Container,
    stack: ItemStack,
    direction: Option<Direction>,
    receiver: Option<&HopperBlockEntity>,
    from_time: Option<i64>,
) -> ItemStack {
    let was_empty = to.is_empty();
    let total = stack.count();
    let remainder = add_item(to, stack, direction);
    if remainder.count() < total
        && let Some(receiver) = receiver
    {
        receiver.on_received(was_empty, from_time);
    }
    remainder
}

/// Vanilla `HopperBlockEntity.ejectItems` / `tryTakeInItemFromSlot` body: moves one
/// item from `from[slot]` into `to`, restoring `from` when nothing fits.
fn move_one(
    from: &mut dyn Container,
    to: &mut dyn Container,
    slot: usize,
    direction: Option<Direction>,
    receiver: Option<&HopperBlockEntity>,
    from_time: Option<i64>,
) -> bool {
    let original = from.get_item(slot).count();
    let taken = from.remove_item(slot, 1);
    let remainder = insert_stack(to, taken, direction, receiver, from_time);
    if remainder.is_empty() {
        from.set_changed();
        to.set_changed();
        return true;
    }
    if original == 1 {
        from.set_item(slot, remainder);
    } else {
        from.get_item_mut(slot).set_count(original);
    }
    false
}

impl BlockEntity for HopperBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            std::mem::replace(&mut container.items, vec![ItemStack::empty(); HOPPER_SLOTS])
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
        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
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
        drop(container);
        self.set_cooldown(nbt_view.int("TransferCooldown").unwrap_or(NO_COOLDOWN_TIME));
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
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
        nbt.insert("TransferCooldown", self.cooldown());
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        None
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    fn tick(&self, world: &Arc<World>) {
        self.push_items_tick(world);
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
    use std::sync::atomic::Ordering;

    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::BlockPos;

    use super::*;

    struct TestContainer {
        items: Vec<ItemStack>,
    }

    // SAFETY: Test-only key, unique to this test container.
    unsafe impl DowncastType for TestContainer {
        const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/hopper_container");
    }

    impl Container for TestContainer {
        fn items(&self) -> &[ItemStack] {
            &self.items
        }

        fn items_mut(&mut self) -> &mut [ItemStack] {
            &mut self.items
        }

        fn get_container_size(&self) -> usize {
            self.items.len()
        }

        fn set_changed(&mut self) {}
    }

    fn test_hopper() -> HopperBlockEntity {
        init_vanilla_registry();
        HopperBlockEntity::new(
            Weak::new(),
            BlockPos::new(0, 0, 0),
            vanilla_blocks::HOPPER.default_state(),
        )
    }

    #[test]
    fn on_received_skips_one_tick_when_source_already_ticked() {
        let hopper = test_hopper();
        hopper.ticked_game_time.store(10, Ordering::Relaxed);

        hopper.on_received(true, Some(10));

        assert_eq!(hopper.cooldown(), MOVE_ITEM_SPEED - 1);
    }

    #[test]
    fn on_received_full_cooldown_without_same_tick_source() {
        let hopper = test_hopper();
        hopper.ticked_game_time.store(10, Ordering::Relaxed);

        hopper.on_received(true, Some(11));
        assert_eq!(hopper.cooldown(), MOVE_ITEM_SPEED);

        hopper.set_cooldown(NO_COOLDOWN_TIME);
        hopper.on_received(false, Some(10));
        assert_eq!(hopper.cooldown(), NO_COOLDOWN_TIME);
    }

    #[test]
    fn move_one_restores_source_when_target_full() {
        init_vanilla_registry();
        let mut from = TestContainer {
            items: vec![ItemStack::with_count(&vanilla_items::STONE, 1)],
        };
        let mut to = TestContainer {
            items: vec![ItemStack::with_count(&vanilla_items::STONE, 64)],
        };

        let moved = move_one(&mut from, &mut to, 0, None, None, None);

        assert!(!moved);
        assert_eq!(from.get_item(0).count(), 1);
        assert_eq!(to.get_item(0).count(), 64);
    }
}
