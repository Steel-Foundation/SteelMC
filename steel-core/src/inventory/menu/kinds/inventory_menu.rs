//! Player inventory menu.
//!
//! Slot layout (46 total):
//! - Slot 0: Crafting result
//! - Slots 1-4: 2x2 grid
//! - Slots 5-8: Armor (head, chest, legs, feet)
//! - Slots 9-35: Main inventory (27)
//! - Slots 36-44: Hotbar (9)
//! - Slot 45: Offhand

use steel_registry::item_stack::ItemStack;
use steel_utils::locks::{IntoShared, Shared};

use crate::inventory::container::{CraftingContainer, ResultContainer};
use crate::inventory::prelude::*;
use crate::inventory::slots::{ArmorSlot, CraftingHandler};
use crate::player::Player;
use crate::player::player_inventory::{PlayerInventory, armor_equipment};

/// Container ID for the player inventory (always 0).
pub const INVENTORY_MENU_CONTAINER_ID: u8 = 0;

/// Builds the player inventory menu, always open when no other menu is.
///
/// The inventory container should contain:
/// - Slots 0-35: Main inventory (hotbar 0-8, main 9-35)
/// - Slots 36-39: Armor (feet, legs, chest, head)
/// - Slot 40: Offhand
#[must_use]
pub fn inventory_menu(inventory: Shared<PlayerInventory>) -> Menu {
    let crafting_container = CraftingContainer::new(2, 2).into_shared();
    let result_container = ResultContainer::new().into_shared();

    let handler = CraftingHandler::new(crafting_container.clone(), result_container.clone(), 2);

    let mut builder = MenuBuilder::new(None, INVENTORY_MENU_CONTAINER_ID);

    let result = builder.result_slot(handler.clone());
    let grid = builder.section_all(crafting_container);
    let armor = builder.section_at(
        &inventory,
        PlayerInventory::ARMOR_TOP_DOWN,
        SectionKind::custom(|container, index| {
            Box::new(ArmorSlot::new(
                container.clone(),
                index,
                armor_equipment(index),
            ))
        }),
    );
    let player = builder.player_inventory(&inventory);
    let offhand = builder.section_at(
        &inventory,
        [PlayerInventory::SLOT_OFFHAND],
        SectionKind::Normal,
    );

    // No routes: quick_move is a custom override. The grid drains on close.
    builder.drain(grid);

    builder.build(InventoryKind {
        result_container,
        handler,
        result,
        grid,
        armor,
        inv: player.all(),
        main: player.main(),
        hotbar: player.hotbar(),
        offhand,
    })
}

/// Per-menu player-inventory state: recipe handler, result container, and the
/// section handles for its custom shift-click.
pub struct InventoryKind {
    /// The result container.
    result_container: Shared<ResultContainer>,
    handler: CraftingHandler,
    /// The result (slot 0).
    result: Section,
    /// The 2x2 grid (slots 1-4).
    grid: Section,
    /// Armor slots (slots 5-8).
    armor: Section,
    /// Main inventory + hotbar (slots 9-44).
    inv: Section,
    /// Main inventory (slots 9-35).
    main: Section,
    /// Hotbar (slots 36-44).
    hotbar: Section,
    /// Offhand (slot 45).
    offhand: Section,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for InventoryKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/inventory");
}

impl InventoryKind {
    /// `ContainerId` of the 2x2 grid.
    pub(crate) fn crafting_id(&self) -> ContainerId {
        self.handler.crafting_id()
    }

    /// Shared handle to the 2x2 grid container.
    pub(crate) fn crafting_container(&self) -> Shared<CraftingContainer> {
        self.handler.crafting_container()
    }

    /// Shared recipe handler for the 2x2 crafting grid and its result.
    pub(crate) fn crafting_handler(&self) -> CraftingHandler {
        self.handler.clone()
    }

    /// Recomputes the result from the current grid contents.
    pub(crate) fn update_result(&self, guard: &mut ContainerLockGuard) {
        self.handler.update_result(guard);
    }

    /// Moves items between the main inventory and hotbar.
    fn move_between_inventory_and_hotbar(
        &self,
        behavior: &MenuBehavior,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        stack: &mut ItemStack,
    ) -> bool {
        if self.main.contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                stack,
                self.hotbar.start(),
                self.hotbar.end(),
                FillDirection::Forward,
            )
        } else if self.hotbar.contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                stack,
                self.main.start(),
                self.main.end(),
                FillDirection::Forward,
            )
        } else {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                stack,
                self.inv.start(),
                self.inv.end(),
                FillDirection::Forward,
            )
        }
    }
}

