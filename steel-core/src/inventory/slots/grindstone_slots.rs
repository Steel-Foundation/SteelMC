//! Result slot handler for the grindstone menu.
//!
//! Mirrors vanilla `GrindstoneMenu`: the result is recomputed whenever an input
//! changes, and taking it awards the stripped enchantment levels as experience
//! orbs before clearing both inputs.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::data_components::vanilla_components::{
    ENCHANTMENTS, ItemEnchantments, MAX_DAMAGE, REPAIR_COST, STORED_ENCHANTMENTS,
};
use steel_registry::enchantment::{Enchantment, EnchantmentRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    REGISTRY, RegistryExt as _, TaggedRegistryExt as _, level_events, vanilla_enchantment_tags,
    vanilla_items,
};
use steel_utils::BlockPos;
use steel_utils::locks::Shared;

use crate::entity::entities::ExperienceOrbEntity;
use crate::inventory::container::{ResultContainer, SimpleContainer};
use crate::inventory::lock::{ContainerId, ContainerLockGuard, ContainerRef};
use crate::inventory::slots::ResultHandler;
use crate::player::Player;
use crate::world::World;

/// Result slot handler for a grindstone.
#[derive(Clone)]
pub struct GrindstoneResultHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
}

impl GrindstoneResultHandler {
    /// Creates a new handler for the grindstone at `block_pos`.
    pub const fn new(
        input_container: Shared<SimpleContainer>,
        result_container: Shared<ResultContainer>,
        block_pos: BlockPos,
        world: Arc<World>,
    ) -> Self {
        Self {
            input_container,
            result_container,
            block_pos,
            world,
        }
    }

    /// Shared handle to the two-slot input container.
    #[must_use]
    pub fn input_container(&self) -> Shared<SimpleContainer> {
        self.input_container.clone()
    }

    /// Shared handle to the single-slot result container.
    #[must_use]
    pub fn result_container_handle(&self) -> Shared<ResultContainer> {
        self.result_container.clone()
    }

    fn input_id(&self) -> ContainerId {
        ContainerId::from_arc(&self.input_container)
    }

    fn result_id(&self) -> ContainerId {
        ContainerId::from_arc(&self.result_container)
    }
}

impl ResultHandler for GrindstoneResultHandler {
    fn result_container(&self) -> ContainerRef {
        ContainerRef::from(self.result_container.clone())
    }

    fn dependencies(&self) -> Vec<ContainerRef> {
        vec![ContainerRef::from(self.input_container.clone())]
    }

    fn update_result(&self, guard: &mut ContainerLockGuard) {
        let (first, second) = {
            let input = guard
                .get(self.input_id())
                .expect("grindstone input container not locked");
            (input.get_item(0).clone(), input.get_item(1).clone())
        };

        let result = compute_result(&first, &second);

        let container = guard
            .get_mut(self.result_id())
            .expect("grindstone result container not locked");
        container.set_item(0, result);
        container.set_changed();
    }

    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) -> Option<ItemStack> {
        let raw_experience = {
            let input = guard
                .get(self.input_id())
                .expect("grindstone input container not locked");
            experience_from_item(input.get_item(0)) + experience_from_item(input.get_item(1))
        };
        award_experience(&self.world, self.block_pos, raw_experience);
        self.world
            .level_event(level_events::SOUND_GRINDSTONE_USED, self.block_pos, 0, None);

        {
            let input = guard
                .get_mut(self.input_id())
                .expect("grindstone input container not locked");
            input.set_item(0, ItemStack::empty());
            input.set_item(1, ItemStack::empty());
            input.set_changed();
        }

        self.update_result(guard);
        None
    }

    fn is_result_valid(&self, guard: &ContainerLockGuard, _player: &Player) -> bool {
        let Some(result) = guard.get(self.result_id()) else {
            return false;
        };
        let stored = result.get_item(0);
        if stored.is_empty() {
            return false;
        }

        let Some(input) = guard.get(self.input_id()) else {
            return false;
        };
        let expected = compute_result(input.get_item(0), input.get_item(1));
        ItemStack::matches(stored, &expected)
    }
}

/// Whether an item may be placed into a grindstone input slot.
///
/// Vanilla `GrindstoneMenu` accepts damageable items and anything carrying
/// enchantments (including enchanted books via stored enchantments).
#[must_use]
pub fn grindstone_input_allows(stack: &ItemStack) -> bool {
    stack.is_damageable_item() || has_any_enchantments(stack)
}

/// Vanilla `EnchantmentHelper.hasAnyEnchantments`: true when either the
/// `enchantments` or `stored_enchantments` component is present and non-empty.
fn has_any_enchantments(stack: &ItemStack) -> bool {
    stack.get(ENCHANTMENTS).is_some_and(|e| !e.is_empty())
        || stack
            .get(STORED_ENCHANTMENTS)
            .is_some_and(|e| !e.is_empty())
}

