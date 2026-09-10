//! Grindstone menu: two input slots and a computed result slot.
//!
//! Slot layout (39 total), matching vanilla `GrindstoneMenu`:
//! - Slot 0: first input
//! - Slot 1: second input
//! - Slot 2: result
//! - Slots 3-29: main inventory (27)
//! - Slots 30-38: hotbar (9)

use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{vanilla_blocks, vanilla_menu_types};
use steel_utils::BlockPos;
use steel_utils::locks::{IntoShared as _, Shared};

use crate::inventory::{
    container::{ResultContainer, SimpleContainer},
    prelude::*,
    slots::{GrindstoneResultHandler, grindstone_input_allows},
};
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;

/// Builds the grindstone menu.
#[must_use]
pub fn grindstone(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    pos: BlockPos,
    world: &Arc<World>,
) -> Menu {
    let input_container = SimpleContainer::new(2).into_shared();
    let result_container = ResultContainer::new().into_shared();

    let handler = GrindstoneResultHandler::new(
        input_container.clone(),
        result_container.clone(),
        pos,
        world.clone(),
    );

    let mut builder = MenuBuilder::new(&vanilla_menu_types::GRINDSTONE, container_id);

    let inputs = builder.section_all_with(
        input_container.clone(),
        SectionKind::restricted(|_index, stack| grindstone_input_allows(stack)),
    );
    let result = builder.result_slot(handler.clone());
    let player = builder.player_inventory(&inventory);

    builder.route_with_remainder_policy(
        result,
        player.all(),
        FillDirection::Backward,
        FakeResultRemainderPolicy::Discard,
    );
    builder.route(inputs, player.all(), FillDirection::Forward);
    builder.route(player.hotbar(), inputs, FillDirection::Forward);
    builder.route(player.main(), inputs, FillDirection::Forward);
    builder.drain(inputs);

    builder.build(GrindstoneKind {
        result_container,
        result,
        block_pos: pos,
        world: world.clone(),
        handler,
    })
}

/// Per-menu grindstone state: result container, block position, and the handler
/// that recomputes the result.
pub struct GrindstoneKind {
    result_container: Shared<ResultContainer>,
    result: Section,
    block_pos: BlockPos,
    world: Arc<World>,
    handler: GrindstoneResultHandler,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for GrindstoneKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/grindstone");
}

impl MenuKind for GrindstoneKind {
    /// Prevents taking the computed result during pickup-all.
    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, slot_index: usize) -> bool {
        !self.result.contains(slot_index)
    }

    /// Valid while the original grindstone stays in range.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.world.get_block_state(self.block_pos).get_block() == &vanilla_blocks::GRINDSTONE
            && player.is_within_block_interaction_range_with_buffer(self.block_pos, 4.0)
    }

    /// Clears the virtual result on close. Inputs are drained by [`Menu::removed`].
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
