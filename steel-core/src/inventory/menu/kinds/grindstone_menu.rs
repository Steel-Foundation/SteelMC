//! Grindston menu.
use std::sync::Arc;

use steel_registry::vanilla_blocks;
use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt,
    blocks::block_state_ext::BlockStateExt,
    data_components::{
        components::ItemEnchantments,
        vanilla_components::{ENCHANTMENTS, REPAIR_COST},
    },
    item_stack::ItemStack,
    vanilla_enchantment_tags::EnchantmentTag,
    vanilla_items, vanilla_menu_types,
};
use steel_utils::{
    BlockPos,
    locks::{IntoShared, Shared},
};

use crate::{
    inventory::{
        container::{ResultContainer, SimpleContainer},
        menu::kinds::AnvilKind,
        prelude::*,
        slots::GrindstoneResultHandler,
    },
    player::player_inventory::PlayerInventory,
    world::World,
};

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

    let mut builder = MenuBuilder::new(&vanilla_menu_types::GRINDSTONE, container_id);

    let input = builder.section_all(&input_container);
    let result = builder.result_slot(GrindstoneResultHandler::new(
        input_container.clone(),
        result_container.clone(),
        pos,
        world.clone(),
    ));

    let player = builder.player_inventory(&inventory);

    builder.route_with_remainder_policy(
        result,
        player.all(),
        FillDirection::Backward,
        FakeResultRemainderPolicy::Discard,
    );
    builder.route(input, player.all(), FillDirection::Forward);
    builder.route(player.hotbar(), input, FillDirection::Forward);
    builder.route(player.main(), input, FillDirection::Forward);
    builder.drain(input);

    builder.build(GrindstoneKind {
        input_container,
        result_container,
        block_pos: pos,
        world: Arc::clone(world),
    })
}

/// Per-menu grindstone state: inputs, result, level cost, and rename text.
pub struct GrindstoneKind {
    /// Input container (two slots).
    input_container: Shared<SimpleContainer>,
    /// Result container (single virtual slot).
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for GrindstoneKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/grindstone");
}

impl GrindstoneKind {
    /// Builds the grindstone result from combining and renaming the two inputs.
    ///
    /// # Panics
    /// Panics if the input container is not exactly two slots.
    #[tracing::instrument(skip(self, _behavior, player, guard), level = "info", fields(player = %player.gameprofile.name))]
    pub(crate) fn create_result(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        let Some([input_container, result_container]) = guard.get_disjoint_mut([
            ContainerId::from_arc(&self.input_container),
            ContainerId::from_arc(&self.result_container),
        ]) else {
            log::warn!("failed to lock input and/or result containers to create grindstone result");
            return;
        };

        let [first, second] = input_container.items() else {
            log::warn!("input_container in anvil menu does not fit expected shape");
            return;
        };

        if first.is_empty() && second.is_empty() {
            result_container.set_item(0, ItemStack::empty());
            return;
        }

        if first.count() <= 1 && second.count() <= 1 {
            if !first.is_empty() && !second.is_empty() {
                result_container.set_item(0, ItemStack::empty());
                // TODO: merge items in grindstone
            } else {
                let item = if first.is_empty() { second } else { first };

                // TODO: check if item is enchanted, not just enchantable
                if item.is_enchanted() {
                    result_container
                        .set_item(0, GrindstoneKind::remove_non_curses_from(item.clone()));
                } else {
                    result_container.set_item(0, ItemStack::empty());
                }
            }
        }
    }

    /// Remove non-curse enchantments from items and returns them
    #[must_use]
    pub fn remove_non_curses_from(mut item: ItemStack) -> ItemStack {
        let Some(enchantments) = item.get_enchantments() else {
            // TODO: Only remove non-curses from enchanted books
            if item.is(&vanilla_items::ENCHANTED_BOOK) {
                return ItemStack::new(&vanilla_items::BOOK);
            }
            return ItemStack::empty();
        };

        let mut new_enchantments = ItemEnchantments::empty();

        enchantments
            .iter()
            .filter(|(id, _)| {
                REGISTRY.enchantments.by_key(id).is_some_and(|enchantment| {
                    REGISTRY
                        .enchantments
                        .is_in_tag(enchantment, &EnchantmentTag::CURSE)
                })
            })
            .for_each(|(id, level)| new_enchantments.set(id.clone(), *level));

        let mut repair_cost = 0;

        for _i in 0..new_enchantments.len() {
            repair_cost = AnvilKind::calculate_increased_repair_cost(repair_cost);
        }

        item.set(REPAIR_COST, repair_cost);
        item.set(ENCHANTMENTS, new_enchantments);
        item
    }
}

impl MenuKind for GrindstoneKind {
    /// Returns true while the original grindstone remains in range.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        let state = self.world.get_block_state(self.block_pos);
        state.get_block() == &vanilla_blocks::GRINDSTONE
            && player.is_within_block_interaction_range_with_buffer(self.block_pos, 4.0)
    }

    fn slots_changed(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        self.create_result(behavior, guard, player);
    }

    /// Clears the virtual result on close. Inputs are drained by [`Menu::removed`].
    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.input_container.lock().set_item(0, ItemStack::empty());
        self.input_container.lock().set_item(1, ItemStack::empty());
        self.result_container.lock().set_item(0, ItemStack::empty());
    }
}
