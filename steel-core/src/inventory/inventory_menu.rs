//! The player's inventory menu.
//!
//! Slot layout (46 total):
//! - Slot 0: Crafting result
//! - Slots 1-4: 2x2 crafting grid
//! - Slots 5-8: Armor (head, chest, legs, feet)
//! - Slots 9-35: Main inventory (27 slots)
//! - Slots 36-44: Hotbar (9 slots)
//! - Slot 45: Offhand

use std::{mem, sync::Arc};

use steel_registry::data_components::vanilla_components::EquippableSlot;
use steel_registry::item_stack::ItemStack;
use steel_utils::locks::SyncMutex;

use crate::inventory::{
    SyncPlayerInv,
    container::Container,
    crafting::{CraftingContainer, ResultContainer},
    lock::{ContainerLockGuard, ContainerRef},
    menu::{Menu, MenuBehavior},
    recipe_manager,
    slot::{
        ArmorSlot, CraftingGridSlot, CraftingResultSlot, NormalSlot, Slot, SlotType,
        SyncCraftingContainer, SyncResultContainer, add_standard_inventory_slots,
    },
};
use crate::player::Player;

/// Slot indices for the inventory menu.
pub mod slots {
    /// Slot index for the crafting result (slot 0).
    pub const RESULT_SLOT: usize = 0;
    /// Start of the 2x2 crafting grid (slot 1).
    pub const CRAFT_SLOT_START: usize = 1;
    /// End of the 2x2 crafting grid (slot 5, exclusive).
    pub const CRAFT_SLOT_END: usize = 5;
    /// Start of armor slots (slot 5).
    pub const ARMOR_SLOT_START: usize = 5;
    /// End of armor slots (slot 9, exclusive).
    pub const ARMOR_SLOT_END: usize = 9;
    /// Start of main inventory (slot 9).
    pub const INV_SLOT_START: usize = 9;
    /// End of main inventory (slot 36, exclusive).
    pub const INV_SLOT_END: usize = 36;
    /// Start of hotbar (slot 36).
    pub const HOTBAR_SLOT_START: usize = 36;
    /// End of hotbar (slot 45, exclusive).
    pub const HOTBAR_SLOT_END: usize = 45;
    /// Offhand slot index (slot 45).
    pub const OFFHAND_SLOT: usize = 45;
    /// Total number of slots in the inventory menu.
    pub const TOTAL_SLOTS: usize = 46;
}

/// The player's inventory menu.
/// This is always open when no other menu is open.
pub struct InventoryMenu {
    behavior: MenuBehavior,
    /// The 2x2 crafting grid container.
    crafting_container: SyncCraftingContainer,
    /// The crafting result container.
    result_container: SyncResultContainer,
}

impl InventoryMenu {
    /// Container ID for the player inventory (always 0).
    pub const CONTAINER_ID: u8 = 0;

    /// Creates a new inventory menu for a player.
    ///
    /// The inventory container should contain:
    /// - Slots 0-35: Main inventory (hotbar 0-8, main 9-35)
    /// - Slots 36-39: Armor (feet, legs, chest, head)
    /// - Slot 40: Offhand
    pub fn new(inventory: SyncPlayerInv) -> Self {
        let mut menu_slots = Vec::with_capacity(slots::TOTAL_SLOTS);

        // Create the crafting containers
        let crafting_container: SyncCraftingContainer =
            Arc::new(SyncMutex::new(CraftingContainer::new(2, 2)));
        let result_container: SyncResultContainer =
            Arc::new(SyncMutex::new(ResultContainer::new()));

        // Slot 0: Crafting result
        menu_slots.push(SlotType::CraftingResult(CraftingResultSlot::new(
            result_container.clone(),
            crafting_container.clone(),
        )));

        // Slots 1-4: 2x2 Crafting grid
        for i in 0..4 {
            menu_slots.push(SlotType::CraftingGrid(CraftingGridSlot::new(
                crafting_container.clone(),
                result_container.clone(),
                i,
            )));
        }

        // Slots 5-8: Armor (head, chest, legs, feet)
        // Maps to inventory slots 39, 38, 37, 36
        // Order matches Java's SLOT_IDS: HEAD, CHEST, LEGS, FEET
        for (offset, slot_type) in [
            (39, EquippableSlot::Head),
            (38, EquippableSlot::Chest),
            (37, EquippableSlot::Legs),
            (36, EquippableSlot::Feet),
        ] {
            menu_slots.push(SlotType::Armor(ArmorSlot::new(
                inventory.clone(),
                offset,
                slot_type,
            )));
        }

        // Slots 9-44: Standard inventory (main inventory + hotbar)
        add_standard_inventory_slots(&mut menu_slots, &inventory);

        // Slot 45: Offhand (inventory slot 40)
        menu_slots.push(SlotType::Normal(NormalSlot::new(inventory.clone(), 40)));

        Self {
            behavior: MenuBehavior::new(menu_slots, Self::CONTAINER_ID, None),
            crafting_container,
            result_container,
        }
    }

