//! Read-only chest menu for server GUIs.
//!
//! Same layout as [`chest`](super::chest), but every click and drag is consumed so no item
//! moves in or out; the client is resynchronized by the normal click path. Interaction is
//! observed through the inventory click event, which fires before any container is locked,
//! so handlers may freely change inventories, open other menus or close this one.

use steel_registry::vanilla_menu_types;

use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Menu kind that treats every slot as a button.
pub struct DisplayKind;

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind within the process.
unsafe impl steel_utils::DowncastType for DisplayKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/display");
}

impl MenuKind for DisplayKind {
    fn on_slot_clicked(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _click: Click,
        _player: &Player,
    ) -> ClickOutcome {
        ClickOutcome::Consume
    }

    fn on_drag(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _action: QuickCraft,
        _player: &Player,
    ) -> ClickOutcome {
        ClickOutcome::Consume
    }
}

/// Builds a read-only chest GUI of `rows` rows (clamped to 1..=6) over `container`.
#[must_use]
pub fn display_chest(
    inventory: &Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
    rows: usize,
) -> Menu {
    let rows = rows.clamp(1, 6);
    let menu_type = match rows {
        1 => &vanilla_menu_types::GENERIC_9X1,
        2 => &vanilla_menu_types::GENERIC_9X2,
        3 => &vanilla_menu_types::GENERIC_9X3,
        4 => &vanilla_menu_types::GENERIC_9X4,
        5 => &vanilla_menu_types::GENERIC_9X5,
        _ => &vanilla_menu_types::GENERIC_9X6,
    };
    let mut builder = MenuBuilder::new(menu_type, container_id);
    builder.section(container.into(), rows * 9);
    builder.player_inventory(inventory);
    builder.build(DisplayKind)
}
