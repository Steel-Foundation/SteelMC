use core::sync::atomic::Ordering;
use std::sync::{Arc, atomic::AtomicI32};

use crate::inventory::prelude::*;
use crate::inventory::slots::LoomHandler;
use crate::{
    inventory::container::{ResultContainer, SimpleContainer},
    player::player_inventory::PlayerInventory,
};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::{
    data_components::vanilla_components::DYE,
    data_components::vanilla_components::PROVIDES_BANNER_PATTERNS, vanilla_blocks,
    vanilla_item_tags::ItemTag, vanilla_menu_types,
};
use steel_utils::BlockPos;
use steel_utils::locks::{IntoShared, Shared};

/// Builds a loom menu
#[must_use]
pub fn loom(inventory: Shared<PlayerInventory>, container_id: u8, block_pos: BlockPos) -> Menu {
    let input_container = SimpleContainer::new(3).into_shared();
    let button_id = Arc::new(AtomicI32::new(-1));
    let buttons_len = Arc::new(AtomicI32::new(0));

    let result_container = ResultContainer::new().into_shared();

    let handler = LoomHandler::new(
        input_container.clone(),
        result_container.clone(),
        button_id.clone(),
        buttons_len.clone(),
    );

    let mut builder = MenuBuilder::new(&vanilla_menu_types::LOOM, container_id);

    let input = builder.section_at(
        &input_container,
        [0, 1, 2],
        SectionKind::restricted(|i: usize, stack: &ItemStack| match i {
            0 => stack.item.has_tag(&ItemTag::BANNERS), // TODO: Uses item tag in place of instanceof
            1 => stack.item.has_tag(&ItemTag::LOOM_DYES) && stack.has(DYE),
            2 => stack.item.has_tag(&ItemTag::LOOM_PATTERNS) && stack.has(PROVIDES_BANNER_PATTERNS),
            _ => false,
        }),
    );

    let result = builder.result_slot(handler.clone());
    let player = builder.player_inventory(&inventory);

    builder.route(result, player.all(), FillDirection::Backward);
    builder.route(input, player.all(), FillDirection::Forward);
    builder.route(
        player.main(),
        [input, player.hotbar()],
        FillDirection::Forward,
    );
    builder.route(
        player.hotbar(),
        [input, player.main()],
        FillDirection::Forward,
    );
    builder.drain(input);
    builder.build(LoomKind {
        result_container,
        button_id,
        buttons_len,
        block_pos,
        handler,
    })
}

/// Per-menu loom state
pub struct LoomKind {
    result_container: Shared<ResultContainer>,
    button_id: Arc<AtomicI32>,
    buttons_len: Arc<AtomicI32>,
    block_pos: BlockPos,
    handler: LoomHandler,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for LoomKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/loom");
}

impl MenuKind for LoomKind {
    fn on_button_clicked(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        button_id: i32,
        _player: &Player,
    ) -> ClickOutcome {
        // TODO: ClickOutcome is the wrong return type
        if self.buttons_len.load(Ordering::Relaxed) <= button_id {
            return ClickOutcome::Fallthrough;
        }
        self.button_id.store(button_id, Ordering::Relaxed);
        self.handler.update_result(guard);
        ClickOutcome::Fallthrough
    }

    /// Returns true if the block is still a loom and the player is in
    /// range (plus a 4.0 buffer).
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        let world = player.get_world();
        world.get_block_state(self.block_pos).get_block() == &vanilla_blocks::LOOM
            && player.is_within_block_interaction_range_with_buffer(self.block_pos, 4.0)
    }

    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.result_container.lock().set_item(0, ItemStack::empty());
    }

    fn slots_changed(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.handler.update_result(guard)
    }
}
