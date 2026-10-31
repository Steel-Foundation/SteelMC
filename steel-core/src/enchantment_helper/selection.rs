//! Enchanting table costs and weighted enchantment selection.

use steel_math::round_f32;
use steel_registry::data_components::vanilla_components::ENCHANTABLE;
use steel_registry::enchantment::{Enchantment, EnchantmentRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;
use steel_utils::random::{Random, weighted_random};

const MAX_BOOKSHELVES: i32 = 15;
const BASE_COST_RANDOM_BOUND: i32 = 8;
const ENCHANTABILITY_RANDOM_DIVISOR: i32 = 4;
const COST_VARIATION: f32 = 0.15;
const ADDITIONAL_ENCHANTMENT_RANDOM_BOUND: i32 = 50;

/// An enchantment and its level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnchantmentInstance {
    pub enchantment: EnchantmentRef,
    pub level: u32,
}

impl EnchantmentInstance {
    /// Returns the enchantment weight
    #[must_use]
    pub(crate) const fn weight(&self) -> i32 {
        self.enchantment.weight as i32
    }
}

/// Calculates the required level for an enchanting table offer
pub(crate) fn get_enchantment_cost<R: Random>(
    random: &mut R,
    slot: usize,
    bookcases: i32,
    stack: &ItemStack,
) -> i32 {
    if !stack.has(ENCHANTABLE) {
        return 0;
    }
    let bookcases = bookcases.min(MAX_BOOKSHELVES);
    let selected = random.next_i32_bounded(BASE_COST_RANDOM_BOUND)
        + 1
        + (bookcases >> 1)
        + random.next_i32_bounded(bookcases + 1);
    match slot {
        0 => (selected / 3).max(1),
        1 => selected * 2 / 3 + 1,
        _ => selected.max(bookcases * 2),
    }
}

/// Selects enchantments for an enchanting table offer
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
        + random.next_i32_bounded(enchantability / ENCHANTABILITY_RANDOM_DIVISOR + 1)
        + random.next_i32_bounded(enchantability / ENCHANTABILITY_RANDOM_DIVISOR + 1);
    let random_span = (random.next_f32() + random.next_f32() - 1.0) * COST_VARIATION;
    cost = round_f32(cost as f32 + cost as f32 * random_span).max(1);

    let mut candidates = get_available_enchantment_results(cost, stack, source);
    if candidates.is_empty() {
        return results;
    }
    if let Some(first) =
        weighted_random::get_random_item(random, &candidates, EnchantmentInstance::weight)
    {
        results.push(*first);
    }
    while random.next_i32_bounded(ADDITIONAL_ENCHANTMENT_RANDOM_BOUND) <= cost {
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

/// Returns the enchantments available for the given cost and item
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

/// Removes enchantments that are incompatible with `target`
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

    use steel_registry::{init_vanilla_registry, vanilla_enchantments, vanilla_items};
    use steel_utils::random::legacy_random::LegacyRandom;

    use super::*;

    const FULL_POWER_BOOKSHELVES: i32 = 15;
    const HIGH_OFFER_MIN_COST: i32 = 30;
    const SHARPNESS_OVERLAP_COST: i32 = 30;

    fn instance(enchantment: EnchantmentRef, level: u32) -> EnchantmentInstance {
        EnchantmentInstance { enchantment, level }
    }

    #[test]
    fn third_offer_costs_at_least_twice_the_bookcases() {
        init_vanilla_registry();
        let sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        for seed in 0..64 {
            let mut random = LegacyRandom::from_seed(seed);
            assert!(
                get_enchantment_cost(&mut random, 2, FULL_POWER_BOOKSHELVES, &sword)
                    >= HIGH_OFFER_MIN_COST
            );
        }
    }

    #[test]
    fn available_results_pick_the_highest_level_whose_window_contains_the_value() {
        init_vanilla_registry();
        let sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        // Sharpness level n spans [1 + 11(n - 1), 21 + 11(n - 1)]; 30 fits levels 2 and 3.
        let results = get_available_enchantment_results(
            SHARPNESS_OVERLAP_COST,
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
            get_available_enchantment_results(SHARPNESS_OVERLAP_COST, &book, sharpness.clone())
                .len(),
            1
        );
        let axe = ItemStack::new(&vanilla_items::DIAMOND_AXE);
        assert_eq!(
            get_available_enchantment_results(SHARPNESS_OVERLAP_COST, &axe, sharpness),
            Vec::new()
        );
    }
}
