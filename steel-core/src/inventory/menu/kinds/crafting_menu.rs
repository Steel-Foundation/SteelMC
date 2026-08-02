//! Crafting table menu (3x3 grid).
//!
//! Slot layout (46 total):
//! - Slot 0: Result
//! - Slots 1-9: 3x3 grid
//! - Slots 10-36: Main inventory (27)
//! - Slots 37-45: Hotbar (9)

use crate::inventory::container::CraftingContainer;
use crate::inventory::container::ResultContainer;
use crate::inventory::prelude::*;
use crate::inventory::slots::CraftingHandler;
use crate::player::player_inventory::PlayerInventory;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_menu_types;
use steel_utils::BlockPos;
use steel_utils::locks::IntoShared;
use steel_utils::locks::Shared;

/// Builds the crafting table menu with a 3x3 grid.
#[must_use]
pub fn crafting(inventory: Shared<PlayerInventory>, container_id: u8, block_pos: BlockPos) -> Menu {
    let crafting_container = CraftingContainer::new(3, 3).into_shared();
    let result_container = ResultContainer::new().into_shared();

    let handler = CraftingHandler::new(crafting_container.clone(), result_container.clone(), 3);

    let mut builder = MenuBuilder::new(&vanilla_menu_types::CRAFTING, container_id);
    let result = builder.result_slot(handler.clone());
    let grid = builder.section_all(crafting_container);
    let player = builder.player_inventory(&inventory);

    builder.route(result, player.all(), FillDirection::Backward);
    builder.route(grid, player.all(), FillDirection::Forward);
    builder.route(
        player.main(),
        [grid, player.hotbar()],
        FillDirection::Forward,
    );
    builder.route(
        player.hotbar(),
        [grid, player.main()],
        FillDirection::Forward,
    );
    builder.drain(grid);

    builder.build(CraftingKind {
        result_container,
        result,
        block_pos,
        handler,
    })
}

/// Per-menu crafting state: result container, table position, and recipe handler.
pub struct CraftingKind {
    /// The result container.
    result_container: Shared<ResultContainer>,
    /// The result (slot 0).
    result: Section,
    /// The crafting table block position.
    block_pos: BlockPos,
    handler: CraftingHandler,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for CraftingKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/crafting");
}

impl MenuKind for CraftingKind {
    /// Prevents taking from the result slot during pickup-all.
    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, slot_index: usize) -> bool {
        !self.result.contains(slot_index)
    }

    /// Returns true if the block is still a crafting table and the player is in
    /// range (plus a 4.0 buffer).
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        let world = player.get_world();
        world.get_block_state(self.block_pos).get_block() == &vanilla_blocks::CRAFTING_TABLE
            && player.is_within_block_interaction_range_with_buffer(self.block_pos, 4.0)
    }

    /// Clears the virtual result on close. The grid is drained by [`Menu::removed`].
    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.result_container.lock().set_item(0, ItemStack::empty());
    }

    fn slots_changed(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.handler.update_result(guard);
    }
}

#[cfg(test)]
mod tests;