impl MenuKind for InventoryKind {
    /// Handles shift-click for a slot, including armor/offhand auto-equip.
    ///
    /// Always returns `Some`: the item originally in the slot, or empty if
    /// nothing moved.
    #[expect(
        clippy::too_many_lines,
        reason = "mirrors Java's InventoryMenu::quickMoveStack branch structure"
    )]
    fn quick_move(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> Option<ItemStack> {
        if slot_index >= behavior.slots().len() {
            return Some(ItemStack::empty());
        }

        let stack = behavior.slots()[slot_index].get_item(guard).clone();
        if stack.is_empty() {
            return Some(ItemStack::empty());
        }
        if self.result.contains(slot_index)
            && !behavior.slots()[slot_index].may_pickup(guard, player)
        {
            return Some(ItemStack::empty());
        }

        let clicked = stack.clone();
        let mut stack_mut = stack;

        // Target range depends on the clicked slot.
        let moved = if self.result.contains(slot_index) {
            // Result to inventory, filling existing stacks first.
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut stack_mut,
                self.inv.start(),
                self.inv.end(),
                FillDirection::Backward,
            )
        } else if self.grid.contains(slot_index) || self.armor.contains(slot_index) {
            // Grid or armor to inventory.
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut stack_mut,
                self.inv.start(),
                self.inv.end(),
                FillDirection::Forward,
            )
        } else {
            // Item is in inventory/hotbar, try to equip it first.
            let equippable_slot = clicked.get_equippable_slot();

            if let Some(eq_slot) = equippable_slot {
                if eq_slot.slot_type() == EquipmentSlotType::HumanoidArmor {
                    // Armor slots are ordered head, chest, legs, feet.
                    let armor_slot_index = self.armor.start()
                        + match eq_slot {
                            EquipmentSlot::Head => 0,
                            EquipmentSlot::Chest => 1,
                            EquipmentSlot::Legs => 2,
                            EquipmentSlot::Feet => 3,
                            _ => unreachable!(),
                        };

                    if behavior.slots()[armor_slot_index].has_item(guard) {
                        self.move_between_inventory_and_hotbar(
                            behavior,
                            guard,
                            slot_index,
                            &mut stack_mut,
                        )
                    } else {
                        behavior.move_item_stack_to(
                            guard,
                            slot_index,
                            &mut stack_mut,
                            armor_slot_index,
                            armor_slot_index + 1,
                            FillDirection::Forward,
                        )
                    }
                } else if eq_slot == EquipmentSlot::OffHand {
                    if behavior.slots()[self.offhand.start()].has_item(guard) {
                        self.move_between_inventory_and_hotbar(
                            behavior,
                            guard,
                            slot_index,
                            &mut stack_mut,
                        )
                    } else {
                        behavior.move_item_stack_to(
                            guard,
                            slot_index,
                            &mut stack_mut,
                            self.offhand.start(),
                            self.offhand.end(),
                            FillDirection::Forward,
                        )
                    }
                } else {
                    self.move_between_inventory_and_hotbar(
                        behavior,
                        guard,
                        slot_index,
                        &mut stack_mut,
                    )
                }
            } else {
                self.move_between_inventory_and_hotbar(behavior, guard, slot_index, &mut stack_mut)
            }
        };

        if !moved {
            return Some(ItemStack::empty());
        }

        behavior.update_quick_move_source(guard, slot_index, &stack_mut, &clicked);

        if stack_mut.count == clicked.count {
            return Some(ItemStack::empty());
        }

        if let Some(remainder) = behavior.slots()[slot_index].on_take(guard, &mut stack_mut, player)
        {
            // Crafting remainders like empty buckets go back to the inventory.
            player.add_item_or_drop_with_guard(guard, remainder);
        }

        if self.result.contains(slot_index) {
            // Drop result output that didn't fit.
            if !stack_mut.is_empty() {
                let _ = guard.run_unlocked(|| player.drop_item(stack_mut, false, false));
            }
        }

        Some(clicked)
    }

    /// Prevents taking from the result slot during pickup-all.
    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, slot_index: usize) -> bool {
        !self.result.contains(slot_index)
    }

    /// Clears the virtual result on close. The grid is drained by [`Menu::removed`].
    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.result_container.lock().set_item(0, ItemStack::empty());
    }

    fn slots_changed(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.handler.update_result(guard);
    }
}

#[cfg(test)]
mod tests;
