//! Blast furnace menu.
//!
//! 3 slots:
//! - 0: Input (ingredient)
//! - 1: Fuel
//! - 2: Output (result)
//!
//! 4 Data slots:
//! - 0: Lit time (fuel remaining)
//! - 1: Lit duration (max fuel)
//! - 2: Cooking progress
//! - 3: Cooking total time

use steel_registry::vanilla_menu_types;
use steel_utils::Downcast as _;

use crate::block_entity::entities::BlastFurnaceContainer;
use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Builds a blast furnace menu.
#[must_use]
pub fn blast_furnace(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
) -> Menu {
    let container = container.into();

    let mut builder = MenuBuilder::new(&vanilla_menu_types::BLAST_FURNACE, container_id);
    
    // Furnace slots
    let furnace = builder.section(&container, 3);
    
    // Player slots
    let player = builder.player_inventory(&inventory);

    builder.route(furnace, player.all(), FillDirection::Backward);
    builder.route(player.all(), furnace, FillDirection::Forward);

    let lit_time = builder.data_slot(0);
    let lit_duration = builder.data_slot(0);
    let cooking_progress = builder.data_slot(0);
    let cooking_total_time = builder.data_slot(0);

    builder.build(BlastFurnaceKind { 
        container,
        lit_time,
        lit_duration,
        cooking_progress,
        cooking_total_time,
    })
}

/// Per-menu blast furnace state managing container data slot synchronization.
pub struct BlastFurnaceKind {
    container: ContainerRef,
    lit_time: DataSlot,
    lit_duration: DataSlot,
    cooking_progress: DataSlot,
    cooking_total_time: DataSlot,
}

// SAFETY: Unique type key for blast furnace menu.
unsafe impl steel_utils::DowncastType for BlastFurnaceKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/blast_furnace");
}

impl MenuKind for BlastFurnaceKind {
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.still_valid(player)
    }

    fn on_open(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        self.on_tick(behavior, guard, player);
    }

    fn on_tick(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        if let Some(container) = guard
            .get(self.container.container_id())
            .and_then(|c| c.downcast_ref::<BlastFurnaceContainer>())
        {
            self.lit_time.set(behavior, container.lit_time as i16);
            self.lit_duration.set(behavior, container.lit_duration as i16);
            self.cooking_progress.set(behavior, container.cooking_progress as i16);
            self.cooking_total_time.set(behavior, container.cooking_total_time as i16);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::BlockPos;
    use steel_utils::locks::IntoShared as _;

    use super::*;
    use crate::block_entity::BlockEntity as _;
    use crate::block_entity::entities::BlastFurnaceBlockEntity;
    use crate::inventory::container::SimpleContainer;
    use crate::test_support::fresh_test_world;

    #[test]
    fn blast_furnace_menu_slot_count() {
        let inventory = PlayerInventory::new().into_shared();
        let container = SimpleContainer::new(3).into_shared();

        let menu = blast_furnace(inventory, 1, container);

        assert_eq!(menu.behavior().slot_count(), 3 + 36);
    }

    #[test]
    fn blast_furnace_menu_data_sync() {
        init_vanilla_registry();
        let world = fresh_test_world("menu_sync_test");
        let container = SimpleContainer::new(3).into_shared();
        let inventory = PlayerInventory::new().into_shared();
        let mut menu = blast_furnace(inventory, 1, container);

        let player = crate::test_support::TestPlayerBuilder::new(world, "MenuTestPlayer", 1).build();
        menu.on_open(&player);
        menu.on_tick(&player);
        assert!(menu.kind().still_valid(menu.behavior(), &player));
    }
}
