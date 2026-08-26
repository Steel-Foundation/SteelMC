//! Dispenser and Dropper block entity implementations.
//!
//! Dispensers and droppers share the exact same 9-slot (3x3 grid) container
//! data structure and randomization logic.

use std::{
    mem,
    sync::{Arc, Weak},
};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use std::str::FromStr;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier, locks::SyncMutex,
};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::world::World;

/// Number of slots in a dispenser/dropper (3x3 grid).
pub const DISPENSER_SLOTS: usize = 9;

/// Shared internal state for Dispenser-like block entities (Dispenser & Dropper).
pub struct SharedDispenserState {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<DispenserContainer>>,
    pub container_ref: ContainerRef,
}

/// Container data for a dispenser/dropper.
pub struct DispenserContainer {
    items: Vec<ItemStack>,
    /// Optional loot table used to populate this container when first accessed.
    pub loot_table: Option<Identifier>,
    /// Seed for randomizing the loot table output.
    pub loot_table_seed: i64,
}

// SAFETY: This key is owned by Steel and uniquely identifies the shared container
// data used by dispenser and dropper block entities.
unsafe impl DowncastType for DispenserContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/dispenser_like");
}

impl SharedDispenserState {
    /// Creates new shared state for a dispenser-like block entity.
    #[must_use]
    pub fn new(
        type_ref: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        let base = Arc::new(BlockEntityBase::new(type_ref, level, pos, state));
        let container = Arc::new(SyncMutex::new(DispenserContainer {
            items: vec![ItemStack::empty(); DISPENSER_SLOTS],
            loot_table: None,
            loot_table_seed: 0,
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
        }
    }

    /// Returns the internal container.
    #[must_use]
    pub const fn container(&self) -> &Arc<SyncMutex<DispenserContainer>> {
        &self.container
    }

    /// Returns a randomly chosen non-empty slot index if any exists.
    #[must_use]
    pub fn get_random_non_empty_slot(&self) -> Option<usize> {
        let container = self.container.lock();
        let non_empty_indices: Vec<usize> = container
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| if item.is_empty() { None } else { Some(idx) })
            .collect();

        if non_empty_indices.is_empty() {
            return None;
        }

        let pick = (rand::random::<u32>() as usize) % non_empty_indices.len();
        Some(non_empty_indices[pick])
    }

    /// Removes and returns a single item from the specified slot.
    #[must_use]
    pub fn take_single_item(&self, slot: usize) -> ItemStack {
        let mut container = self.container.lock();
        if slot >= DISPENSER_SLOTS {
            return ItemStack::empty();
        }

        let stack = &mut container.items[slot];
        if stack.is_empty() {
            return ItemStack::empty();
        }

        let split = stack.split(1);
        if stack.is_empty() {
            *stack = ItemStack::empty();
        }
        split
    }

    /// Inserts an item back into the dispenser container, merging or using empty slots.
    #[must_use]
    pub fn insert_item_back(&self, mut item: ItemStack) -> ItemStack {
        if item.is_empty() {
            return ItemStack::empty();
        }

        let mut container = self.container.lock();
        // First try to merge with existing non-empty matching slots
        for slot_item in &mut container.items {
            if !slot_item.is_empty() && ItemStack::is_same_item_same_components(slot_item, &item) {
                let max_stack = slot_item.max_stack_size();
                let available = max_stack - slot_item.count();
                if available > 0 {
                    let to_add = item.count().min(available);
                    slot_item.set_count(slot_item.count() + to_add);
                    item.set_count(item.count() - to_add);
                    if item.is_empty() {
                        return ItemStack::empty();
                    }
                }
            }
        }

        // Then place into first empty slot
        for slot_item in &mut container.items {
            if slot_item.is_empty() {
                *slot_item = item;
                return ItemStack::empty();
            }
        }

        item
    }

    /// Drops all inventory items on removal.
    pub fn pre_remove_side_effects(&self, pos: BlockPos) {
        let items = {
            let mut container = self.container.lock();
            mem::replace(
                &mut container.items,
                vec![ItemStack::empty(); DISPENSER_SLOTS],
            )
        };
        let Some(world) = self.base.level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    /// Loads items from NBT.
    pub fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();

        let loot_table = nbt_view
            .string("LootTable")
            .and_then(|value| Identifier::from_str(&value.to_string()).ok());

        let mut container = self.container.lock();
        container.loot_table = loot_table;
        container.loot_table_seed = nbt_view.long("LootTableSeed").unwrap_or(0);

        container.items.fill(ItemStack::empty());

        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                if let Some(slot) = compound.byte("Slot") {
                    let slot = slot as usize;
                    if slot < DISPENSER_SLOTS
                        && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                    {
                        container.items[slot] = item;
                    }
                }
            }
        }
    }

    /// Saves items into NBT.
    pub fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();

        if let Some(loot_table) = &container.loot_table {
            nbt.insert("LootTable", loot_table.to_string());
            if container.loot_table_seed != 0 {
                nbt.insert("LootTableSeed", NbtTag::Long(container.loot_table_seed));
            }
            return;
        }

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
}

