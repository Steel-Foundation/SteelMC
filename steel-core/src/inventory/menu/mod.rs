//! Contains the Menu API

mod behavior;
mod builder;
mod grid;
mod kind;
pub mod kinds;
mod layout;

use crate::inventory::container::Container as _;
pub use behavior::MenuBehavior;
pub use builder::{
    ContainerSlots, DataSlot, FakeResultRemainderPolicy, FillDirection, IntoSections, MenuBuilder,
    PlayerInventorySections, Section, SectionKind, SectionSource, SlotFactory,
};
pub use grid::{ColSpan, GridPlacer, PlacementBuilder, Rect, Region, RowSpan, SpanBounds};
pub use kind::MenuKind;
pub(crate) use layout::MenuLayout;
#[cfg(test)]
use steel_utils::locks::Shared;

use std::fmt;
use std::mem;

use steel_registry::{item_stack::ItemStack, menu_type::MenuTypeRef};
use steel_utils::{Downcast as _, types::GameType};

use crate::inventory::container::CraftingContainer;
use crate::inventory::menu::kinds::InventoryKind;
use crate::{
    inventory::lock::{ContainerId, ContainerLockGuard, ContainerRef},
    player::Player,
};

use crate::inventory::click::{Click, ClickOutcome, SwapTarget, can_item_quick_replace};

/// A menu opened by a player: the shared click machinery plus one
/// [`MenuKind`].
///
/// The single concrete menu type. It owns the [`MenuBehavior`], the
/// `MenuLayout`, and a boxed [`MenuKind`]. Click handlers are inherent methods.
pub struct Menu {
    behavior: MenuBehavior,
    layout: MenuLayout,
    kind: Box<dyn MenuKind>,
    overrides_player_slots: bool,
}

impl fmt::Debug for Menu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Menu")
            .field("behavior", &self.behavior)
            .field("kind", &self.kind.downcast_type_key())
            .finish_non_exhaustive()
    }
}

impl Menu {
    /// Assembles a menu from its parts.
    pub(super) fn from_parts(
        behavior: MenuBehavior,
        layout: MenuLayout,
        kind: Box<dyn MenuKind>,
        overrides_player_slots: bool,
    ) -> Self {
        Self {
            behavior,
            layout,
            kind,
            overrides_player_slots,
        }
    }

    /// Returns a reference to the shared menu behavior.
    #[must_use]
    pub const fn behavior(&self) -> &MenuBehavior {
        &self.behavior
    }

    /// Returns a mutable reference to the shared menu behavior.
    pub const fn behavior_mut(&mut self) -> &mut MenuBehavior {
        &mut self.behavior
    }

    /// Returns a reference to this menu's kind.
    #[must_use]
    pub fn kind(&self) -> &dyn MenuKind {
        self.kind.as_ref()
    }

    /// Returns a mutable reference to this menu's kind.
    pub fn kind_mut(&mut self) -> &mut dyn MenuKind {
        self.kind.as_mut()
    }

    /// The container ID for this menu (0 for the player inventory).
    #[must_use]
    pub const fn container_id(&self) -> u8 {
        self.behavior.container_id()
    }

    /// The menu type for the open-screen packet, or `None` for the player's own
    /// inventory.
    #[must_use]
    pub const fn menu_type(&self) -> Option<MenuTypeRef> {
        self.behavior.menu_type()
    }

    /// Returns whether this menu paints over the client's standard player slots.
    #[must_use]
    pub const fn overrides_player_slots(&self) -> bool {
        self.overrides_player_slots
    }

    /// Returns true if this menu is still valid for the player.
    #[must_use]
    pub fn still_valid(&self, player: &Player) -> bool {
        self.kind.still_valid(&self.behavior, player)
    }

    /// Returns true if the item can be taken from the slot during pickup all.
    #[must_use]
    pub fn can_take_item_for_pick_all(&self, carried: &ItemStack, slot_index: usize) -> bool {
        self.kind.can_take_item_for_pick_all(carried, slot_index)
    }

    /// Called when the menu is closed. Hands the carried item and input sections
    /// back to the player, then runs the kind's cleanup. Items drop into the
    /// world if the player can't take them (see
    /// [`Player::returns_menu_items_to_inventory`]).
    pub fn removed(&mut self, player: &Player) {
        let return_to_inventory = player.returns_menu_items_to_inventory();

        let carried = mem::take(self.behavior.carried_mut());
        if !carried.is_empty() {
            if return_to_inventory {
                player.add_item_or_drop(carried);
            } else {
                let _ = player.drop_item(carried, false, false);
            }
        }
        self.layout
            .return_drained_items(&self.behavior, player, return_to_inventory);

        let Self { behavior, kind, .. } = self;
        kind.removed(behavior, player);
    }

