//! Shared nine-column container menus.
//!
//! Named for vanilla's `GENERIC_9Xn` menu types; vanilla calls this shared layout `ChestMenu`.
//!
//! 1-6 rows of 9 slots. Layout:
//! - Slots 0 to `rows * 9 - 1`: Container
//! - Slots `rows * 9` to `rows * 9 + 26`: Main inventory (27)
//! - Slots `rows * 9 + 27` to `rows * 9 + 35`: Hotbar (9)

use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;

use crate::inventory::menu::GRID_WIDTH;
use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Builds a container menu with `rows` rows of 9 slots plus the player inventory.
///
/// # Panics
/// Panics if `rows` is 0 or greater than 6.
#[must_use]
pub fn generic_9x_menu(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
    rows: usize,
) -> Menu {
    let container = container.into();
    generic_9x_menu_with_kind(
        inventory,
        container_id,
        container.clone(),
        rows,
        Generic9xKind { container },
    )
}

/// Builds the same layout as [`generic_9x_menu`] under a caller-supplied menu kind.
///
/// Menus whose validity or close cleanup is not the backing
/// container's own (the ender chest, whose container belongs to the player
/// rather than the block) share the layout through this.
///
/// # Panics
/// Panics if `rows` is 0 or greater than 6.
#[must_use]
pub(crate) fn generic_9x_menu_with_kind(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
    rows: usize,
    kind: impl MenuKind + 'static,
) -> Menu {
    let container = container.into();
    assert!(
        (1..=6).contains(&rows),
        "Container rows must be between 1 and 6"
    );

    let mut builder = MenuBuilder::new(menu_type_for_rows(rows), container_id);
    let container_slots = builder.section(&container, rows * GRID_WIDTH);
    let player = builder.player_inventory(&inventory);

    builder.route(container_slots, player.all(), FillDirection::Backward);
    builder.route(player.all(), container_slots, FillDirection::Forward);

    builder.build(kind)
}

/// Menu type for a container with `rows` rows.
///
/// # Panics
/// Panics if `rows` is 0 or greater than 6.
#[must_use]
pub fn menu_type_for_rows(rows: usize) -> MenuTypeRef {
    match rows {
        1 => &vanilla_menu_types::GENERIC_9X1,
        2 => &vanilla_menu_types::GENERIC_9X2,
        3 => &vanilla_menu_types::GENERIC_9X3,
        4 => &vanilla_menu_types::GENERIC_9X4,
        5 => &vanilla_menu_types::GENERIC_9X5,
        6 => &vanilla_menu_types::GENERIC_9X6,
        _ => panic!("Invalid row count: {rows}"),
    }
}

/// Per-menu container state: just the backing container for the validity check.
pub struct Generic9xKind {
    /// The backing container.
    container: ContainerRef,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for Generic9xKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/generic_9x");
}

impl MenuKind for Generic9xKind {
    fn opened_containers(&self) -> smallvec::SmallVec<[ContainerId; 2]> {
        smallvec::smallvec![self.container.container_id()]
    }

    /// Returns true if the backing container is still valid for the player.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.still_valid(player)
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::locks::IntoShared as _;

    use super::*;
    use crate::inventory::container::SimpleContainer;

    #[test]
    fn generic_9x_menu_uses_exactly_the_rows_requested_from_oversized_container() {
        let inventory = PlayerInventory::new().into_shared();
        let container = SimpleContainer::new(18).into_shared();

        let menu = generic_9x_menu(inventory, 1, container, 1);

        assert_eq!(menu.behavior().slot_count(), 9 + 36);
    }
}
