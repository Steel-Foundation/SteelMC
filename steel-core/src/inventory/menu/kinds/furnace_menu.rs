//! Furnace menu for furnace-like containers (furnace, blast furnace, smoker).
//!
//! 3 machine slots + player inventory:
//! - Slot 0: Input (item to smelt)
//! - Slot 1: Fuel (item to burn)
//! - Slot 2: Output (smelted result)
//! - Slots 3-38: Player inventory (27 main + 9 hotbar)

use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;

use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Builds a furnace menu with 3 machine slots (input, fuel, output) plus the player inventory.
///
/// The furnace menu uses the `FURNACE` menu type which has 39 total slots.
#[must_use]
pub fn furnace(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
) -> Menu {
    let container = container.into();
    let mut builder = MenuBuilder::new(&vanilla_menu_types::FURNACE, container_id);

    // Machine slots: input (0), fuel (1), output (2)
    let machine = builder.section(&container, 3);
    let player = builder.player_inventory(&inventory);

    // Quick-move routing:
    // - From machine slots -> player inventory (backward fill)
    builder.route(machine, player.all(), FillDirection::Backward);
    // - From player inventory -> machine slots (forward fill)
    builder.route(player.all(), machine, FillDirection::Forward);

    builder.build(FurnaceKind { container })
}

/// Per-menu furnace state: just the backing container for the validity check.
pub struct FurnaceKind {
    /// The backing container.
    container: ContainerRef,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for FurnaceKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/furnace");
}

impl MenuKind for FurnaceKind {
    /// Returns true if the backing container is still valid for the player.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.still_valid(player)
    }
}