    /// Applies a client rename to the menu's kind. A no-op for kinds without a
    /// rename input.
    pub fn set_item_name(&mut self, name: impl Into<String>, player: &Player) {
        let Self { behavior, kind, .. } = self;
        kind.on_rename(behavior, name.into(), player);
    }

    /// Clears or counts crafting-grid items in the base inventory menu,
    /// returning the number cleared or counted. Returns 0 for any other menu.
    pub(crate) fn clear_or_count_crafting_items(
        &mut self,
        predicate: &dyn Fn(&ItemStack) -> bool,
        amount_to_remove: i32,
        counting_only: bool,
    ) -> i32 {
        let Some(kind) = self.kind.downcast_ref::<InventoryKind>() else {
            return 0;
        };
        let crafting_id = kind.crafting_id();
        let mut guard = self.behavior.lock_all_containers();
        let Some(crafting) = guard.get_typed_mut::<CraftingContainer>(crafting_id) else {
            return 0;
        };

        crafting.clear_or_count_matching_items(predicate, amount_to_remove, counting_only)
    }

    /// A shared handle to the base inventory menu's 2x2 crafting grid, or `None`
    /// for any other menu.
    #[cfg(test)]
    pub(crate) fn crafting_container(&self) -> Option<Shared<CraftingContainer>> {
        let kind = self.kind.downcast_ref::<InventoryKind>()?;
        Some(kind.crafting_container())
    }

    /// Recomputes the base inventory menu's crafting result. A no-op for any
    /// other menu.
    pub(crate) fn update_crafting_result(&mut self) {
        let Some(kind) = self.kind.downcast_mut::<InventoryKind>() else {
            return;
        };
        let mut guard = self.behavior.lock_all_containers();
        kind.update_result(&mut guard);
    }

    /// Recomputes recipe-driven slots after a change (delegates to the kind).
    fn slots_changed(&mut self, guard: &mut ContainerLockGuard, player: &Player) {
        let Self { behavior, kind, .. } = self;
        kind.slots_changed(behavior, guard, player);
    }

    /// Runs the kind's `on_open` hook, after contents are built but before they
    /// are sent to the client.
    pub fn on_open(&mut self, player: &Player) {
        let mut guard = self.behavior().lock_all_containers();
        let Self { behavior, kind, .. } = self;
        kind.on_open(behavior, &mut guard, player);
    }

    /// Runs the kind's `on_tick` hook. Called once per server tick while open.
    pub fn on_tick(&mut self, player: &Player) {
        let mut guard = self.behavior().lock_all_containers();
        let Self { behavior, kind, .. } = self;
        kind.on_tick(behavior, &mut guard, player);
    }

