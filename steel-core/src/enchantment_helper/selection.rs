//! Vanilla `EnchantmentHelper` enchanting-table selection: the cost roll, the
//! candidate window scan, and the weighted picks that build an offer.

use steel_registry::data_components::vanilla_components::ENCHANTABLE;
use steel_registry::enchantment::{Enchantment, EnchantmentRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;
use steel_utils::java;
use steel_utils::random::{Random, weighted_random};

/// Vanilla `EnchantmentInstance`: one enchantment at one level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnchantmentInstance {
    pub enchantment: EnchantmentRef,
    pub level: u32,
}

impl EnchantmentInstance {
    /// Vanilla `EnchantmentInstance.weight`.
    #[must_use]
    pub(crate) const fn weight(&self) -> i32 {
        self.enchantment.weight as i32
    }
}

/// Vanilla `EnchantmentHelper.getEnchantmentCost`: the level shown on offer `slot`.
pub(crate) fn get_enchantment_cost<R: Random>(
    random: &mut R,
    slot: usize,
    bookcases: i32,
    stack: &ItemStack,
) -> i32 {
    if !stack.has(ENCHANTABLE) {
        return 0;
    }
    let bookcases = bookcases.min(15);
    let selected =
        random.next_i32_bounded(8) + 1 + (bookcases >> 1) + random.next_i32_bounded(bookcases + 1);
    match slot {
        0 => (selected / 3).max(1),
        1 => selected * 2 / 3 + 1,
        _ => selected.max(bookcases * 2),
    }
}

/// Vanilla `EnchantmentHelper.selectEnchantment`: rolls the modified level and
/// draws weighted, mutually compatible enchantments from `source`.
pub(crate) fn select_enchantment<R: Random>(
    random: &mut R,
    stack: &ItemStack,
    enchantment_cost: i32,
    source: impl Iterator<Item = EnchantmentRef>,
) -> Vec<EnchantmentInstance> {
    let mut results = Vec::new();
    let Some(enchantable) = stack.get(ENCHANTABLE) else {
        return results;
    };
    let enchantability = enchantable.value();
    let mut cost = enchantment_cost
        + 1
        + random.next_i32_bounded(enchantability / 4 + 1)
        + random.next_i32_bounded(enchantability / 4 + 1);
    let random_span = (random.next_f32() + random.next_f32() - 1.0) * 0.15;
    cost = java::round_f32(cost as f32 + cost as f32 * random_span).max(1);

    let mut candidates = get_available_enchantment_results(cost, stack, source);
    if candidates.is_empty() {
        return results;
    }
    if let Some(first) =
        weighted_random::get_random_item(random, &candidates, EnchantmentInstance::weight)
    {
        results.push(*first);
    }
    while random.next_i32_bounded(50) <= cost {
        if let Some(last) = results.last() {
            filter_compatible_enchantments(&mut candidates, last);
        }
        if candidates.is_empty() {
            break;
        }
        if let Some(pick) =
            weighted_random::get_random_item(random, &candidates, EnchantmentInstance::weight)
        {
            results.push(*pick);
        }
        cost /= 2;
    }
    results
}

/// Vanilla `EnchantmentHelper.getAvailableEnchantmentResults`: for every
/// enchantment applicable to `stack` (books accept all), the highest level whose
/// cost window contains `value`.
fn get_available_enchantment_results(
    value: i32,
    stack: &ItemStack,
    source: impl Iterator<Item = EnchantmentRef>,
) -> Vec<EnchantmentInstance> {
    let is_book = stack.is(&vanilla_items::BOOK);
    let mut results = Vec::new();
    for enchantment in source {
        if !is_book && !enchantment.is_primary_item(stack.item()) {
            continue;
        }
        for level in (1..=enchantment.max_level).rev() {
            let level_cost = level as i32;
            if value >= enchantment.get_min_cost(level_cost)
                && value <= enchantment.get_max_cost(level_cost)
            {
                results.push(EnchantmentInstance { enchantment, level });
                break;
            }
        }
    }
    results
}

/// Vanilla `EnchantmentHelper.filterCompatibleEnchantments`: drops `target`
/// itself and everything in an exclusive set with it.
fn filter_compatible_enchantments(
    candidates: &mut Vec<EnchantmentInstance>,
    target: &EnchantmentInstance,
) {
    candidates
        .retain(|candidate| Enchantment::are_compatible(target.enchantment, candidate.enchantment));
}

#[cfg(test)]
mod tests {
    use std::iter::once;

