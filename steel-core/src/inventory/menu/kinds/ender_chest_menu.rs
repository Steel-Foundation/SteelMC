//! Ender chest menu.

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::ender_chest::{ENDER_CHEST_SLOTS, SyncPlayerEnderChest};
use crate::inventory::menu::kinds::chest_menu::chest_with_kind;
use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Builds the menu over a player's ender chest container.
#[must_use]
pub fn ender_chest(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: SyncPlayerEnderChest,
) -> Menu {
    chest_with_kind(
        inventory,
        container_id,
        container.clone(),
        ENDER_CHEST_SLOTS / 9,
        EnderChestKind { container },
    )
}

/// Per-menu ender chest state: the player's container, which knows which block
/// entity the menu was opened from.
pub struct EnderChestKind {
    container: SyncPlayerEnderChest,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl DowncastType for EnderChestKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/ender_chest");
}

impl MenuKind for EnderChestKind {
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.lock().still_valid(player)
    }

    /// `PlayerEnderChestContainer.stopOpen`
    /// drops the active chest so a later validity check can't consult a stale one.
    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.container.lock().clear_active_chest();
    }
}