    /// Shift-click (quick move) for a slot: the kind's override if any, else the
    /// declarative route table. Returns the item originally in the slot, or
    /// empty if nothing moved.
    fn quick_move_stack(
        &mut self,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> ItemStack {
        let Self {
            behavior,
            layout,
            kind,
            ..
        } = self;
        if let Some(result) = kind.quick_move(behavior, guard, slot_index, player) {
            result
        } else {
            layout.quick_move(behavior, guard, slot_index, player)
        }
    }

    /// Handles a click action in this menu. Packet clicks are validated via
    /// [`Click::parse`]; invalid programmatically constructed clicks are ignored.
    ///
    /// TODO: Add `tryItemClickBehaviorOverride` for bundle item support.
    pub fn clicked(&mut self, click: Click, player: &Player) {
        if !click.is_valid_for(self.behavior().slot_count()) {
            log::debug!(
                "Ignoring programmatic container click that violates parsed-click invariants: \
                 {click:?}"
            );
            return;
        }

        let has_infinite_materials = player.game_mode() == GameType::Creative;
        if let Click::QuickCraft(action) = click {
            let outcome = {
                let mut guard = self.behavior().lock_all_containers();
                let Self { behavior, kind, .. } = self;
                kind.on_drag(behavior, &mut guard, action, player)
            };
            if outcome == ClickOutcome::Consume {
                self.behavior_mut().reset_quick_craft();
            } else {
                let Self { behavior, kind, .. } = self;
                behavior.do_quick_craft(action, has_infinite_materials, player, &|slot| {
                    kind.can_drag_to(slot)
                });
            }
        } else {
            // Any non-quickcraft click resets an in-progress quickcraft.
            if self.behavior().quickcraft().is_some() {
                self.behavior_mut().reset_quick_craft();
            }

            // Menu-defined click hook. A consumed click skips default handling.
            // The guard is dropped before the default arms re-lock the same containers.
            let outcome = {
                let mut guard = self.behavior().lock_all_containers();
                let Self { behavior, kind, .. } = self;
                kind.on_slot_clicked(behavior, &mut guard, click, player)
            };

            if outcome == ClickOutcome::Fallthrough {
                match click {
                    Click::Pickup { slot, button } => {
                        self.behavior_mut().do_pickup(slot, button, player);
                    }
                    Click::DropCarried { button } => {
                        self.behavior_mut().drop_carried(button, player);
                    }
                    Click::QuickMove { slot } => {
                        self.do_quick_move(slot, player);
                    }
                    Click::Swap { slot, with } => {
                        self.do_swap(slot, with, player);
                    }
                    Click::Clone { slot } => {
                        self.behavior_mut().do_clone(slot, has_infinite_materials);
                    }
                    Click::Throw { slot, whole_stack } => {
                        self.behavior_mut().do_throw(slot, whole_stack, player);
                    }
                    Click::PickupAll { slot, direction } => {
                        self.do_pickup_all(slot, direction, player);
                    }
                    Click::QuickCraft(_) => unreachable!(),
                }
            }
        }
        // Recompute recipe-driven slots after the click. A QuickCraft drag has
        // no slot on its end phase, so recompute on any non-empty menu.
        let should_recompute = match click {
            Click::DropCarried { .. } => false,
            Click::QuickCraft(_) => !self.behavior().slots().is_empty(),
            _ => true,
        };
        if should_recompute {
            let mut guard = self.behavior().lock_all_containers();
            self.slots_changed(&mut guard, player);
        }
    }

    /// Handles quick move (shift-click).
    fn do_quick_move(&mut self, slot_index: usize, player: &Player) {
        let mut guard = self.behavior().lock_all_containers();

        if !self.behavior().slots()[slot_index].may_pickup(&guard, player) {
            return;
        }

        let initial_item = self.behavior().slots()[slot_index].get_item(&guard).clone();
        if initial_item.is_empty() {
            return;
        }

        // Loop while the slot still holds the same item type.
        let mut result = self.quick_move_stack(&mut guard, slot_index, player);

        while !result.is_empty() {
            let current_item = self.behavior().slots()[slot_index].get_item(&guard).clone();
            if !ItemStack::is_same_item(&current_item, &result) {
                break;
            }
            result = self.quick_move_stack(&mut guard, slot_index, player);
        }
    }

    /// Handles swap (number keys for a hotbar slot, or swap-hands for the
    /// offhand).
    fn do_swap(&mut self, slot_index: usize, with: SwapTarget, player: &Player) {
        let player_inventory = ContainerRef::from(player.inventory.clone());
        let player_inv_id = ContainerId::from_arc(&player.inventory);
        let mut guard = self.behavior().lock_all_containers_with(player_inventory);

        let behavior = self.behavior();
        let target_slot = &behavior.slots()[slot_index];
        let inventory_slot = with.inventory_slot();

        let mut target_item = target_slot.get_item(&guard).clone();
        let Some(inventory) = guard.get(player_inv_id) else {
            unreachable!("the explicitly locked player inventory must be present");
        };
        let source_item = inventory.get_item(inventory_slot).clone();

        if source_item.is_empty() && target_item.is_empty() {
            return;
        }

        if source_item.is_empty() {
            // Move target -> inventory.
            if target_slot.may_pickup(&guard, player) {
                target_slot.set_by_player(&mut guard, ItemStack::empty(), &target_item);
                if let Some(remainder) = target_slot.on_take(&mut guard, &mut target_item, player) {
                    player.add_item_or_drop_with_guard(&mut guard, remainder);
                }
                let Some(inventory) = guard.get_mut(player_inv_id) else {
                    unreachable!("the explicitly locked player inventory must be present");
                };
                inventory.set_item(inventory_slot, target_item);
            }
        } else if target_item.is_empty() {
            // Move inventory -> target.
            if target_slot.may_place(&source_item) {
                let max_size = target_slot.get_max_stack_size_for_item(&guard, &source_item);
                if source_item.count > max_size {
                    let Some(inv) = guard.get_mut(player_inv_id) else {
                        unreachable!("the explicitly locked player inventory must be present");
                    };
                    let to_place = inv.get_item_mut(inventory_slot).split(max_size);
                    target_slot.set_by_player(&mut guard, to_place, &ItemStack::empty());
                } else {
                    let Some(inventory) = guard.get_mut(player_inv_id) else {
                        unreachable!("the explicitly locked player inventory must be present");
                    };
                    inventory.set_item(inventory_slot, ItemStack::empty());
                    target_slot.set_by_player(&mut guard, source_item, &ItemStack::empty());
                }
            }
        } else {
            // Swap target <-> inventory.
            if target_slot.may_pickup(&guard, player) && target_slot.may_place(&source_item) {
                let max_size = target_slot.get_max_stack_size_for_item(&guard, &source_item);
                if source_item.count > max_size {
                    // Source too big: place a partial stack, return target to inventory.
                    let Some(inv) = guard.get_mut(player_inv_id) else {
                        unreachable!("the explicitly locked player inventory must be present");
                    };
                    let to_place = inv.get_item_mut(inventory_slot).split(max_size);
                    target_slot.set_by_player(&mut guard, to_place, &target_item);
                    if let Some(remainder) =
                        target_slot.on_take(&mut guard, &mut target_item, player)
                    {
                        player.add_item_or_drop_with_guard(&mut guard, remainder);
                    }
                    let mut displaced = target_item;
                    let Some(inventory) = guard.get_mut(player_inv_id) else {
                        unreachable!("the explicitly locked player inventory must be present");
                    };
                    let added = inventory.add(&mut displaced);
                    // Vanilla's Inventory::add consumes uninserted stacks in creative mode.
                    if !added && !player.has_infinite_materials() {
                        let _ = guard.run_unlocked(|| player.drop_item(displaced, false, true));
                    }
                } else {
                    target_slot.set_by_player(&mut guard, source_item, &target_item);
                    if let Some(remainder) =
                        target_slot.on_take(&mut guard, &mut target_item, player)
                    {
                        player.add_item_or_drop_with_guard(&mut guard, remainder);
                    }
                    let Some(inventory) = guard.get_mut(player_inv_id) else {
                        unreachable!("the explicitly locked player inventory must be present");
                    };
                    inventory.set_item(inventory_slot, target_item);
                }
            }
        }
    }

    /// Handles pickup all (double-click): collects matching items from all slots
    /// into the carried stack.
    fn do_pickup_all(&mut self, slot_index: usize, direction: FillDirection, player: &Player) {
        let mut guard = self.behavior().lock_all_containers();

        let behavior = self.behavior();
        let slot = &behavior.slots()[slot_index];
        let slot_has_item = !slot.get_item(&guard).is_empty();
        let slot_may_pickup = slot.may_pickup(&guard, player);

        if behavior.carried().is_empty() || (slot_has_item && slot_may_pickup) {
            return;
        }

        let max_stack = behavior.carried().max_stack_size();
        let carried_item = behavior.carried().clone();
        let slot_count = behavior.slots().len();

        let (start, step): (i32, i32) = match direction {
            FillDirection::Forward => (0, 1),
            FillDirection::Backward => (slot_count as i32 - 1, -1),
        };

        // First pass collects non-full stacks, second pass the full ones.
        for pass in 0..2 {
            let mut i = start;
            while i >= 0 && i < slot_count as i32 && self.behavior().carried().count < max_stack {
                let target_slot = &self.behavior().slots()[i as usize];
                let target_item = target_slot.get_item(&guard).clone();

                if !target_item.is_empty()
                    && can_item_quick_replace(&target_item, &carried_item, true)
                    && target_slot.may_pickup(&guard, player)
                    && self.can_take_item_for_pick_all(&carried_item, i as usize)
                {
                    // First pass skips full stacks, second pass includes them.
                    if pass != 0 || target_item.count != target_item.max_stack_size() {
                        let can_take = max_stack - self.behavior().carried().count;
                        let to_take = target_item.count.min(can_take);
                        let removed = target_slot.safe_take(&mut guard, to_take, can_take, player);
                        self.behavior_mut()
                            .carried_mut()
                            .grow(removed.count.min(can_take));
                    }
                }

                i += step;
            }
        }
    }
}

#[cfg(test)]
mod tests;
