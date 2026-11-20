//! Dispenser and dropper block entity: a 9-slot container that dispenses from a
//! randomly chosen non-empty slot. Vanilla `DispenserBlockEntity` (also used for droppers).

use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::world::World;

/// Number of slots in a dispenser or dropper (3x3 grid).
pub const DISPENSER_SLOTS: usize = 9;

/// Item storage of a dispenser or dropper.
pub struct DispenserContainer {
    items: Vec<ItemStack>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `DispenserContainer`.
unsafe impl DowncastType for DispenserContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/dispenser");
}

impl Container for DispenserContainer {
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

/// Dispenser (or dropper) block entity.
pub struct DispenserBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<DispenserContainer>>,
    container_ref: ContainerRef,
}

// SAFETY: This key is owned by Steel and uniquely identifies `DispenserBlockEntity`.
unsafe impl DowncastType for DispenserBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/dispenser");
}

impl DispenserBlockEntity {
    /// Creates a dispenser or dropper block entity of the given registered type.
    #[must_use]
    pub fn new(
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        let base = Arc::new(BlockEntityBase::new(block_entity_type, level, pos, state));
        let container = Arc::new(SyncMutex::new(DispenserContainer {
            items: vec![ItemStack::empty(); DISPENSER_SLOTS],
        }));
        let shared_container: SharedContainer =
            Arc::<SyncMutex<DispenserContainer>>::clone(&container);
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
        }
    }

    /// Vanilla `DispenserBlockEntity.getRandomSlot`: reservoir-samples one non-empty
    /// slot uniformly, or returns `None` when the container is empty.
    #[must_use]
    pub fn random_slot(&self) -> Option<usize> {
        let container = self.container.lock();
        let mut replace_slot = None;
        let mut replace_odds = 1;
        for (slot, item) in container.items.iter().enumerate() {
            if !item.is_empty() && rand::random_range(0..replace_odds) == 0 {
                replace_slot = Some(slot);
            }
            if !item.is_empty() {
                replace_odds += 1;
            }
        }
        replace_slot
    }

    /// Returns a copy of the stack in `slot`.
    #[must_use]
    pub fn get_slot(&self, slot: usize) -> ItemStack {
        self.container.lock().items[slot].clone()
    }

    /// Replaces the stack in `slot`.
    pub fn set_slot(&self, slot: usize, stack: ItemStack) {
        self.container.lock().items[slot] = stack;
    }
}

impl BlockEntity for DispenserBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            std::mem::replace(
                &mut container.items,
                vec![ItemStack::empty(); DISPENSER_SLOTS],
            )
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
                    if slot < DISPENSER_SLOTS
                        && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                    {
                        container.items[slot] = item;
                    }
                }
            }
        }
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
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        None
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::*;

    #[test]
    fn random_slot_is_none_when_empty_and_only_non_empty_slot_when_single() {
        init_vanilla_registry();
        let dispenser = DispenserBlockEntity::new(
            &steel_registry::vanilla_block_entity_types::DISPENSER,
            Weak::new(),
            BlockPos::new(0, 0, 0),
            steel_registry::vanilla_blocks::DISPENSER.default_state(),
        );
        assert_eq!(dispenser.random_slot(), None);

        dispenser.set_slot(4, ItemStack::with_count(&vanilla_items::STONE, 3));
        for _ in 0..32 {
            assert_eq!(dispenser.random_slot(), Some(4));
        }
    }
}
