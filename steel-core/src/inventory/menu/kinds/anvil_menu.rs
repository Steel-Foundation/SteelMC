//! Anvil menu.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI32, Ordering},
};

use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt,
    blocks::block_state_ext::BlockStateExt,
    data_components::{
        components::ItemEnchantments,
        vanilla_components::{CUSTOM_NAME, ENCHANTMENTS, REPAIR_COST, STORED_ENCHANTMENTS},
    },
    enchantment::Enchantment,
    item_stack::ItemStack,
    vanilla_block_tags::BlockTag,
    vanilla_items, vanilla_menu_types,
};
use steel_utils::{
    BlockPos, Identifier, java,
    locks::{IntoShared, Shared, SyncMutex},
    text::DisplayResolutor,
};
use text_components::TextComponent;

use crate::{
    behavior::ITEM_BEHAVIORS,
    inventory::{
        container::{ResultContainer, SimpleContainer},
        prelude::*,
        slots::AnvilResultHandler,
    },
    player::player_inventory::PlayerInventory,
    world::World,
};

/// Builds the anvil menu.
#[must_use]
pub fn anvil(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    pos: BlockPos,
    world: &Arc<World>,
) -> Menu {
    let input_container = SimpleContainer::new(2).into_shared();
    let repair_item_count = Arc::new(AtomicI32::new(0));
    let level_cost = Arc::new(AtomicI32::new(0));
    let only_renaming = Arc::new(AtomicBool::new(false));

    let result_container = ResultContainer::new().into_shared();

    let mut builder = MenuBuilder::new(&vanilla_menu_types::ANVIL, container_id);

    let input = builder.section_all(&input_container);
    let result = builder.result_slot(AnvilResultHandler::new(
        input_container.clone(),
        result_container.clone(),
        repair_item_count.clone(),
        level_cost.clone(),
        only_renaming.clone(),
        pos,
        world.clone(),
    ));

    let player = builder.player_inventory(&inventory);

    let level_cost_data_slot = builder.data_slot(0);

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

    builder.build(AnvilKind {
        input_container,
        result_container,
        block_pos: pos,
        world: Arc::clone(world),
        repair_item_count,
        level_cost: level_cost_data_slot,
        level_cost_value: level_cost,
        only_renaming,
        item_name: SyncMutex::new(None),
    })
}

/// Per-menu anvil state: inputs, result, level cost, and rename text.
pub struct AnvilKind {
    /// Input container (two slots).
    input_container: Shared<SimpleContainer>,
    /// Result container (single virtual slot).
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
    repair_item_count: Arc<AtomicI32>,
    /// Client-facing level cost data slot.
    level_cost: DataSlot,
    /// Level cost shared with [`AnvilResultHandler`], kept in sync with `level_cost`.
    level_cost_value: Arc<AtomicI32>,
    /// Whether the current result changes only the first input's name.
    only_renaming: Arc<AtomicBool>,
    item_name: SyncMutex<Option<String>>,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for AnvilKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/anvil");
}

impl AnvilKind {
    /// Sets the level cost. The client receives the packet's low 16 bits while
    /// the result handler retains the full server-side cost.
    fn set_cost(&mut self, behavior: &mut MenuBehavior, cost: i32) {
        self.level_cost.set(behavior, Self::client_cost(cost));
        self.level_cost_value.store(cost, Ordering::Relaxed);
    }

    const fn client_cost(cost: i32) -> i16 {
        let [low, high, _, _] = cost.to_le_bytes();
        i16::from_le_bytes([low, high])
    }

