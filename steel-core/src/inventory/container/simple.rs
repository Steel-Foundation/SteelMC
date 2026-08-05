use steel_registry::item_stack::ItemStack;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::container::Container;

/// A Simple Container
pub struct SimpleContainer {
    items: Vec<ItemStack>,
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
        }
    }

    /// Creates a Simple Container with already initialized items
    #[must_use]
    pub const fn from_items(items: Vec<ItemStack>) -> Self {
        Self { items }
    }
}

impl Container for SimpleContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_changed(&mut self) {}
}
