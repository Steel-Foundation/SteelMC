//! The crafting table menu (3x3 crafting grid).
//!
//! Slot layout (46 total):
//! - Slot 0: Crafting result
//! - Slots 1-9: 3x3 crafting grid
//! - Slots 10-36: Main inventory (27 slots)
//! - Slots 37-45: Hotbar (9 slots)

use std::{mem, sync::Arc};

use steel_registry::item_stack::ItemStack;
use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, translations};
use text_components::TextComponent;

use crate::inventory::{
    SyncPlayerInv,
    container::Container,
    crafting::{CraftingContainer, ResultContainer},
    lock::{ContainerLockGuard, ContainerRef},
    menu::{Menu, MenuBehavior},
    menu_provider::{MenuInstance, MenuProvider},
    slot::{
        CraftingGridSlot, CraftingResultSlot, Slot, SlotType, SyncCraftingContainer,
        SyncResultContainer, add_standard_inventory_slots,
    },
};
use crate::player::Player;

/// Slot indices for the crafting menu.
pub mod slots {
    /// Slot index for the crafting result (slot 0).
    pub const RESULT_SLOT: usize = 0;
    /// Start of the 3x3 crafting grid (slot 1).
    pub const CRAFT_SLOT_START: usize = 1;
    /// End of the 3x3 crafting grid (slot 10, exclusive).
    pub const CRAFT_SLOT_END: usize = 10;
    /// Start of main inventory (slot 10).
    pub const INV_SLOT_START: usize = 10;
    /// End of main inventory (slot 37, exclusive).
    pub const INV_SLOT_END: usize = 37;
    /// Start of hotbar (slot 37).
    pub const HOTBAR_SLOT_START: usize = 37;
    /// End of hotbar (slot 46, exclusive).
    pub const HOTBAR_SLOT_END: usize = 46;
    /// Total number of slots in the crafting menu.
    pub const TOTAL_SLOTS: usize = 46;
}

/// The crafting table menu with a 3x3 crafting grid.
///
/// Based on Java's `CraftingMenu`.
pub struct CraftingMenu {
    behavior: MenuBehavior,
    /// The 3x3 crafting grid container.
    crafting_container: SyncCraftingContainer,
    /// The crafting result container.
    result_container: SyncResultContainer,
    /// The position of the crafting table block.
    block_pos: BlockPos,
}

impl CraftingMenu {
    /// Creates a new crafting menu for a player.
    ///
    /// # Arguments
    /// * `inventory` - The player's inventory
    /// * `container_id` - The container ID for this menu (1-100)
    /// * `block_pos` - The position of the crafting table block
    #[must_use]
    pub fn new(inventory: SyncPlayerInv, container_id: u8, block_pos: BlockPos) -> Self {
        let mut menu_slots = Vec::with_capacity(slots::TOTAL_SLOTS);

        // Create the crafting containers
        let crafting_container: SyncCraftingContainer =
            Arc::new(SyncMutex::new(CraftingContainer::new(3, 3)));
        let result_container: SyncResultContainer =
            Arc::new(SyncMutex::new(ResultContainer::new()));

        // Slot 0: Crafting result
        menu_slots.push(SlotType::CraftingResult(CraftingResultSlot::new_3x3(
            result_container.clone(),
            crafting_container.clone(),
        )));

        // Slots 1-9: 3x3 Crafting grid
        for i in 0..9 {
            menu_slots.push(SlotType::CraftingGrid(CraftingGridSlot::new_3x3(
                crafting_container.clone(),
                result_container.clone(),
                i,
            )));
        }

        // Slots 10-45: Standard inventory (main inventory + hotbar)
        add_standard_inventory_slots(&mut menu_slots, &inventory);

        Self {
            behavior: MenuBehavior::new(
                menu_slots,
                container_id,
                Some(vanilla_menu_types::CRAFTING),
            ),
            crafting_container,
            result_container,
            block_pos,
        }
    }