fn is_curse(enchantment: EnchantmentRef) -> bool {
    REGISTRY.enchantments.is_in_tag(
        enchantment,
        &vanilla_enchantment_tags::EnchantmentTag::CURSE,
    )
}

/// Vanilla `Enchantment.Cost.calculate`: `base + per_level_above_first * (level - 1)`.
const fn enchantment_min_cost(enchantment: &Enchantment, level: u32) -> i32 {
    enchantment.min_cost.base + enchantment.min_cost.per_level_above_first * (level as i32 - 1)
}

/// Vanilla `GrindstoneMenu.getExperienceFromItem`: sum of the minimum enchanting
/// cost of every non-curse enchantment on the item.
fn experience_from_item(item: &ItemStack) -> i32 {
    let Some(enchantments) = item.get_enchantments_for_crafting() else {
        return 0;
    };

    let mut amount = 0;
    for (key, level) in enchantments.iter() {
        let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
            continue;
        };
        if !is_curse(enchantment) {
            amount += enchantment_min_cost(enchantment, *level);
        }
    }
    amount
}

/// Vanilla `GrindstoneMenu.getExperienceAmount` combined with `ExperienceOrb.award`:
/// `ceil(raw / 2) + random(ceil(raw / 2))`, spawned at the block center.
fn award_experience(world: &Arc<World>, pos: BlockPos, raw_experience: i32) {
    if raw_experience <= 0 {
        return;
    }
    // Vanilla `(int)Math.ceil(raw / 2.0)`; `raw` is positive here.
    let half = (raw_experience + 1) / 2;
    let amount = half + rand::random_range(0..half);
    ExperienceOrbEntity::award(world, block_center(pos), amount);
}

fn block_center(pos: BlockPos) -> DVec3 {
    DVec3::new(
        f64::from(pos.x()) + 0.5,
        f64::from(pos.y()) + 0.5,
        f64::from(pos.z()) + 0.5,
    )
}

/// Vanilla `GrindstoneMenu.computeResult`.
fn compute_result(input: &ItemStack, additional: &ItemStack) -> ItemStack {
    if input.is_empty() && additional.is_empty() {
        return ItemStack::empty();
    }
    if input.count() > 1 || additional.count() > 1 {
        return ItemStack::empty();
    }

    if input.is_empty() || additional.is_empty() {
        let item = if input.is_empty() { additional } else { input };
        if !has_any_enchantments(item) {
            return ItemStack::empty();
        }
        let mut stripped = item.clone();
        remove_non_curses_from(&mut stripped);
        return stripped;
    }

    merge_items(input, additional)
}

/// Vanilla `GrindstoneMenu.mergeItems`.
fn merge_items(input: &ItemStack, additional: &ItemStack) -> ItemStack {
    if !input.is(additional.item) {
        return ItemStack::empty();
    }

    let durability = input.get_max_damage().max(additional.get_max_damage());
    let remaining_input = input.get_max_damage() - input.get_damage_value();
    let remaining_additional = additional.get_max_damage() - additional.get_damage_value();
    let remaining = remaining_input + remaining_additional + durability * 5 / 100;

    let mut count = 1;
    if !input.is_damageable_item() {
        if input.max_stack_size() < 2 || !ItemStack::matches(input, additional) {
            return ItemStack::empty();
        }
        count = 2;
    }

    let mut result = input.copy_with_count(count);
    if result.is_damageable_item() {
        result.set(MAX_DAMAGE, durability);
        result.set_damage_value((durability - remaining).max(0));
    }

    merge_enchants_from(&mut result, additional);
    remove_non_curses_from(&mut result);
    result
}

/// Vanilla `GrindstoneMenu.mergeEnchantsFrom`: pull the sacrifice's enchantments
/// onto the target, keeping the higher level. Curses only transfer when the
/// target does not already carry them.
fn merge_enchants_from(target: &mut ItemStack, source: &ItemStack) {
    let Some(source_enchantments) = source.get_enchantments_for_crafting().cloned() else {
        return;
    };

    let mut merged = target
        .get_enchantments_for_crafting()
        .cloned()
        .unwrap_or_default();

    for (key, level) in source_enchantments.iter() {
        let cursed = REGISTRY.enchantments.by_key(key).is_some_and(is_curse);
        if !cursed || merged.get_level(key) == 0 {
            merged.upgrade(key.clone(), *level);
        }
    }

    write_enchantments(target, merged);
}

/// Vanilla `GrindstoneMenu.removeNonCursesFrom`: strip every non-curse
/// enchantment, demote an emptied enchanted book to a plain book, and rebuild the
/// repair cost from the surviving curses.
fn remove_non_curses_from(item: &mut ItemStack) {
    let current = item
        .get_enchantments_for_crafting()
        .cloned()
        .unwrap_or_default();

    let mut kept = ItemEnchantments::empty();
    for (key, level) in current.iter() {
        if REGISTRY.enchantments.by_key(key).is_some_and(is_curse) {
            kept.set(key.clone(), *level);
        }
    }
    let kept_count = kept.len();
    write_enchantments(item, kept);

    if item.is(&vanilla_items::ENCHANTED_BOOK) && kept_count == 0 {
        item.set_item(&vanilla_items::BOOK.key);
    }

    let mut repair_cost = 0;
    for _ in 0..kept_count {
        repair_cost = calculate_increased_repair_cost(repair_cost);
    }
    item.set(REPAIR_COST, repair_cost);
}