    use steel_registry::vanilla_enchantment_tags::EnchantmentTag;
    use steel_registry::{
        REGISTRY, TaggedRegistryExt, init_vanilla_registry, vanilla_enchantments, vanilla_items,
    };
    use steel_utils::random::legacy_random::LegacyRandom;

    use super::*;

    fn table_enchantments() -> Box<dyn Iterator<Item = EnchantmentRef>> {
        Box::new(
            REGISTRY
                .enchantments
                .iter_tag(&EnchantmentTag::IN_ENCHANTING_TABLE),
        )
    }

    fn instance(enchantment: EnchantmentRef, level: u32) -> EnchantmentInstance {
        EnchantmentInstance { enchantment, level }
    }

    #[test]
    fn cost_is_zero_for_items_without_enchantability() {
        init_vanilla_registry();
        let mut random = LegacyRandom::from_seed(1);
        let stone = ItemStack::new(&vanilla_items::STONE);
        assert_eq!(get_enchantment_cost(&mut random, 2, 15, &stone), 0);
    }

    #[test]
    fn cost_roll_caps_bookcases_at_fifteen() {
        init_vanilla_registry();
        let sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        for slot in 0..3 {
            let mut capped = LegacyRandom::from_seed(7);
            let mut excessive = LegacyRandom::from_seed(7);
            assert_eq!(
                get_enchantment_cost(&mut capped, slot, 15, &sword),
                get_enchantment_cost(&mut excessive, slot, 40, &sword),
            );
        }
    }

    #[test]
    fn third_offer_costs_at_least_twice_the_bookcases() {
        init_vanilla_registry();
        let sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        for seed in 0..64 {
            let mut random = LegacyRandom::from_seed(seed);
            assert!(get_enchantment_cost(&mut random, 2, 15, &sword) >= 30);
        }
    }

    #[test]
    fn available_results_pick_the_highest_level_whose_window_contains_the_value() {
        init_vanilla_registry();
        let sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        // Sharpness level n spans [1 + 11(n - 1), 21 + 11(n - 1)]; 30 fits levels 2 and 3.
        let results = get_available_enchantment_results(
            30,
            &sword,
            once(&vanilla_enchantments::SHARPNESS as EnchantmentRef),
        );
        assert_eq!(results, vec![instance(&vanilla_enchantments::SHARPNESS, 3)]);
    }

    #[test]
    fn books_accept_enchantments_that_are_not_primary_for_other_items() {
        init_vanilla_registry();
        let sharpness = once(&vanilla_enchantments::SHARPNESS as EnchantmentRef);
        let book = ItemStack::new(&vanilla_items::BOOK);
        assert_eq!(
            get_available_enchantment_results(30, &book, sharpness.clone()).len(),
            1
        );
        let axe = ItemStack::new(&vanilla_items::DIAMOND_AXE);
        assert_eq!(
            get_available_enchantment_results(30, &axe, sharpness),
            Vec::new()
        );
    }

    #[test]
    fn filter_removes_the_target_and_its_exclusive_set() {
        init_vanilla_registry();
        let mut candidates = vec![
            instance(&vanilla_enchantments::SHARPNESS, 1),
            instance(&vanilla_enchantments::SMITE, 1),
            instance(&vanilla_enchantments::UNBREAKING, 1),
        ];
        filter_compatible_enchantments(
            &mut candidates,
            &instance(&vanilla_enchantments::SHARPNESS, 1),
        );
        assert_eq!(
            candidates,
            vec![instance(&vanilla_enchantments::UNBREAKING, 1)]
        );
    }

    #[test]
    fn selected_enchantments_are_deterministic_and_pairwise_compatible() {
        init_vanilla_registry();
        let sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        for seed in 0..200 {
            let first = select_enchantment(
                &mut LegacyRandom::from_seed(seed),
                &sword,
                30,
                table_enchantments(),
            );
            let second = select_enchantment(
                &mut LegacyRandom::from_seed(seed),
                &sword,
                30,
                table_enchantments(),
            );
            assert_eq!(first, second);
            assert_ne!(first, Vec::new());
            for (index, earlier) in first.iter().enumerate() {
                for later in &first[index + 1..] {
                    assert!(Enchantment::are_compatible(
                        earlier.enchantment,
                        later.enchantment
                    ));
                }
            }
        }
    }

    #[test]
    fn selection_yields_nothing_for_items_without_enchantability() {
        init_vanilla_registry();
        let stone = ItemStack::new(&vanilla_items::STONE);
        assert_eq!(
            select_enchantment(
                &mut LegacyRandom::from_seed(3),
                &stone,
                30,
                table_enchantments()
            ),
            Vec::new()
        );
    }
}