    /// Returns the menu type for the crafting table.
    #[must_use]
    pub fn menu_type() -> MenuTypeRef {
        vanilla_menu_types::CRAFTING
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

    /// Returns the position of the crafting table block.
    #[must_use]
    pub const fn block_pos(&self) -> BlockPos {
        self.block_pos
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
}

impl Menu for CraftingMenu {
    fn behavior(&self) -> &MenuBehavior {
        &self.behavior
    }

    fn behavior_mut(&mut self) -> &mut MenuBehavior {
        &mut self.behavior
    }

    /// Handles shift-click (quick move) for a slot.
    ///
    /// Based on Java's `CraftingMenu::quickMoveStack`:
    /// - Result slot (0) -> inventory (10-46), prefer existing stacks
    /// - Crafting grid (1-9) -> inventory (10-46)
    /// - Inventory (10-36) -> crafting grid (1-9), then hotbar (37-45)
    /// - Hotbar (37-45) -> crafting grid (1-9), then inventory (10-36)
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

        let moved = if slot_index == slots::RESULT_SLOT {
            // Result slot -> inventory (10-46), prefer to fill existing stacks first (backwards)
            // Java: moveItemStackTo(stack, 10, 46, true)
            // Also calls onCraftedBy for achievements
            if !self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::HOTBAR_SLOT_END,
                true,
            ) {
                return ItemStack::empty();
            }
            // slot.onQuickCraft is handled by on_take below
            true
        } else if (slots::INV_SLOT_START..slots::HOTBAR_SLOT_END).contains(&slot_index) {
            // Inventory or hotbar -> try crafting grid first, then other inventory section
            // Java: moveItemStackTo(stack, 1, 10, false) first
            if !self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::CRAFT_SLOT_START,
                slots::CRAFT_SLOT_END,
                false,
            ) {
                // Then try the other inventory section
                if slot_index < slots::HOTBAR_SLOT_START {
                    // Main inventory -> hotbar
                    // Java: moveItemStackTo(stack, 37, 46, false)
                    if !self.behavior.move_item_stack_to(
                        guard,
                        &mut stack_mut,
                        slots::HOTBAR_SLOT_START,
                        slots::HOTBAR_SLOT_END,
                        false,
                    ) {
                        return ItemStack::empty();
                    }
                } else {
                    // Hotbar -> main inventory
                    // Java: moveItemStackTo(stack, 10, 37, false)
                    if !self.behavior.move_item_stack_to(
                        guard,
                        &mut stack_mut,
                        slots::INV_SLOT_START,
                        slots::HOTBAR_SLOT_START,
                        false,
                    ) {
                        return ItemStack::empty();
                    }
                }
            }
            true
        } else if (slots::CRAFT_SLOT_START..slots::CRAFT_SLOT_END).contains(&slot_index) {
            // Crafting grid -> inventory (10-46)
            // Java: moveItemStackTo(stack, 10, 46, false)
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::HOTBAR_SLOT_END,
                false,
            )
        } else {
            false
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
        if slot_index == slots::RESULT_SLOT {
            if let Some(remainder) =
                self.behavior.slots[slot_index].on_take(guard, &clicked, player)
            {
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
        slot_index != slots::RESULT_SLOT
    }

    /// Returns true if the player is still within range of the crafting table.
    ///
    /// Based on Java's `CraftingMenu::stillValid` which checks:
    /// 1. The block at the position is still a crafting table
    /// 2. The player is within 8 blocks (4.0 * 2 = 8 block interaction range + 4)
    fn still_valid(&self) -> bool {
        // Note: We check this via the world in handle_container_click
        // The actual distance check happens there
        true
    }

    /// Called when the crafting menu is closed.
    /// Returns crafting grid items to the player's inventory.
    ///
    /// Based on Java's `CraftingMenu::removed` which calls `clearContainer`.
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

impl MenuInstance for CraftingMenu {
    fn menu_type(&self) -> MenuTypeRef {
        vanilla_menu_types::CRAFTING
    }

    fn container_id(&self) -> u8 {
        self.behavior.container_id
    }
}

/// Provider for creating a crafting menu.
pub struct CraftingMenuProvider {
    inventory: SyncPlayerInv,
    pos: BlockPos,
}

impl CraftingMenuProvider {
    /// Creates a new crafting menu provider.
    #[must_use]
    pub const fn new(inventory: SyncPlayerInv, pos: BlockPos) -> Self {
        Self { inventory, pos }
    }
}

impl MenuProvider for CraftingMenuProvider {
    fn title(&self) -> TextComponent {
        TextComponent::translated(translations::CONTAINER_CRAFTING.msg())
    }

    fn create(&self, container_id: u8) -> Box<dyn MenuInstance> {
        Box::new(CraftingMenu::new(
            self.inventory.clone(),
            container_id,
            self.pos,
        ))
    }
}
