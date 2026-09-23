//! Grindstone menu.
use std::sync::Arc;

use steel_registry::data_components::vanilla_components::MAX_DAMAGE;
use steel_registry::vanilla_blocks;
use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt,
    blocks::block_state_ext::BlockStateExt,
    data_components::{
        components::ItemEnchantments,
        vanilla_components::{ENCHANTMENTS, REPAIR_COST, STORED_ENCHANTMENTS},
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

use std::cmp::max;

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

    let input = builder.section_all_with(
        &input_container,
        SectionKind::restricted(|_index, item_stack| {
            item_stack.is_damageable_item()
                || item_stack
                    .get_enchantments_for_crafting()
                    .is_some_and(|e| !e.is_empty())
        }),
    );
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
    builder.drain(input);

    builder.build(GrindstoneKind {
        input_container,
        result_container,
        block_pos: pos,
        world: Arc::clone(world),
        input,
        player,
    })
}

/// Per-menu grindstone state: the two inputs and the virtual result.
pub struct GrindstoneKind {
    /// Input container (two slots).
    input_container: Shared<SimpleContainer>,
    /// Result container (single virtual slot).
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
    input: Section,
    player: PlayerInventorySections,
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
            log::warn!("input_container in grindstone menu does not fit expected shape");
            return;
        };

        if first.is_empty() && second.is_empty() {
            result_container.set_item(0, ItemStack::empty());
            return;
        }

        if first.count() <= 1 && second.count() <= 1 {
            if !first.is_empty() && !second.is_empty() {
                result_container.set_item(
                    0,
                    GrindstoneKind::merge_items(first.clone(), second.clone()),
                );
            } else {
                let item = if first.is_empty() { second } else { first };

                if item
                    .get_enchantments_for_crafting()
                    .is_some_and(|e| !e.is_empty())
                {
                    result_container
                        .set_item(0, GrindstoneKind::remove_non_curses_from(item.clone()));
                } else {
                    result_container.set_item(0, ItemStack::empty());
                }
            }
        } else {
            result_container.set_item(0, ItemStack::empty());
        }
    }

    /// Merges two items and their enchants but gets rid of their non-curse enchants
    #[must_use]
    fn merge_items(first: ItemStack, second: ItemStack) -> ItemStack {
        if !first.is(second.item()) {
            return ItemStack::empty();
        }

        let durability = max(first.get_max_damage(), second.get_max_damage());
        let remaining1 = first.get_max_damage() - first.get_damage_value();
        let remaining2 = second.get_max_damage() - second.get_damage_value();
        let remaining = remaining1 + remaining2 + durability * 5 / 100;
        let mut count = 1;

        if !first.is_damageable_item() {
            count = 2;

            if first.max_stack_size() < 2 || !ItemStack::matches(&first, &second) {
                return ItemStack::empty();
            }
        }

        let mut new_item = first.copy_with_count(count);
        if new_item.is_damageable_item() {
            new_item.set(MAX_DAMAGE, durability);
            new_item.set_damage_value(max(durability - remaining, 0));
        }

        GrindstoneKind::merge_enchantments_from(&mut new_item, &second);
        GrindstoneKind::remove_non_curses_from(new_item)
    }

    /// Copies `source`'s enchantments onto `target`, keeping the higher level when
    /// both carry the same one. Curses only transfer if `target` does not already
    /// have them, so merging two cursed items cannot stack the curse level.
    fn merge_enchantments_from(target: &mut ItemStack, source: &ItemStack) {
        let Some(source_enchantments) = source.get_enchantments_for_crafting() else {
            return;
        };

        for (id, level) in source_enchantments.iter() {
            let is_curse = REGISTRY.enchantments.by_key(id).is_some_and(|enchantment| {
                REGISTRY
                    .enchantments
                    .is_in_tag(enchantment, &EnchantmentTag::CURSE)
            });

            let target_level = target
                .get_enchantments_for_crafting()
                .map_or(0, |enchantments| enchantments.get_level(id));

            if !is_curse || target_level == 0 {
                target.upgrade_enchantment(id.clone(), *level);
            }
        }
    }

    /// Remove non-curse enchantments from items and returns them
    #[must_use]
    pub fn remove_non_curses_from(mut item: ItemStack) -> ItemStack {
        let Some(enchantments) = item.get_enchantments_for_crafting() else {
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
        for _ in 0..new_enchantments.len() {
            repair_cost = AnvilKind::calculate_increased_repair_cost(repair_cost);
        }

        item.set(REPAIR_COST, repair_cost);
        if item.is(&vanilla_items::ENCHANTED_BOOK) {
            if new_enchantments.is_empty() {
                // Clear before transmuting: BOOK has no default to sanitize against.
                item.set(STORED_ENCHANTMENTS, new_enchantments);
                item.set_item(&vanilla_items::BOOK.key);
                return item;
            }
            item.set(STORED_ENCHANTMENTS, new_enchantments);
        } else {
            item.set(ENCHANTMENTS, new_enchantments);
        }

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
        self.result_container.lock().set_item(0, ItemStack::empty());
    }

    fn quick_move(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> Option<ItemStack> {
        if !self.player.all().contains(slot_index) {
            return None;
        }

        let clicked = behavior.slots()[slot_index].get_item(guard).clone();
        if clicked.is_empty() {
            return Some(ItemStack::empty());
        }

        let Some(inputs) = guard.get(ContainerId::from_arc(&self.input_container)) else {
            log::warn!("input container not locked while quick-moving in the grindstone menu");
            return Some(ItemStack::empty());
        };
        let both_inputs_filled = !inputs.get_item(0).is_empty() && !inputs.get_item(1).is_empty();

        let mut remaining = clicked.clone();
        let destination = if !both_inputs_filled {
            self.input
        } else if self.player.main().contains(slot_index) {
            self.player.hotbar()
        } else {
            self.player.main()
        };

        let moved = behavior.move_item_stack_to(
            guard,
            slot_index,
            &mut remaining,
            destination.start(),
            destination.end(),
            FillDirection::Forward,
        );
        if !moved {
            return Some(ItemStack::empty());
        }

        behavior.update_quick_move_source(guard, slot_index, &remaining, &clicked);
        if remaining.count() == clicked.count() {
            return Some(ItemStack::empty());
        }
        if let Some(remainder) = behavior.slots()[slot_index].on_take(guard, &remaining, player) {
            player.add_item_or_drop_with_guard(guard, remainder);
        }
        Some(clicked)
    }
}
