//! Chest menu for chest-like containers (chests, barrels, ender chests, shulker boxes).
//!
//! 1-6 rows of 9 slots. Layout:
//! - Slots 0 to `rows * 9 - 1`: Container
//! - Slots `rows * 9` to `rows * 9 + 26`: Main inventory (27)
//! - Slots `rows * 9 + 27` to `rows * 9 + 35`: Hotbar (9)

use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;

use crate::block_entity::{ContainerOpeners, SharedBlockEntity};
use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;

/// Slots per row in a chest menu.
pub const SLOTS_PER_ROW: usize = 9;

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
    chest_with_openers(
        inventory,
        container_id,
        vec![(container, rows * SLOTS_PER_ROW)],
        rows,
        Vec::new(),
    )
}

/// Builds a chest-like menu backed by one or more independently locked sections.
///
/// `openers` contains the block entities whose viewer counters this menu owns.
///
/// # Panics
///
/// Panics when `rows` is outside 1 through 6 or the sections do not contain
/// exactly `rows * 9` slots.
#[must_use]
pub fn chest_with_openers(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    containers: Vec<(ContainerRef, usize)>,
    rows: usize,
    openers: Vec<SharedBlockEntity>,
) -> Menu {
    assert!(
        (1..=6).contains(&rows),
        "Chest rows must be between 1 and 6"
    );
    assert_eq!(
        containers.iter().map(|(_, size)| size).sum::<usize>(),
        rows * SLOTS_PER_ROW,
        "Chest sections must cover every container slot"
    );

    let mut builder = MenuBuilder::new(menu_type_for_rows(rows), container_id);
    let chest = containers
        .iter()
        .map(|(container, size)| builder.section(container, *size))
        .collect::<Vec<_>>();
    let player = builder.player_inventory(&inventory);

    let opener_container_ids = openers
        .iter()
        .filter_map(|block_entity| block_entity.container_openers())
        .map(ContainerOpeners::opener_container_id)
        .collect();

    for section in &chest {
        builder.route(*section, [player.all()], FillDirection::Backward);
    }
    builder.route(player.all(), chest.as_slice(), FillDirection::Forward);

    builder.build(ChestKind {
        containers: containers
            .into_iter()
            .map(|(container, _)| container)
            .collect(),
        openers,
        opener_container_ids,
        opened: false,
    })
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

/// Per-menu chest state and viewer-count ownership.
pub struct ChestKind {
    containers: Vec<ContainerRef>,
    openers: Vec<SharedBlockEntity>,
    opener_container_ids: Vec<ContainerId>,
    opened: bool,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for ChestKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/chest");
}

impl MenuKind for ChestKind {
    fn opener_container_ids(&self) -> &[ContainerId] {
        &self.opener_container_ids
    }

    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        self.opened = true;
        guard.run_unlocked(|| {
            for block_entity in &self.openers {
                if let Some(openers) = block_entity.container_openers() {
                    openers.start_open(player);
                }
            }
        });
    }

    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        if !self.opened {
            return;
        }
        self.opened = false;
        for block_entity in &self.openers {
            if let Some(openers) = block_entity.container_openers() {
                openers.stop_open(player);
            }
        }
    }

    /// Returns true if every backing container is still valid for the player.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.containers
            .iter()
            .all(|container| container.still_valid(player))
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
