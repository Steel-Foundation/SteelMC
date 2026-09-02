//! Per-menu behavior hooks.

use steel_registry::item_stack::ItemStack;
use steel_utils::ErasedType;

use crate::inventory::menu::behavior::MenuBehavior;
use crate::{
    inventory::lock::{ContainerId, ContainerLockGuard},
    player::Player,
};

use crate::inventory::click::{Click, ClickOutcome, QuickCraft};

/// Per-menu behavior that isn't shared: recompute-on-change, validity, close
/// cleanup, and the optional shift-click override.
///
/// Menu transitions requested while a hook owns mutable access to the current
/// menu are applied after that hook returns.
///
/// Concrete implementations must claim a unique
/// [`steel_utils::DowncastTypeKey`] through [`steel_utils::DowncastType`].
pub trait MenuKind: ErasedType + Send + Sync {
    /// Returns the block-container identities whose opener lifecycle this menu owns.
    ///
    /// This is deliberately narrower than every inventory referenced by the
    /// menu: ordinary chest-shaped menus may share storage without opening a
    /// block entity or driving its animation.
    fn opener_container_ids(&self) -> &[ContainerId] {
        &[]
    }

    /// Recompute recipe-driven slots after a click touched a real slot.
    fn slots_changed(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
    }

    /// Extra cleanup on close beyond [`Menu::removed`].
    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {}

    /// Applies a rename from the client (anvil-style text input) and recomputes
    /// any result. No-op for kinds without a rename input.
    fn on_rename(&mut self, _behavior: &mut MenuBehavior, _name: String, _player: &Player) {}

    /// Runs after initial contents are built but before they're sent, so
    /// anything populated here appears in the first render.
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
    }

    /// Runs once per tick per viewer while open, before changes are synced.
    fn on_tick(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
    }

    /// Runs for every non-drag click before default handling. Return
    /// [`ClickOutcome::Consume`] to treat the slot as a button, or
    /// [`ClickOutcome::Fallthrough`] for default handling.
    fn on_slot_clicked(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _click: Click,
        _player: &Player,
    ) -> ClickOutcome {
        ClickOutcome::Fallthrough
    }

    /// Runs for each drag phase before default handling. Return
    /// [`ClickOutcome::Consume`] to cancel the drag.
    fn on_drag(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _action: QuickCraft,
        _player: &Player,
    ) -> ClickOutcome {
        ClickOutcome::Fallthrough
    }

    /// Returns true if a drag may distribute items into `slot_index`.
    fn can_drag_to(&self, _slot_index: usize) -> bool {
        true
    }

    /// Returns true if this menu is still valid for the player.
    fn still_valid(&self, _behavior: &MenuBehavior, _player: &Player) -> bool {
        true
    }

    /// Returns true if an item may be taken from `slot_index` during pickup-all.
    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, _slot_index: usize) -> bool {
        true
    }

    /// Shift-click override. Return `Some` to fully handle the quick-move, or
    /// `None` to fall back to the route table.
    fn quick_move(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _slot_index: usize,
        _player: &Player,
    ) -> Option<ItemStack> {
        None
    }
}