fn write_enchantments(item: &mut ItemStack, enchantments: ItemEnchantments) {
    if item.is(&vanilla_items::ENCHANTED_BOOK) {
        item.set(STORED_ENCHANTMENTS, enchantments);
    } else {
        item.set(ENCHANTMENTS, enchantments);
    }
}

/// Vanilla `AnvilMenu.calculateIncreasedRepairCost`: `cost * 2 + 1`.
const fn calculate_increased_repair_cost(cost: i32) -> i32 {
    cost.saturating_mul(2).saturating_add(1)
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::vanilla_components::REPAIR_COST;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::items::ItemRef;
    use steel_registry::{init_vanilla_registry, vanilla_enchantments, vanilla_items};
    use steel_utils::Identifier;

    use super::{compute_result, experience_from_item};

    fn enchanted(item: ItemRef, entries: &[(&Identifier, u32)]) -> ItemStack {
        let mut stack = ItemStack::new(item);
        let owned: Vec<_> = entries
            .iter()
            .map(|(key, level)| ((*key).clone(), *level))
            .collect();
        stack.set_enchantments(&owned, false);
        stack
    }

    #[test]
    fn a_single_unenchanted_item_produces_no_result() {
        init_vanilla_registry();
        let result = compute_result(
            &ItemStack::new(&vanilla_items::DIAMOND_SWORD),
            &ItemStack::empty(),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn disenchanting_keeps_only_curses_and_resets_repair_cost() {
        init_vanilla_registry();
        let sword = enchanted(
            &vanilla_items::DIAMOND_SWORD,
            &[
                (&vanilla_enchantments::SHARPNESS.key, 4),
                (&vanilla_enchantments::VANISHING_CURSE.key, 1),
            ],
        );

        let result = compute_result(&sword, &ItemStack::empty());

        assert!(result.is(&vanilla_items::DIAMOND_SWORD));
        let enchantments = result
            .get_enchantments_for_crafting()
            .expect("the surviving curse keeps an enchantments component");
        assert_eq!(
            enchantments.get_level(&vanilla_enchantments::SHARPNESS.key),
            0
        );
        assert_eq!(
            enchantments.get_level(&vanilla_enchantments::VANISHING_CURSE.key),
            1
        );
        // One surviving enchantment -> repair cost `0 * 2 + 1`.
        assert_eq!(result.get(REPAIR_COST), Some(&1));
    }

    #[test]
    fn an_enchanted_book_with_only_removable_enchantments_becomes_a_book() {
        init_vanilla_registry();
        let book = enchanted(
            &vanilla_items::ENCHANTED_BOOK,
            &[(&vanilla_enchantments::UNBREAKING.key, 3)],
        );

        let result = compute_result(&book, &ItemStack::empty());

        assert!(result.is(&vanilla_items::BOOK));
    }

    #[test]
    fn merging_two_damaged_tools_repairs_with_a_five_percent_bonus() {
        init_vanilla_registry();
        let mut first = ItemStack::new(&vanilla_items::IRON_PICKAXE);
        let mut second = ItemStack::new(&vanilla_items::IRON_PICKAXE);
        first.set_damage_value(200);
        second.set_damage_value(200);
        let max_damage = first.get_max_damage();

        let result = compute_result(&first, &second);

        assert!(result.is(&vanilla_items::IRON_PICKAXE));
        assert_eq!(result.count(), 1);
        let remaining = (max_damage - 200) * 2 + max_damage * 5 / 100;
        assert_eq!(result.get_damage_value(), (max_damage - remaining).max(0));
    }

    #[test]
    fn merging_two_different_items_produces_no_result() {
        init_vanilla_registry();
        let result = compute_result(
            &ItemStack::new(&vanilla_items::IRON_PICKAXE),
            &ItemStack::new(&vanilla_items::IRON_AXE),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn experience_only_counts_non_curse_enchantments() {
        init_vanilla_registry();
        let cursed_only = enchanted(
            &vanilla_items::DIAMOND_SWORD,
            &[(&vanilla_enchantments::VANISHING_CURSE.key, 1)],
        );
        assert_eq!(experience_from_item(&cursed_only), 0);

        let sharpened = enchanted(
            &vanilla_items::DIAMOND_SWORD,
            &[(&vanilla_enchantments::SHARPNESS.key, 1)],
        );
        assert_eq!(
            experience_from_item(&sharpened),
            vanilla_enchantments::SHARPNESS.min_cost.base
        );
    }
}
