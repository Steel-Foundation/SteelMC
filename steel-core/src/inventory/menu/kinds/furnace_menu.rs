//! Furnace menu for furnace-like containers (furnace, blast furnace, smoker).
//!
//! 3 machine slots + player inventory:
//! - Slot 0: Input (item to smelt)
//! - Slot 1: Fuel (item to burn)
//! - Slot 2: Output (smelted result)
//! - Slots 3-38: Player inventory (27 main + 9 hotbar)

use crate::block_entity::{
    SharedBlockEntity,
    entities::{FurnaceBlockEntity, FurnaceContainer},
};
use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use steel_utils::{Downcast, locks::SyncMutex};

/// Builds a furnace menu with 3 machine slots (input, fuel, output) plus the player inventory.
///
/// The furnace menu uses the `FURNACE` menu type which has 39 total slots and 4 data slots:
/// - Data 0: lit_time_remaining (fire animation)
/// - Data 1: lit_total_time (fire animation scale)
/// - Data 2: cooking_progress (arrow animation)
/// - Data 3: cooking_total_time (arrow animation scale)
#[must_use]
pub fn furnace(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
    block_entity: SharedBlockEntity,
) -> Menu {
    let container = container.into();

    // Get the Arc<SyncMutex<FurnaceContainer>> for the data slot updates
    let furnace_container = block_entity
        .downcast_ref::<FurnaceBlockEntity>()
        .expect("furnace menu requires FurnaceBlockEntity")
        .container();

    let mut builder = MenuBuilder::new(&steel_registry::vanilla_menu_types::FURNACE, container_id);

    // Machine slots: input (0), fuel (1), output (2)
    let machine = builder.section(&container, 3);
    let player = builder.player_inventory(&inventory);

    // Data slots for furnace animation (fire + cooking progress)
    let lit_time_remaining = builder.data_slot(0);
    let lit_total_time = builder.data_slot(0);
    let cooking_progress = builder.data_slot(0);
    let cooking_total_time = builder.data_slot(0);

    // Quick-move routing:
    // - From machine slots -> player inventory (backward fill)
    builder.route(machine, player.all(), FillDirection::Backward);
    // - From player inventory -> machine slots (forward fill)
    builder.route(player.all(), machine, FillDirection::Forward);

    builder.build(FurnaceKind {
        container,
        furnace_container,
        lit_time_remaining,
        lit_total_time,
        cooking_progress,
        cooking_total_time,
    })
}

/// Per-menu furnace state: container + data slots for animations.
pub struct FurnaceKind {
    /// The backing container.
    container: ContainerRef,
    /// The furnace container (for reading atomic state)
    furnace_container: Arc<SyncMutex<FurnaceContainer>>,
    /// Data slot 0: fuel burn time remaining (for fire animation)
    pub lit_time_remaining: DataSlot,
    /// Data slot 1: fuel total burn time (for fire animation scale)
    pub lit_total_time: DataSlot,
    /// Data slot 2: cooking progress (for arrow animation)
    pub cooking_progress: DataSlot,
    /// Data slot 3: cooking total time (for arrow animation scale)
    pub cooking_total_time: DataSlot,
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

    /// Update data slots every tick to sync furnace state to client
    fn on_tick(
        &mut self,
        behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        // Use try_lock to avoid deadlock - if we can't get the lock, skip this update
        if let Some(container) = self.furnace_container.try_lock() {
            let lit_time = container.lit_time_remaining.load(Ordering::Relaxed) as i16;
            let lit_total = container.lit_total_time.load(Ordering::Relaxed) as i16;
            let cook_time = container.cooking_timer.load(Ordering::Relaxed) as i16;
            let cook_total = container.cooking_total_time.load(Ordering::Relaxed) as i16;
            drop(container); // Release the lock immediately

            self.lit_time_remaining.set(behavior, lit_time);
            self.lit_total_time.set(behavior, lit_total);
            self.cooking_progress.set(behavior, cook_time);
            self.cooking_total_time.set(behavior, cook_total);
        }
    }
}