    /// Updates the crafting result based on the current grid contents.
    /// Should be called whenever a crafting grid slot changes.
    pub fn update_crafting_result(&self) {
        let crafting = self.crafting_container.lock();
        let mut result = self.result_container.lock();
        recipe_manager::slot_changed_crafting_grid(&crafting, &mut *result, true);
    }

    /// Returns a reference to the crafting container.
    #[must_use]
    pub const fn crafting_container(&self) -> &SyncCraftingContainer {
        &self.crafting_container
    }

    /// Returns a reference to the result container.
    #[must_use]
    pub const fn result_container(&self) -> &SyncResultContainer {
        &self.result_container
    }

    /// Returns a `ContainerRef` for the crafting container.
    #[must_use]
    pub fn crafting_container_ref(&self) -> ContainerRef {
        ContainerRef::CraftingContainer(Arc::clone(&self.crafting_container))
    }

    /// Returns a `ContainerRef` for the result container.
    #[must_use]
    pub fn result_container_ref(&self) -> ContainerRef {
        ContainerRef::ResultContainer(Arc::clone(&self.result_container))
    }

    /// Returns true if the given slot index is in the hotbar or offhand.
    /// Based on Java's `InventoryMenu::isHotbarSlot`.
    #[must_use]
    pub fn is_hotbar_slot(slot: usize) -> bool {
        (slots::HOTBAR_SLOT_START..slots::HOTBAR_SLOT_END).contains(&slot)
            || slot == slots::OFFHAND_SLOT
    }

    /// Returns true if the given slot index is an armor slot.
    #[must_use]
    pub fn is_armor_slot(slot: usize) -> bool {
        (slots::ARMOR_SLOT_START..slots::ARMOR_SLOT_END).contains(&slot)
    }

    /// Returns true if the given slot index is in the main inventory.
    #[must_use]
    pub fn is_inventory_slot(slot: usize) -> bool {
        (slots::INV_SLOT_START..slots::INV_SLOT_END).contains(&slot)
    }

    /// Helper method to move items between inventory and hotbar.
    fn move_between_inventory_and_hotbar(
        &self,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        stack: &mut ItemStack,
    ) -> bool {
        if (slots::INV_SLOT_START..slots::INV_SLOT_END).contains(&slot_index) {
            // Main inventory -> hotbar (36-45)
            self.behavior.move_item_stack_to(
                guard,
                stack,
                slots::HOTBAR_SLOT_START,
                slots::HOTBAR_SLOT_END,
                false,
            )
        } else if (slots::HOTBAR_SLOT_START..slots::HOTBAR_SLOT_END).contains(&slot_index) {
            // Hotbar -> main inventory (9-36)
            self.behavior.move_item_stack_to(
                guard,
                stack,
                slots::INV_SLOT_START,
                slots::INV_SLOT_END,
                false,
            )
        } else if slot_index == slots::OFFHAND_SLOT {
            // Offhand -> inventory (9-45)
            self.behavior.move_item_stack_to(
                guard,
                stack,
                slots::INV_SLOT_START,
                slots::OFFHAND_SLOT,
                false,
            )
        } else {
            // Default: try to move to inventory
            self.behavior.move_item_stack_to(
                guard,
                stack,
                slots::INV_SLOT_START,
                slots::OFFHAND_SLOT,
                false,
            )
        }
    }
}

impl Menu for InventoryMenu {
    fn behavior(&self) -> &MenuBehavior {
        &self.behavior
    }

    fn behavior_mut(&mut self) -> &mut MenuBehavior {
        &mut self.behavior
    }

