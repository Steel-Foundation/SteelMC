//! The chest menu for chest-like containers (chests, barrels, ender chests, shulker boxes).
//!
//! Supports 1-6 rows of 9 slots each. The slot layout is:
//! - Slots 0 to `rows * 9 - 1`: Container slots
//! - Slots `rows * 9` to `rows * 9 + 26`: Main inventory (27 slots)
//! - Slots `rows * 9 + 27` to `rows * 9 + 35`: Hotbar (9 slots)

use std::mem;

use steel_registry::item_stack::ItemStack;
use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;
use text_components::TextComponent;

use crate::inventory::{
    SyncPlayerInv,
    lock::{ContainerLockGuard, ContainerRef},
    menu::{Menu, MenuBehavior},
    menu_provider::{MenuInstance, MenuProvider},
    slot::{NormalSlot, Slot, SlotType, add_standard_inventory_slots},
};
use crate::player::Player;

/// Number of slots per row in a chest menu.
pub const SLOTS_PER_ROW: usize = 9;

/// Slot index helpers for chest menus.
pub mod slots {
    use super::SLOTS_PER_ROW;

    /// Returns the number of container slots for a given row count.
    #[must_use]
    pub const fn container_slot_count(rows: usize) -> usize {
        rows * SLOTS_PER_ROW
    }

    /// Returns the start index of the main inventory slots.
    #[must_use]
    pub const fn inv_slot_start(rows: usize) -> usize {
        container_slot_count(rows)
    }

    /// Returns the end index (exclusive) of the main inventory slots.
    #[must_use]
    pub const fn inv_slot_end(rows: usize) -> usize {
        inv_slot_start(rows) + 27
    }

    /// Returns the start index of the hotbar slots.
    #[must_use]
    pub const fn hotbar_slot_start(rows: usize) -> usize {
        inv_slot_end(rows)
    }

    /// Returns the end index (exclusive) of the hotbar slots (total slot count).
    #[must_use]
    pub const fn hotbar_slot_end(rows: usize) -> usize {
        hotbar_slot_start(rows) + 9
    }

    /// Returns the total number of slots for a given row count.
    #[must_use]
    pub const fn total_slots(rows: usize) -> usize {
        hotbar_slot_end(rows)
    }
}

/// A menu for chest-like containers.
///
/// This menu is used for chests (3 rows), double chests (6 rows), barrels (3 rows),
/// ender chests (3 rows), and shulker boxes (3 rows).
///
/// Based on Java's `ChestMenu`.
pub struct ChestMenu {
    behavior: MenuBehavior,
    /// Reference to the container (chest, barrel, etc.).
    container: ContainerRef,
    /// Number of rows in the container (1-6).
    rows: usize,
}

impl ChestMenu {
    /// Creates a new chest menu with the specified number of rows.
    ///
    /// # Arguments
    /// * `inventory` - The player's inventory
    /// * `container_id` - The container ID for this menu (1-100)
    /// * `container` - Reference to the container (chest, barrel, etc.)
    /// * `rows` - Number of rows (1-6)
    ///
    /// # Panics
    /// Panics if `rows` is 0 or greater than 6.
    #[must_use]
    pub fn new(
        inventory: SyncPlayerInv,
        container_id: u8,
        container: ContainerRef,
        rows: usize,
    ) -> Self {
        assert!(
            (1..=6).contains(&rows),
            "Chest rows must be between 1 and 6"
        );

        let container_slots = slots::container_slot_count(rows);
        let total_slots = slots::total_slots(rows);
        let mut menu_slots = Vec::with_capacity(total_slots);

        // Add container slots (0 to rows * 9 - 1)
        for i in 0..container_slots {
            menu_slots.push(SlotType::Normal(NormalSlot::new(container.clone(), i)));
        }

        // Add standard inventory slots (main inventory + hotbar)
        add_standard_inventory_slots(&mut menu_slots, &inventory);

        Self {
            behavior: MenuBehavior::new(
                menu_slots,
                container_id,
                Some(Self::menu_type_for_rows(rows)),
            ),
            container,
            rows,
        }
    }

    /// Creates a 1-row chest menu.
    #[must_use]
    pub fn one_row(inventory: SyncPlayerInv, container_id: u8, container: ContainerRef) -> Self {
        Self::new(inventory, container_id, container, 1)
    }

    /// Creates a 2-row chest menu.
    #[must_use]
    pub fn two_rows(inventory: SyncPlayerInv, container_id: u8, container: ContainerRef) -> Self {
        Self::new(inventory, container_id, container, 2)
    }

    /// Creates a 3-row chest menu (standard chest, barrel, ender chest, shulker box).
    #[must_use]
    pub fn three_rows(inventory: SyncPlayerInv, container_id: u8, container: ContainerRef) -> Self {
        Self::new(inventory, container_id, container, 3)
    }

    /// Creates a 4-row chest menu.
    #[must_use]
    pub fn four_rows(inventory: SyncPlayerInv, container_id: u8, container: ContainerRef) -> Self {
        Self::new(inventory, container_id, container, 4)
    }

    /// Creates a 5-row chest menu.
    #[must_use]
    pub fn five_rows(inventory: SyncPlayerInv, container_id: u8, container: ContainerRef) -> Self {
        Self::new(inventory, container_id, container, 5)
    }

