//! Chest menu for chest-like containers (chests, barrels, ender chests, shulker boxes).
//!
//! 1-6 rows of 9 slots. Layout:
//! - Slots 0 to `rows * 9 - 1`: Container
//! - Slots `rows * 9` to `rows * 9 + 26`: Main inventory (27)
//! - Slots `rows * 9 + 27` to `rows * 9 + 35`: Hotbar (9)

use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;

use std::sync::Arc;

use crate::block_entity::SharedBlockEntity;
use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Builds a chest-like menu with `rows` rows of 9 slots plus the player inventory.
///
/// # Panics
/// Panics if `rows` is 0 or greater than 6.
#[must_use]
pub fn chest(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
    rows: usize,
) -> Menu {
    let container = container.into();
    assert!(
        (1..=6).contains(&rows),
        "Chest rows must be between 1 and 6"
    );

    let mut builder = MenuBuilder::new(menu_type_for_rows(rows), container_id);
    let chest = builder.section(&container, rows * 9);
    let player = builder.player_inventory(&inventory);

    builder.route(chest, player.all(), FillDirection::Backward);
    builder.route(player.all(), chest, FillDirection::Forward);

    builder.build(ChestKind {
        containers: vec![container],
        viewed_block_entities: Vec::new(),
    })
}

/// Builds a chest-like menu over one or two container block entities.
///
/// Vanilla wraps a double chest in a `CompoundContainer` and hands it to
/// `ChestMenu.sixRows`. Steel keeps each half independently lockable and maps
/// them to consecutive sections instead, which produces the same slot order.
/// The block entities are notified through Vanilla's `Container.startOpen` and
/// `stopOpen` so their viewer counters stay accurate.
///
/// # Panics
/// Panics if the block entities provide no container, or together cover a slot
/// count that is not 1 to 6 full rows.
#[must_use]
pub fn chest_for_block_entities(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    block_entities: Vec<SharedBlockEntity>,
) -> Menu {
    let containers: Vec<ContainerRef> = block_entities
        .iter()
        .map(|block_entity| {
            ContainerRef::from_block_entity(Arc::clone(block_entity))
                .expect("chest block entity must expose a container")
        })
        .collect();

    let slot_count: usize = containers.iter().map(container_size).sum();
    let rows = slot_count / 9;
    assert!(
        (1..=6).contains(&rows) && slot_count.is_multiple_of(9),
        "Chest sections must cover 1 to 6 full rows, got {slot_count} slots"
    );

    let mut builder = MenuBuilder::new(menu_type_for_rows(rows), container_id);
    let sections: Vec<Section> = containers
        .iter()
        .map(|container| builder.section_all(container))
        .collect();
    let player = builder.player_inventory(&inventory);

    builder.route(sections.clone(), player.all(), FillDirection::Backward);
    builder.route(player.all(), sections, FillDirection::Forward);

    builder.build(ChestKind {
        containers,
        viewed_block_entities: block_entities,
    })
}

fn container_size(container: &ContainerRef) -> usize {
    ContainerLockGuard::lock_all(&[container])
        .get(container.container_id())
        .map_or(0, Container::get_container_size)
}

/// Menu type for a chest of `rows` rows.
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

/// Per-menu chest state: the backing containers plus the block entities whose
/// viewer counters this menu drives.
pub struct ChestKind {
    /// The backing containers, in section order.
    containers: Vec<ContainerRef>,
    /// Block entities notified on open and close, empty for plain containers.
    viewed_block_entities: Vec<SharedBlockEntity>,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for ChestKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/chest");
}

impl MenuKind for ChestKind {
    /// Returns true if every backing container is still valid for the player.
    ///
    /// Vanilla's `CompoundContainer.stillValid` requires both halves.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.containers
            .iter()
            .all(|container| container.still_valid(player))
    }

    /// Vanilla `ChestMenu`'s constructor call to `Container.startOpen`.
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        if self.viewed_block_entities.is_empty() {
            return;
        }
        // Viewer callbacks reach into the world, so they must not run while this
        // menu holds its container locks.
        guard.run_unlocked(|| {
            for block_entity in &self.viewed_block_entities {
                block_entity.start_open(player);
            }
        });
    }

    /// Vanilla `ChestMenu.removed`'s call to `Container.stopOpen`.
    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        for block_entity in &self.viewed_block_entities {
            block_entity.stop_open(player);
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::locks::IntoShared as _;

    use super::*;
    use crate::inventory::container::SimpleContainer;

    #[test]
    fn chest_uses_exactly_the_rows_requested_from_oversized_container() {
        let inventory = PlayerInventory::new().into_shared();
        let container = SimpleContainer::new(18).into_shared();

        let menu = chest(inventory, 1, container, 1);

        assert_eq!(menu.behavior().slot_count(), 9 + 36);
    }
}
