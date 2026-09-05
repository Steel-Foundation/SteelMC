use std::sync::LazyLock;

use steel_registry::{RegistryEntry, item_stack::ItemStack, vanilla_items, vanilla_menu_types};
use steel_utils::locks::Shared;

use crate::{
    inventory::{
        lock::ContainerRef,
        menu::{FillDirection, Menu, MenuBehavior, MenuBuilder, MenuKind, SectionKind},
    },
    player::{Player, player_inventory::PlayerInventory},
};

/// Builds a shulker box menu with 3 rows of 9 slots plus the player inventory.
#[must_use]
pub fn shulker_box(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: impl Into<ContainerRef>,
) -> Menu {
    let container = container.into();

    let mut builder = MenuBuilder::new(&vanilla_menu_types::SHULKER_BOX, container_id);
    let shulker_box = builder.section_with(
        &container,
        27,
        SectionKind::restricted(|_slot, stack| !is_shulker_box(stack)),
    );
    let player = builder.player_inventory(&inventory);

    builder.route(shulker_box, player.all(), FillDirection::Backward);
    builder.route(player.all(), shulker_box, FillDirection::Forward);

    builder.build(ShulkerBoxKind { container })
}

fn is_shulker_box(stack: &ItemStack) -> bool {
    static SHULKERS: LazyLock<[usize; 17]> = LazyLock::new(|| {
        [
            vanilla_items::SHULKER_BOX.id(),
            vanilla_items::RED_SHULKER_BOX.id(),
            vanilla_items::BLUE_SHULKER_BOX.id(),
            vanilla_items::CYAN_SHULKER_BOX.id(),
            vanilla_items::GRAY_SHULKER_BOX.id(),
            vanilla_items::LIME_SHULKER_BOX.id(),
            vanilla_items::PINK_SHULKER_BOX.id(),
            vanilla_items::BLACK_SHULKER_BOX.id(),
            vanilla_items::BROWN_SHULKER_BOX.id(),
            vanilla_items::GREEN_SHULKER_BOX.id(),
            vanilla_items::WHITE_SHULKER_BOX.id(),
            vanilla_items::ORANGE_SHULKER_BOX.id(),
            vanilla_items::PURPLE_SHULKER_BOX.id(),
            vanilla_items::YELLOW_SHULKER_BOX.id(),
            vanilla_items::MAGENTA_SHULKER_BOX.id(),
            vanilla_items::LIGHT_BLUE_SHULKER_BOX.id(),
            vanilla_items::LIGHT_GRAY_SHULKER_BOX.id(),
        ]
    });
    SHULKERS.contains(&stack.item().id())
}

/// Per-menu shulker box state: just the backing container for the validity check.
pub struct ShulkerBoxKind {
    /// The backing container.
    container: ContainerRef,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for ShulkerBoxKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/shulker_box");
}

impl MenuKind for ShulkerBoxKind {
    /// Returns true if the backing container is still valid for the player.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.still_valid(player)
    }
}
