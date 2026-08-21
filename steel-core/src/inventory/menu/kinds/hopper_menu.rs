//! Hopper menu for hopper containers (hopper blocks, hopper minecarts).
//!
//! A single row of 5 slots. Layout:
//! - Slots 0 to 4: Container
//! - Slots 5 to 31: Main inventory (27)
//! - Slots 32 to 40: Hotbar (9)

use steel_registry::vanilla_menu_types;

use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Builds a hopper menu with 5 container slots plus the player inventory.
#[must_use]
pub fn hopper(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
) -> Menu {
    let container = container.into();

    let mut builder = MenuBuilder::new(&vanilla_menu_types::HOPPER, container_id);
    let hopper = builder.section(&container, 5);
    let player = builder.player_inventory(&inventory);

    builder.route(hopper, player.all(), FillDirection::Backward);
    builder.route(player.all(), hopper, FillDirection::Forward);

    builder.build(HopperKind { container })
}

/// Per-menu hopper state: just the backing container for the validity check.
pub struct HopperKind {
    /// The backing container.
    container: ContainerRef,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for HopperKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/hopper");
}

impl MenuKind for HopperKind {
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
    fn hopper_uses_exactly_five_slots_from_oversized_container() {
        let inventory = PlayerInventory::new().into_shared();
        let container = SimpleContainer::new(18).into_shared();

        let menu = hopper(inventory, 1, container);

        assert_eq!(menu.behavior().slot_count(), 5 + 36);
    }
}
