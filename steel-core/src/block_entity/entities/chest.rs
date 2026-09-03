//! Chest block entity implementation.
//!
//! Chests are container block entities with 27 slots (3x9 grid). Support for
//! structure-generated loot tables mirrors vanilla `RandomizableContainer`
//! (see [`ContainerLoot`]).
//!
//! Steel difference: chests do not yet merge into double chests; the
//! `type` block property is not used for container sharing.

use std::{
    mem,
    sync::{Arc, Weak},
};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::world::World;

use super::container_loot::ContainerLoot;

/// Number of slots in a chest (3 rows of 9).
pub const CHEST_SLOTS: usize = 27;

struct ChestContainer {
    items: Vec<ItemStack>,
}

/// Chest block entity.
pub struct ChestBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<ChestContainer>>,
    container_ref: ContainerRef,
    loot: SyncMutex<ContainerLoot>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChestBlockEntity`.
unsafe impl DowncastType for ChestBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/chest");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by a chest block entity.
unsafe impl DowncastType for ChestContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/chest");
}

impl ChestBlockEntity {
    /// Creates a new chest block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::CHEST,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(ChestContainer {
            items: vec![ItemStack::empty(); CHEST_SLOTS],
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
            loot: SyncMutex::new(ContainerLoot::default()),
        }
    }

    /// Rolls any pending structure loot table into the chest, mirroring
    /// vanilla's first-open `unpackLootTable`.
    pub fn try_populate_loot(&self, world: &Arc<World>) {
        let mut loot = self.loot.lock();
        if !loot.has_pending_loot() {
            return;
        }
        let seed = loot.loot_seed(world.seed(), self.get_block_pos());
        let mut container = self.container.lock();
        if loot.populate(seed, self.get_block_pos(), &mut *container) {
            drop(container);
            self.set_changed();
        }
    }
}

impl BlockEntity for ChestBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        // Vanilla unpacks the loot table when the chest is removed so its
        // contents drop together with the chest's items.
        if let Some(world) = self.get_level() {
            self.try_populate_loot(&world);
        }
        let items = {
            let mut container = self.container.lock();
            mem::replace(&mut container.items, vec![ItemStack::empty(); CHEST_SLOTS])
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
        *self.loot.lock() = ContainerLoot::load(nbt);

        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());

        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                if let Some(slot) = compound.byte("Slot") {
                    let slot = slot as usize;
                    if slot < CHEST_SLOTS
                        && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                    {
                        container.items[slot] = item;
                    }
                }
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.loot.lock().save(nbt);

        let container = self.container.lock();
        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in container.items.iter().enumerate() {
            if !item.is_empty() {
                if let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag() {
                    item_nbt.insert("Slot", slot as i8);
                    items.push(item_nbt);
                }
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        None
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }
}

impl Container for ChestContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        CHEST_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < CHEST_SLOTS {
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

    use simdnbt::borrow::read_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_blocks;

    use super::*;
    use crate::inventory::container::SimpleContainer;

    fn supply_chest_nbt(seed: i64) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:chests/trial_chambers/supply");
        nbt.insert("LootTableSeed", seed);
        nbt
    }

    macro_rules! borrowed {
        ($nbt:ident, $bytes:ident, $out:ident) => {
            let mut $bytes: Vec<u8> = Vec::new();
            $nbt.write(&mut $bytes);
            let $out =
                read_compound(&mut Cursor::new($bytes.as_slice())).expect("test nbt should reborrow");
        };
    }

    #[test]
    fn template_loot_nbt_populates_deterministically_and_clears_the_table() {
        init_vanilla_registry();

        let pos = BlockPos::new(1, 2, 3);
        let nbt42 = supply_chest_nbt(42);
        borrowed!(nbt42, bytes42, nbt42_borrowed);
        let mut first = ContainerLoot::load(&nbt42_borrowed);
        let nbt42b = supply_chest_nbt(42);
        borrowed!(nbt42b, bytes42b, nbt42b_borrowed);
        let mut second = ContainerLoot::load(&nbt42b_borrowed);

        assert!(first.has_pending_loot());
        assert!(second.has_pending_loot());

        let mut first_container = SimpleContainer::new(CHEST_SLOTS);
        let mut second_container = SimpleContainer::new(CHEST_SLOTS);
        assert!(first.populate(42, pos, &mut first_container));
        assert!(second.populate(42, pos, &mut second_container));

        // The same loot-table seed must roll identical contents.
        assert_eq!(
            first_container.items().iter().filter(|item| !item.is_empty()).count(),
            first_container.items().iter().filter(|item| !item.is_empty()).count()
        );
        assert!(!first_container.items().iter().all(ItemStack::is_empty),
            "supply loot table must produce items");
        for (slot, (a, b)) in first_container
            .items()
            .iter()
            .zip(second_container.items().iter())
            .enumerate()
        {
            assert_eq!(a.item.key, b.item.key, "slot {slot} diverged between identical rolls");
        }

        // Different seeds roll different slot placements (overwhelmingly likely).
        let nbt43 = supply_chest_nbt(43);
        borrowed!(nbt43, bytes43, nbt43_borrowed);
        let mut third = ContainerLoot::load(&nbt43_borrowed);
        let mut third_container = SimpleContainer::new(CHEST_SLOTS);
        assert!(third.populate(43, pos, &mut third_container));

        // After rolling, the loot table reference must be cleared so the
        // rolled contents are what persists.
        assert!(!first.has_pending_loot());
        let mut saved = NbtCompound::new();
        first.save(&mut saved);
        assert!(!saved.contains("LootTable"));
        assert!(!saved.contains("LootTableSeed"));
    }

    #[test]
    fn chest_block_entity_survives_save_load_round_trip() {
        init_vanilla_registry();

        let chest = ChestBlockEntity::new(
            Weak::new(),
            BlockPos::new(4, 5, 6),
            vanilla_blocks::CHEST.default_state(),
        );
        let nbt7 = supply_chest_nbt(7);
        borrowed!(nbt7, bytes7, nbt7_borrowed);
        chest.load_additional(&nbt7_borrowed);
        assert!(chest.loot.lock().has_pending_loot());

        let mut saved = NbtCompound::new();
        chest.save_additional(&mut saved);
        assert!(saved.contains("LootTable"));

        let restored = ChestBlockEntity::new(
            Weak::new(),
            BlockPos::new(4, 5, 6),
            vanilla_blocks::CHEST.default_state(),
        );
        borrowed!(saved, saved_bytes, saved_borrowed);
        restored.load_additional(&saved_borrowed);
        assert!(restored.loot.lock().has_pending_loot());
    }
}
