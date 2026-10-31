use steel_registry::item_stack::ItemStack;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::container::Container;

/// A Simple Container
pub struct SimpleContainer {
    items: Vec<ItemStack>,
    times_changed: u32,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SimpleContainer`.
unsafe impl DowncastType for SimpleContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/simple");
}

impl SimpleContainer {
    /// Creates a new Simple Container
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            items: vec![ItemStack::empty(); size],
            times_changed: 0,
        }
    }

    /// Creates a Simple Container with already initialized items
    #[must_use]
    pub const fn from_items(items: Vec<ItemStack>) -> Self {
        Self {
            items,
            times_changed: 0,
        }
    }

    /// Number of `set_changed` calls so far; compare snapshots to detect a change.
    #[must_use]
    pub const fn times_changed(&self) -> u32 {
        self.times_changed
    }
}

impl Container for SimpleContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_changed(&mut self) {
        self.times_changed = self.times_changed.wrapping_add(1);
    }
}