    /// Handles shift-click (quick move) for a slot.
    /// Based on Java's `InventoryMenu::quickMoveStack`.
    ///
    /// Returns the item that was originally in the slot (before any move occurred),
    /// or empty if nothing was moved.
    fn quick_move_stack(
        &mut self,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> ItemStack {
        if slot_index >= self.behavior.slots.len() {
            return ItemStack::empty();
        }

        // Get the current item from the slot
        let stack = self.behavior.slots[slot_index].get_item(guard).clone();
        if stack.is_empty() {
            return ItemStack::empty();
        }

        let clicked = stack.clone();
        let mut stack_mut = stack;

        // Determine target range based on which slot was clicked
        // This matches the Java implementation in InventoryMenu::quickMoveStack
        let moved = if slot_index == slots::RESULT_SLOT {
            // Crafting result -> inventory (9-45), prefer to fill existing stacks first
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::OFFHAND_SLOT,
                true,
            )
        } else if (slots::CRAFT_SLOT_START..slots::CRAFT_SLOT_END).contains(&slot_index) {
            // Crafting grid -> inventory (9-45)
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::OFFHAND_SLOT,
                false,
            )
        } else if (slots::ARMOR_SLOT_START..slots::ARMOR_SLOT_END).contains(&slot_index) {
            // Armor -> inventory (9-45)
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::OFFHAND_SLOT,
                false,
            )
        } else {
            // Item is in inventory/hotbar - try to equip it first
            let equippable_slot = clicked.get_equippable_slot();

            // Try to move to armor slot if it's armor
            if let Some(eq_slot) = equippable_slot {
                if eq_slot.is_humanoid_armor() {
                    // Calculate the target armor slot index based on the equipment slot
                    // Java: 8 - eqSlot.getIndex() where HEAD=0, CHEST=1, LEGS=2, FEET=3
                    let armor_slot_index = match eq_slot {
                        EquippableSlot::Head => slots::ARMOR_SLOT_START, // 5
                        EquippableSlot::Chest => slots::ARMOR_SLOT_START + 1, // 6
                        EquippableSlot::Legs => slots::ARMOR_SLOT_START + 2, // 7
                        EquippableSlot::Feet => slots::ARMOR_SLOT_START + 3, // 8
                        _ => unreachable!(),
                    };

                    // Only try to move if the target armor slot is empty
                    if self.behavior.slots[armor_slot_index].has_item(guard) {
                        // Armor slot occupied, move between inventory/hotbar
                        self.move_between_inventory_and_hotbar(guard, slot_index, &mut stack_mut)
                    } else {
                        self.behavior.move_item_stack_to(
                            guard,
                            &mut stack_mut,
                            armor_slot_index,
                            armor_slot_index + 1,
                            false,
                        )
                    }
                } else if eq_slot == EquippableSlot::Offhand {
                    // Try to move to offhand slot if empty
                    if self.behavior.slots[slots::OFFHAND_SLOT].has_item(guard) {
                        self.move_between_inventory_and_hotbar(guard, slot_index, &mut stack_mut)
                    } else {
                        self.behavior.move_item_stack_to(
                            guard,
                            &mut stack_mut,
                            slots::OFFHAND_SLOT,
                            slots::OFFHAND_SLOT + 1,
                            false,
                        )
                    }
                } else {
                    self.move_between_inventory_and_hotbar(guard, slot_index, &mut stack_mut)
                }
            } else {
                self.move_between_inventory_and_hotbar(guard, slot_index, &mut stack_mut)
            }
        };

        if !moved {
            return ItemStack::empty();
        }

        // Update the source slot with the remaining items
        self.behavior.slots[slot_index].set_item(guard, stack_mut.clone());

        // Check if unchanged
        if stack_mut.count == clicked.count {
            return ItemStack::empty();
        }

        self.behavior.slots[slot_index].set_changed(guard);

        // Call on_take for the result slot to consume ingredients
        // This must happen after set_item so the slot reflects the new state
        if slot_index == slots::RESULT_SLOT {
            if let Some(remainder) =
                self.behavior.slots[slot_index].on_take(guard, &clicked, player)
            {
                // Try to place crafting remainders (e.g., empty buckets) back in inventory
                player.add_item_or_drop_with_guard(guard, remainder);
            }

            // Java: if (slotIndex == 0) { player.drop(stack, false); }
            // Drop any items from the result slot that couldn't fit in the inventory
            if !stack_mut.is_empty() {
                player.drop_item(stack_mut, false, true);
            }
        }

        clicked
    }

    /// Returns true if the item can be taken from the slot during pickup all.
    /// Prevents taking from the crafting result slot.
    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, slot_index: usize) -> bool {
        // Can't pickup-all from the crafting result slot
        slot_index != slots::RESULT_SLOT
    }

    /// Called when the inventory menu is closed.
    /// Returns crafting grid items to the player's inventory.
    ///
    /// Java's behavior (via `placeItemBackInInventory`):
    /// 1. Try to stack with existing items (selected slot, offhand, then all slots)
    /// 2. Try to place in empty slots (hotbar first, then main inventory)
    fn removed(&mut self, player: &Player) {
        // Clear the carried item first
        let carried = mem::take(&mut self.behavior.carried);

        // If player was holding something, try to return it to inventory
        if !carried.is_empty() {
            player.add_item_or_drop(carried);
        }

        // Collect all items from crafting grid first (to release the lock)
        let crafting_items: Vec<ItemStack> = {
            let mut crafting = self.crafting_container.lock();
            (0..crafting.get_container_size())
                .map(|i| crafting.remove_item_no_update(i))
                .filter(|item| !item.is_empty())
                .collect()
        };

        // Now place collected items back in inventory
        for item in crafting_items {
            player.add_item_or_drop(item);
        }

        // Clear the result slot (it's virtual, just clear it)
        self.result_container.lock().set_item(0, ItemStack::empty());
    }
}
