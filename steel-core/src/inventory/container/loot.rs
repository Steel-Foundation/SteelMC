//! Vanilla `LootTable.fill` placement into a container.

use std::mem;

use rand::{Rng, RngExt};
use steel_registry::item_stack::ItemStack;

use crate::inventory::container::Container;

/// Places generated loot into empty container slots.
///
/// Mirrors vanilla `LootTable.fill` after `getRandomItems`: empty slots are
/// shuffled, stacks with count greater than 1 may be split to occupy extra
/// slots, then items are written from the end of the shuffled slot list.
pub fn fill_container<R: Rng>(container: &mut dyn Container, items: Vec<ItemStack>, rng: &mut R) {
    let mut items = items;
    let mut available_slots = available_slots(container, rng);
    shuffle_and_split_items(&mut items, available_slots.len(), rng);

    for item in items {
        let Some(slot) = available_slots.pop() else {
            log::warn!("Tried to over-fill a container");
            return;
        };
        container.set_item(slot, item);
    }
}

fn available_slots<R: Rng>(container: &dyn Container, rng: &mut R) -> Vec<usize> {
    let mut slots: Vec<usize> = (0..container.get_container_size())
        .filter(|&slot| container.get_item(slot).is_empty())
        .collect();
    shuffle_indices(&mut slots, rng);
    slots
}

fn shuffle_and_split_items<R: Rng>(
    items: &mut Vec<ItemStack>,
    available_slots: usize,
    rng: &mut R,
) {
    let mut splittable = Vec::new();
    items.retain_mut(|item| {
        if item.is_empty() {
            false
        } else if item.count() > 1 {
            splittable.push(mem::take(item));
            false
        } else {
            true
        }
    });

    while available_slots > items.len() + splittable.len() && !splittable.is_empty() {
        let index = next_int_inclusive(rng, 0, splittable.len() as i32 - 1) as usize;
        let mut stack = splittable.swap_remove(index);
        let remove = next_int_inclusive(rng, 1, stack.count() / 2);
        let copy = stack.split(remove);
        if stack.count() > 1 && rng.random_bool(0.5) {
            splittable.push(stack);
        } else {
            items.push(stack);
        }
        if copy.count() > 1 && rng.random_bool(0.5) {
            splittable.push(copy);
        } else {
            items.push(copy);
        }
    }

    items.append(&mut splittable);
    shuffle(items, rng);
}

fn shuffle_indices<R: Rng>(values: &mut [usize], rng: &mut R) {
    shuffle(values, rng);
}

fn shuffle<T, R: Rng>(values: &mut [T], rng: &mut R) {
    for i in (1..values.len()).rev() {
        let j = rng.random_range(0..=i);
        values.swap(i, j);
    }
}

/// Vanilla `Mth.nextInt(random, min, max)`: inclusive, returns `min` when `min >= max`.
fn next_int_inclusive<R: Rng>(rng: &mut R, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.random_range(min..=max)
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng as _;
    use rand::rngs::StdRng;
    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::*;
    use crate::inventory::container::SimpleContainer;

    #[test]
    fn fill_places_generated_stacks_into_empty_slots() {
        init_vanilla_registry();
        let mut container = SimpleContainer::new(3);
        container.set_item(1, ItemStack::new(&vanilla_items::DIRT));
        let mut rng = StdRng::seed_from_u64(1);
        fill_container(
            &mut container,
            vec![ItemStack::new(&vanilla_items::STONE)],
            &mut rng,
        );

        let filled = (0..3)
            .filter(|&slot| container.get_item(slot).is(&vanilla_items::STONE))
            .count();
        assert_eq!(filled, 1);
        assert!(container.get_item(1).is(&vanilla_items::DIRT));
    }

    #[test]
    fn fill_splits_oversized_stacks_into_extra_empty_slots() {
        init_vanilla_registry();
        let mut container = SimpleContainer::new(4);
        let mut rng = StdRng::seed_from_u64(7);
        fill_container(
            &mut container,
            vec![ItemStack::with_count(&vanilla_items::STONE, 8)],
            &mut rng,
        );

        let occupied = (0..4)
            .filter(|&slot| !container.get_item(slot).is_empty())
            .count();
        let total: i32 = (0..4).map(|slot| container.get_item(slot).count()).sum();
        assert!(occupied > 1, "vanilla split uses extra empty slots");
        assert_eq!(total, 8);
    }
}
