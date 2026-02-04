//! Barrel block entity implementation.
//!
//! Barrels are container block entities with 27 slots (3x9 grid),
//! functioning similarly to chests but without double-chest behavior.

use std::any::Any;
use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::block_entity::BlockEntity;
use crate::inventory::container::Container;
use crate::world::World;

/// Number of slots in a barrel (3 rows of 9).
pub const BARREL_SLOTS: usize = 27;

/// Barrel block entity.
///
/// A simple container with 27 slots, using the same menu as chests.
pub struct BarrelBlockEntity {
    /// Weak reference to the world for marking chunks dirty.
    level: Weak<World>,
    /// Position in the world.
    pos: BlockPos,
    /// Current block state.
    state: BlockStateId,
    /// Whether this entity has been marked for removal.
    removed: bool,
    /// The 27 item slots.
    items: Vec<ItemStack>,
}

impl BarrelBlockEntity {
    /// Creates a new barrel block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            level,
            pos,
            state,
            removed: false,
            items: vec![ItemStack::empty(); BARREL_SLOTS],
        }
    }
}

impl BlockEntity for BarrelBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> BlockEntityTypeRef {
        vanilla_block_entity_types::BARREL
    }

    fn get_block_pos(&self) -> BlockPos {
        self.pos
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    fn pre_remove_side_effects(&mut self, pos: BlockPos, _state: BlockStateId) {
        // Drop all items when the barrel is broken
        if let Some(world) = self.level.upgrade() {
            for item in self.items.drain(..) {
                world.drop_item_stack(pos, item);
            }
        }
    }

    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to NbtCompound view for accessing methods
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();

        // Load items from NBT using borrowed NBT for proper ItemStack parsing
        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                // Each item has a "Slot" byte and item data
                if let Some(slot) = compound.byte("Slot") {
                    let slot = slot as usize;
                    if slot < BARREL_SLOTS {
                        // Parse item directly from the borrowed compound
                        if let Some(item) = ItemStack::from_borrowed_compound(&compound) {
                            self.items[slot] = item;
                        }
                    }
                }
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Save items to NBT (only non-empty slots)
        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in self.items.iter().enumerate() {
            if !item.is_empty() {
                // Use ItemStack's ToNbtTag implementation for proper component serialization
                if let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag() {
                    item_nbt.insert("Slot", slot as i8);
                    items.push(item_nbt);
                }
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Barrels don't need to send inventory to clients on chunk load
        // (unlike signs which display text)
        None
    }

    fn as_container(&self) -> Option<&(dyn Container + 'static)> {
        Some(self)
    }

    fn as_container_mut(&mut self) -> Option<&mut (dyn Container + 'static)> {
        Some(self)
    }
}

impl Container for BarrelBlockEntity {
    fn get_container_size(&self) -> usize {
        BARREL_SLOTS
    }

    fn get_item(&self, slot: usize) -> &ItemStack {
        &self.items[slot]
    }

    fn get_item_mut(&mut self, slot: usize) -> &mut ItemStack {
        &mut self.items[slot]
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if slot < BARREL_SLOTS {
            self.items[slot] = stack;
            self.set_changed();
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        64
    }

    fn set_changed(&mut self) {
        BlockEntity::set_changed(self);
    }
}
