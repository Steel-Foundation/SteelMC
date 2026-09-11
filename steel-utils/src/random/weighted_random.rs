//! Vanilla `WeightedRandom`: weighted selection over an arbitrary slice.

use crate::random::Random;

/// Sums `weight` over `items`, mirroring vanilla `WeightedRandom.getTotalWeight`.
///
/// # Panics
/// Panics if the sum exceeds `i32::MAX`, like vanilla's `IllegalArgumentException`.
#[must_use]
pub fn get_total_weight<T>(items: &[T], weight: impl Fn(&T) -> i32) -> i32 {
    let total: i64 = items.iter().map(|item| i64::from(weight(item))).sum();
    assert!(
        total <= i64::from(i32::MAX),
        "Sum of weights must be <= 2147483647"
    );
    total as i32
}

/// Picks one entry with probability proportional to its weight, mirroring
/// vanilla `WeightedRandom.getRandomItem`. Returns `None`, without touching
/// `random`, when the total weight is zero.
///
/// # Panics
/// Panics if the total weight is negative, like vanilla.
pub fn get_random_item<'a, T, R: Random>(
    random: &mut R,
    items: &'a [T],
    weight: impl Fn(&T) -> i32,
) -> Option<&'a T> {
    let total_weight = get_total_weight(items, &weight);
    assert!(total_weight >= 0, "Negative total weight in getRandomItem");
    if total_weight == 0 {
        return None;
    }
    let selection = random.next_i32_bounded(total_weight);
    get_weighted_item(items, selection, weight)
}

/// Walks `items` subtracting each weight from `index` and returns the entry
/// that drives it negative, mirroring vanilla `WeightedRandom.getWeightedItem`.
#[must_use]
pub fn get_weighted_item<T>(items: &[T], mut index: i32, weight: impl Fn(&T) -> i32) -> Option<&T> {
    items.iter().find(|item| {
        index -= weight(item);
        index < 0
    })
}

#[cfg(test)]
mod tests {
    use crate::random::legacy_random::LegacyRandom;

    use super::{get_random_item, get_total_weight, get_weighted_item};

    const ITEMS: [(&str, i32); 3] = [("common", 3), ("rare", 1), ("never", 0)];

    fn weight(item: &(&str, i32)) -> i32 {
        item.1
    }

    #[test]
    fn total_weight_sums_every_entry() {
        assert_eq!(get_total_weight(&ITEMS, weight), 4);
        assert_eq!(get_total_weight::<(&str, i32)>(&[], weight), 0);
    }

    #[test]
    fn weighted_item_walks_cumulative_weights() {
        for index in 0..3 {
            assert_eq!(get_weighted_item(&ITEMS, index, weight), Some(&ITEMS[0]));
        }
        assert_eq!(get_weighted_item(&ITEMS, 3, weight), Some(&ITEMS[1]));
        assert_eq!(get_weighted_item(&ITEMS, 4, weight), None);
    }

    #[test]
    fn zero_total_weight_yields_nothing_without_consuming_the_random() {
        let mut random = LegacyRandom::from_seed(5);
        let before = random.get_seed();
        let empty: [(&str, i32); 1] = [("never", 0)];
        assert_eq!(get_random_item(&mut random, &empty, weight), None);
        assert_eq!(random.get_seed(), before);
    }

    #[test]
    fn random_item_is_deterministic_for_a_seed() {
        let first = get_random_item(&mut LegacyRandom::from_seed(1234), &ITEMS, weight);
        let second = get_random_item(&mut LegacyRandom::from_seed(1234), &ITEMS, weight);
        assert!(first.is_some());
        assert_eq!(first, second);
    }
}
