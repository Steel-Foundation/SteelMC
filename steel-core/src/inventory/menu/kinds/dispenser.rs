//! Dispenser and Dropper (Generic 3x3) menu behavior.
//!
//! Dispensers and droppers share the same 3x3 grid menu layout.

use steel_registry::vanilla_menu_types;

use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Builds a generic 3x3 menu with 9 slots plus the player inventory.
#[must_use]
pub fn generic_3x3_menu(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
) -> Menu {
    let container = container.into();

    let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_3X3, container_id);
    let dispenser = builder.section(&container, 9);
    let player = builder.player_inventory(&inventory);

    builder.route(dispenser, player.all(), FillDirection::Backward);
    builder.route(player.all(), dispenser, FillDirection::Forward);

    builder.build(Generic3x3Kind { container })
}

/// Per-menu dispenser/dropper state: just the backing container for the validity check.
pub struct Generic3x3Kind {
    /// The backing container.
    container: ContainerRef,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for Generic3x3Kind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/generic_3x3");
}

impl MenuKind for Generic3x3Kind {
    /// Returns true if the backing container is still valid for the player.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.still_valid(player)
    }
}