    /// Creates a 6-row chest menu (double chest).
    #[must_use]
    pub fn six_rows(inventory: SyncPlayerInv, container_id: u8, container: ContainerRef) -> Self {
        Self::new(inventory, container_id, container, 6)
    }

    /// Returns the appropriate menu type for the given row count.
    ///
    /// # Panics
    /// Panics if `rows` is 0 or greater than 6.
    #[must_use]
    pub fn menu_type_for_rows(rows: usize) -> MenuTypeRef {
        match rows {
            1 => vanilla_menu_types::GENERIC_9X1,
            2 => vanilla_menu_types::GENERIC_9X2,
            3 => vanilla_menu_types::GENERIC_9X3,
            4 => vanilla_menu_types::GENERIC_9X4,
            5 => vanilla_menu_types::GENERIC_9X5,
            6 => vanilla_menu_types::GENERIC_9X6,
            _ => panic!("Invalid row count: {rows}"),
        }
    }

    /// Returns the number of rows in this chest menu.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// Returns a reference to the container.
    #[must_use]
    pub const fn container(&self) -> &ContainerRef {
        &self.container
    }
}

impl Menu for ChestMenu {
    fn behavior(&self) -> &MenuBehavior {
        &self.behavior
    }

    fn behavior_mut(&mut self) -> &mut MenuBehavior {
        &mut self.behavior
    }

    /// Handles shift-click (quick move) for a slot.
    ///
    /// Based on Java's `ChestMenu::quickMoveStack`:
    /// - Container slots (< rows * 9) -> player inventory (backwards = true)
    /// - Player inventory slots -> container (backwards = false)
    fn quick_move_stack(
        &mut self,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        _player: &Player,
    ) -> ItemStack {
        if slot_index >= self.behavior.slots.len() {
            return ItemStack::empty();
        }

        let slot = &self.behavior.slots[slot_index];
        let stack = slot.get_item(guard).clone();
        if stack.is_empty() {
            return ItemStack::empty();
        }

        let clicked = stack.clone();
        let mut stack_mut = stack;

        let container_slots = slots::container_slot_count(self.rows);
        let total_slots = self.behavior.slots.len();

        let moved = if slot_index < container_slots {
            // Container slot -> player inventory
            // Use backwards = true to prefer filling existing stacks first
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                container_slots,
                total_slots,
                true,
            )
        } else {
            // Player inventory -> container
            // Use backwards = false for forward iteration
            self.behavior
                .move_item_stack_to(guard, &mut stack_mut, 0, container_slots, false)
        };

        if !moved {
            return ItemStack::empty();
        }

        // Update the source slot with remaining items
        self.behavior.slots[slot_index].set_item(guard, stack_mut.clone());

        // Check if unchanged
        if stack_mut.count == clicked.count {
            return ItemStack::empty();
        }

        self.behavior.slots[slot_index].set_changed(guard);

        clicked
    }

    /// Returns true if the container is still valid for interaction.
    ///
    /// Delegates to the container's `still_valid` method.
    fn still_valid(&self) -> bool {
        let guard = self.behavior.lock_all_containers();
        guard
            .get(self.container.container_id())
            .is_some_and(super::container::Container::still_valid)
    }

    /// Called when the menu is closed.
    ///
    /// Drops the carried item (default behavior).
    /// Note: Java's `ChestMenu::removed` also calls `container.stopOpen(player)`,
    /// but we don't have that callback implemented yet.
    fn removed(&mut self, player: &Player) {
        let carried = mem::take(&mut self.behavior.carried);
        if !carried.is_empty() {
            player.drop_item(carried, false, true);
        }
    }
}

impl MenuInstance for ChestMenu {
    fn menu_type(&self) -> MenuTypeRef {
        Self::menu_type_for_rows(self.rows)
    }

    fn container_id(&self) -> u8 {
        self.behavior.container_id
    }
}

/// Provider for creating chest menus.
pub struct ChestMenuProvider {
    inventory: SyncPlayerInv,
    container: ContainerRef,
    rows: usize,
    title: TextComponent,
}

impl ChestMenuProvider {
    /// Creates a new chest menu provider.
    ///
    /// # Arguments
    /// * `inventory` - The player's inventory
    /// * `container` - Reference to the container
    /// * `rows` - Number of rows (1-6)
    /// * `title` - Display title for the menu
    #[must_use]
    pub const fn new(
        inventory: SyncPlayerInv,
        container: ContainerRef,
        rows: usize,
        title: TextComponent,
    ) -> Self {
        Self {
            inventory,
            container,
            rows,
            title,
        }
    }

    /// Creates a provider for a 3-row chest menu (standard chest).
    #[must_use]
    pub const fn three_rows(
        inventory: SyncPlayerInv,
        container: ContainerRef,
        title: TextComponent,
    ) -> Self {
        Self::new(inventory, container, 3, title)
    }

    /// Creates a provider for a 6-row chest menu (double chest).
    #[must_use]
    pub const fn six_rows(
        inventory: SyncPlayerInv,
        container: ContainerRef,
        title: TextComponent,
    ) -> Self {
        Self::new(inventory, container, 6, title)
    }
}

impl MenuProvider for ChestMenuProvider {
    fn title(&self) -> TextComponent {
        self.title.clone()
    }

    fn create(&self, container_id: u8) -> Box<dyn MenuInstance> {
        Box::new(ChestMenu::new(
            self.inventory.clone(),
            container_id,
            self.container.clone(),
            self.rows,
        ))
    }
}
