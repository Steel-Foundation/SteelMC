use steel_registry::item_stack::ItemStack;
use steel_utils::random::{Random, RandomSource};

use crate::inventory::container::Container;

/// Successfully generated loot, ready for storage-local container filling.
///
/// Generate outside container locks. The result borrows the same RNG stream for `fill`.
/// Owners must use the existing container notification path after releasing locks.
pub struct GeneratedLoot<'a> {
    items: Vec<ItemStack>,
    random: &'a mut RandomSource,
}

impl<'a> GeneratedLoot<'a> {
    pub(super) const fn new(items: Vec<ItemStack>, random: &'a mut RandomSource) -> Self {
        Self { items, random }
    }

    /// Fills empty slots using vanilla's slot shuffle and stack splitting order.
    /// Occupied slots are preserved; excess stacks are discarded with a warning.
    pub fn fill(mut self, container: &mut dyn Container) {
        let mut slots: Vec<_> = (0..container.get_container_size())
            .filter(|slot| container.get_item(*slot).is_empty())
            .collect();
        Self::shuffle(&mut slots, self.random);
        self.shuffle_and_split(slots.len());
        for item in self.items {
            let Some(slot) = slots.pop() else {
                log::warn!("Tried to over-fill a container");
                return;
            };
            container.set_item(slot, item);
        }
    }

    fn shuffle_and_split(&mut self, available: usize) {
        let mut splittable = Vec::new();
        let mut single = Vec::new();
        for item in self.items.drain(..) {
            if item.is_empty() {
                continue;
            }
            if item.count() > 1 {
                splittable.push(item);
            } else {
                single.push(item);
            }
        }
        while available > single.len() + splittable.len() && !splittable.is_empty() {
            let index = if splittable.len() <= 1 {
                0
            } else {
                self.random.next_i32_bounded(splittable.len() as i32) as usize
            };
            let mut item = splittable.remove(index);
            let half = item.count() / 2;
            let count = if half <= 1 {
                1
            } else {
                self.random.next_i32_between(1, half)
            };
            let split = item.split(count);
            if item.count() > 1 && self.random.next_bool() {
                splittable.push(item);
            } else {
                single.push(item);
            }
            if split.count() > 1 && self.random.next_bool() {
                splittable.push(split);
            } else {
                single.push(split);
            }
        }
        single.extend(splittable);
        Self::shuffle(&mut single, self.random);
        self.items = single;
    }

    fn shuffle<T>(items: &mut [T], random: &mut RandomSource) {
        for end in (2..=items.len()).rev() {
            let index = random.next_i32_bounded(end as i32) as usize;
            items.swap(end - 1, index);
        }
    }
}
