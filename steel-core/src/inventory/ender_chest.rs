//! Ender chest container implementation.

use std::sync::Weak;

use std::sync::Arc;
use steel_registry::item_stack::ItemStack;

use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityLifecycleExt};
use crate::inventory::container::Container;
use crate::player::Player;

/// Number of slots in an ender chest (3 rows of 9).
pub const ENDER_CHEST_SLOTS: usize = 27;

/// Thread-safe reference to an ender chest block entity.
type WeakBlockEntity = Weak<dyn BlockEntity>;

/// Thread-safe reference to a player's ender chest container.
pub type SyncPlayerEnderChest = Arc<SyncMutex<PlayerEnderChestContainer>>;

/// The player's ender chest inventory.
pub struct PlayerEnderChestContainer {
    items: Vec<ItemStack>,
    active_chest: Option<WeakBlockEntity>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `PlayerEnderChestContainer`.
unsafe impl DowncastType for PlayerEnderChestContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:inventory/player_ender_chest");
}

impl Default for PlayerEnderChestContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerEnderChestContainer {
    /// Creates a new, empty ender chest inventory.
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: vec![ItemStack::empty(); ENDER_CHEST_SLOTS],
            active_chest: None,
        }
    }

    /// Sets the active block entity being interacted with.
    pub fn set_active_chest(&mut self, active_chest: WeakBlockEntity) {
        self.active_chest = Some(active_chest);
    }

    /// Clears the active block entity.
    pub fn clear_active_chest(&mut self) {
        self.active_chest = None;
    }

    /// Checks if the container is still valid for the given player.
    pub fn still_valid(&self, player: &Player) -> bool {
        // If an active chest is set, check if it's still valid (in range, not broken)
        if let Some(weak_chest) = &self.active_chest {
            if let Some(chest) = weak_chest.upgrade() {
                // Validate range with default container interaction range
                return !chest.is_removed()
                    && player
                        .is_within_block_interaction_range_with_buffer(chest.get_block_pos(), 4.0);
            }
            // Chest was destroyed while open
            return false;
        }

        true
    }
}

impl Container for PlayerEnderChestContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_changed(&mut self) {
        // Player data saving handles change tracking for this inventory.
    }
}