impl Container for DispenserContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        DISPENSER_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < DISPENSER_SLOTS {
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

macro_rules! impl_dispenser_like_entity {
    ($name:ident, $doc:expr, $type_key:expr, $block_entity_type:expr) => {
        #[doc = $doc]
        pub struct $name {
            /// Shared internal container state and entity base.
            pub state: SharedDispenserState,
        }

        // SAFETY: This key is owned by Steel and uniquely identifies this block entity.
        unsafe impl DowncastType for $name {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($type_key);
        }

        impl $name {
            /// Creates a new block entity instance.
            #[must_use]
            pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
                Self {
                    state: SharedDispenserState::new($block_entity_type, level, pos, state),
                }
            }
        }

        impl BlockEntity for $name {
            fn base(&self) -> &BlockEntityBase {
                &self.state.base
            }

            fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
                self.state.pre_remove_side_effects(pos);
            }

            fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
                self.state.load_additional(nbt);
            }

            fn save_additional(&self, nbt: &mut NbtCompound) {
                self.state.save_additional(nbt);
            }

            fn get_update_tag(&self) -> Option<NbtCompound> {
                None
            }

            fn container_ref(&self) -> Option<ContainerRef> {
                Some(self.state.container_ref.clone())
            }
        }
    };
}

impl_dispenser_like_entity!(
    DropperBlockEntity,
    "Dropper block entity.",
    "steel:block_entity/dropper",
    &vanilla_block_entity_types::DROPPER
);
impl_dispenser_like_entity!(
    DispenserBlockEntity,
    "Dispenser block entity.",
    "steel:block_entity/dispenser",
    &vanilla_block_entity_types::DISPENSER
);

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};

    fn test_dropper() -> DropperBlockEntity {
        init_vanilla_registry();
        DropperBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::DROPPER.default_state(),
        )
    }

    #[test]
    fn set_item_limits_stack_to_vanilla_container_maximum() {
        let dropper = test_dropper();
        dropper
            .state
            .container()
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 100));

        assert_eq!(dropper.state.container().lock().get_item(0).count(), 64);
    }

    #[test]
    fn take_single_item_and_random_selection() {
        let dropper = test_dropper();
        assert_eq!(dropper.state.get_random_non_empty_slot(), None);

        dropper
            .state
            .container()
            .lock()
            .set_item(4, ItemStack::with_count(&vanilla_items::DIAMOND, 5));

        assert_eq!(dropper.state.get_random_non_empty_slot(), Some(4));

        let single = dropper.state.take_single_item(4);
        assert_eq!(single.item(), &*vanilla_items::DIAMOND);
        assert_eq!(single.count(), 1);
        assert_eq!(dropper.state.container().lock().get_item(4).count(), 4);
    }

    #[test]
    fn pre_remove_preserves_slots_for_existing_menu_references() {
        let dropper = test_dropper();
        dropper
            .state
            .container()
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::STONE));

        dropper.pre_remove_side_effects(
            BlockPos::new(1, 2, 3),
            vanilla_blocks::DROPPER.default_state(),
        );

        let container = dropper.state.container().lock();
        assert_eq!(container.items.len(), DISPENSER_SLOTS);
        assert!(container.items.iter().all(ItemStack::is_empty));
    }

    #[test]
    fn test_nbt_roundtrip() {
        use simdnbt::borrow::read_compound as read_borrowed_compound;
        use std::io::Cursor;

        let dropper = test_dropper();
        dropper
            .state
            .container()
            .lock()
            .set_item(2, ItemStack::with_count(&vanilla_items::GOLD_INGOT, 3));
        dropper
            .state
            .container()
            .lock()
            .set_item(7, ItemStack::with_count(&vanilla_items::REDSTONE, 16));

        let mut nbt = NbtCompound::new();
        dropper.save_additional(&mut nbt);
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("saved dropper NBT should decode");

        let loaded = test_dropper();
        loaded.load_additional(&borrowed);
        let container = loaded.state.container().lock();
        assert_eq!(container.get_item(2).count(), 3);
        assert_eq!(container.get_item(2).item(), &*vanilla_items::GOLD_INGOT);
        assert_eq!(container.get_item(7).count(), 16);
        assert_eq!(container.get_item(7).item(), &*vanilla_items::REDSTONE);
        assert!(container.get_item(0).is_empty());
    }

    #[test]
    fn test_insert_item_back() {
        let dropper = test_dropper();
        let remaining = dropper
            .state
            .insert_item_back(ItemStack::with_count(&vanilla_items::DIAMOND, 10));
        assert!(remaining.is_empty());
        assert_eq!(dropper.state.container().lock().get_item(0).count(), 10);
        assert_eq!(
            dropper.state.container().lock().get_item(0).item(),
            &*vanilla_items::DIAMOND
        );

        let remaining = dropper
            .state
            .insert_item_back(ItemStack::with_count(&vanilla_items::DIAMOND, 5));
        assert!(remaining.is_empty());
        assert_eq!(dropper.state.container().lock().get_item(0).count(), 15);
    }
}