    /// Builds the anvil result from combining and renaming the two inputs.
    ///
    /// # Panics
    /// Panics if the input container is not exactly two slots.
    #[tracing::instrument(skip(self, behavior, player, guard), level = "info", fields(player = %player.gameprofile.name))]
    #[expect(
        clippy::too_many_lines,
        reason = "mirrors Vanilla's ordered createResult flow in one auditable calculation"
    )]
    pub(crate) fn create_result(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        let Some([input_container, result_container]) = guard.get_disjoint_mut([
            ContainerId::from_arc(&self.input_container),
            ContainerId::from_arc(&self.result_container),
        ]) else {
            panic!("failed to lock input and/or result containers to create anvil result")
        };

        let [first, second] = input_container.items() else {
            panic!("input_container in anvil menu does not fit expected shape")
        };

        let mut additional_cost = 0_u32;
        let mut rename_cost = 0_i32;
        self.only_renaming.store(false, Ordering::Relaxed);
        self.set_cost(behavior, 0);

        if first.is_empty() || !Self::can_store_enchantments(first) {
            result_container.set_item(0, ItemStack::empty());
            self.set_cost(behavior, 0);
            return;
        }

        self.repair_item_count.store(0, Ordering::Relaxed);

        let mut result = first.clone();
        let mut enchantments = first
            .get_enchantments_for_crafting()
            .cloned()
            .unwrap_or_default();
        let prior_repair_cost: i64 = i64::from(*first.get(REPAIR_COST).unwrap_or(&0))
            + i64::from(*second.get(REPAIR_COST).unwrap_or(&0));

        if !second.is_empty() {
            let has_stored_enchantments = second.has(STORED_ENCHANTMENTS);

            if result.is_damageable_item() && first.is_valid_repair_item(second) {
                let mut repair_per_unit =
                    result.get_damage_value().min(result.get_max_damage() / 4);
                if repair_per_unit <= 0 {
                    result_container.set_item(0, ItemStack::empty());
                    self.set_cost(behavior, 0);
                    return;
                }

                let mut materials_used = 0;
                while repair_per_unit > 0 && materials_used < second.count {
                    let new_damage = result.get_damage_value() - repair_per_unit;
                    result.set_damage_value(new_damage);
                    additional_cost += 1;
                    materials_used += 1;
                    repair_per_unit = result.get_damage_value().min(result.get_max_damage() / 4);
                }

                self.repair_item_count
                    .store(materials_used, Ordering::Relaxed);
            } else {
                if !has_stored_enchantments
                    && (!result.is(second.item) || !result.is_damageable_item())
                {
                    result_container.set_item(0, ItemStack::empty());
                    self.set_cost(behavior, 0);
                    return;
                }

                if result.is_damageable_item() && !has_stored_enchantments {
                    // Combining two of the same item.
                    let first_durability = first.get_max_damage() - first.get_damage_value();
                    let second_durability = second.get_max_damage() - second.get_damage_value();
                    let durability_bonus = second_durability + result.get_max_damage() * 12 / 100;
                    let total_durability = first_durability + durability_bonus;
                    let new_damage = (result.get_max_damage() - total_durability).max(0);

                    if new_damage < result.get_damage_value() {
                        result.set_damage_value(new_damage);
                        additional_cost += 2;
                    }
                }

                // Enchantment merging.
                let sacrifice_enchantments: ItemEnchantments = second
                    .get_enchantments_for_crafting()
                    .cloned()
                    .unwrap_or_default();
                let mut any_compatible = false;
                let mut any_incompatible = false;

                for (ident, level) in sacrifice_enchantments {
                    let existing_level = enchantments.get_level(&ident);
                    let mut merged_level: u32 = if existing_level == level {
                        level + 1
                    } else {
                        existing_level.max(level)
                    };

                    let enchantment = REGISTRY
                        .enchantments
                        .by_key(&ident)
                        .expect("should exist because we got it from item enchantments");
                    let mut can_apply = enchantment.can_enchant(first.item)
                        || first.is(&vanilla_items::ENCHANTED_BOOK)
                        || player.has_infinite_materials();

                    for (existing_key, _) in enchantments.iter() {
                        if *existing_key == enchantment.key {
                            continue;
                        }
                        let Some(existing) = REGISTRY.enchantments.by_key(existing_key) else {
                            continue;
                        };
                        if !Enchantment::are_compatible(enchantment, existing) {
                            can_apply = false;
                            additional_cost += 1;
                        }
                    }

                    if can_apply {
                        any_compatible = true;
                        merged_level = merged_level.min(enchantment.max_level);
                        enchantments.set(ident, merged_level);

                        let mut anvil_cost: i32 = enchantment.anvil_cost;
                        if has_stored_enchantments {
                            anvil_cost = (anvil_cost / 2).max(1);
                        }
                        additional_cost += anvil_cost as u32 * merged_level;

                        if first.count > 1 {
                            additional_cost = 40;
                        }
                    } else {
                        any_incompatible = true;
                    }
                }

                if any_incompatible && !any_compatible {
                    result_container.set_item(0, ItemStack::empty());
                    self.set_cost(behavior, 0);
                    return;
                }
            }
        }

        // Renaming
        let item_name = self.item_name.lock();
        if let Some(name) = item_name.as_deref().filter(|name| !java::is_blank(name)) {
            if name != ITEM_BEHAVIORS.hover_name(first).to_plain(&DisplayResolutor) {
                rename_cost = 1;
                additional_cost += rename_cost as u32;
                result.set(CUSTOM_NAME, TextComponent::from(name.to_string()));
            }
        } else if first.has(CUSTOM_NAME) {
            rename_cost = 1;
            additional_cost += rename_cost as u32;
            result.remove(CUSTOM_NAME);
        }
        drop(item_name);

        // Final cost.
        let total_cost = if additional_cost == 0 {
            0
        } else {
            (prior_repair_cost + i64::from(additional_cost)).clamp(0, i64::from(i32::MAX)) as i32
        };
        self.set_cost(behavior, total_cost);

        if additional_cost == 0 {
            result = ItemStack::empty();
        }

        let only_renaming = rename_cost == additional_cost as i32 && rename_cost > 0;
        self.only_renaming.store(only_renaming, Ordering::Relaxed);
        if only_renaming && total_cost >= 40 {
            self.set_cost(behavior, 39);
        }

        if total_cost >= 40 && !only_renaming && !player.has_infinite_materials() {
            result = ItemStack::empty();
        }

        // Write repair cost to result.
        if !result.is_empty() {
            let second_repair_cost = *second.get(REPAIR_COST).unwrap_or(&0);
            let mut final_repair_cost = *result.get(REPAIR_COST).unwrap_or(&0);
            if final_repair_cost < second_repair_cost {
                final_repair_cost = second_repair_cost;
            }
            if rename_cost != additional_cost as i32 || rename_cost == 0 {
                final_repair_cost = Self::calculate_increased_repair_cost(final_repair_cost);
            }
            result.set(REPAIR_COST, final_repair_cost);
            let enchantments: Vec<(Identifier, u32)> =
                enchantments.iter().map(|(k, v)| (k.clone(), *v)).collect();
            result.set_enchantments(&enchantments, false);
        }

        result_container.set_item(0, result.clone());
    }

    fn validate_item_name(name: String) -> Option<String> {
        let filtered = name
            .chars()
            .filter(|char| char != &'§' && char >= &' ' && char != &'\x7F')
            .collect::<String>();
        (filtered.encode_utf16().count() <= 50).then_some(filtered)
    }

    fn can_store_enchantments(item_stack: &ItemStack) -> bool {
        item_stack.has(if item_stack.is(&vanilla_items::ENCHANTED_BOOK) {
            STORED_ENCHANTMENTS
        } else {
            ENCHANTMENTS
        })
    }

    const fn calculate_increased_repair_cost(old_repair_cost: i32) -> i32 {
        old_repair_cost.saturating_mul(2).saturating_add(1)
    }
}

impl MenuKind for AnvilKind {
    /// Returns true while the original anvil remains in range.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        let state = self.world.get_block_state(self.block_pos);
        REGISTRY
            .blocks
            .is_in_tag(state.get_block(), &BlockTag::ANVIL)
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

    /// Sets the rename text and recomputes the result with it applied.
    #[tracing::instrument(skip(self, behavior, player), level = "info")]
    fn on_rename(&mut self, behavior: &mut MenuBehavior, name: String, player: &Player) {
        let Some(validated_name) = Self::validate_item_name(name) else {
            return;
        };

        {
            let mut item_name_guard = self.item_name.lock();
            match &*item_name_guard {
                Some(current) if *current == validated_name => return,
                _ => *item_name_guard = Some(validated_name),
            }
        }

        {
            let mut guard = behavior.lock_all_containers();
            self.create_result(behavior, &mut guard, player);
        }
        behavior.broadcast_changes(&player.connection);
    }
}

#[cfg(test)]
mod tests;
