use rustc_hash::FxHashMap;

use steel_utils::Identifier;

pub mod item;

use crate::{
    REGISTRY, RegistryExt, blocks::BlockRef, data_components::DataComponentMap,
    item_stack::ItemStack,
};

/// A Minecraft item type.
pub struct Item {
    pub key: Identifier,
    pub components: DataComponentMap,
    /// The item key returned when this item is used in crafting (e.g., "bucket" from milk_bucket).
    /// Stored as an Identifier to avoid circular reference issues during initialization.
    pub craft_remainder: Option<Identifier>,
}

impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item").field("key", &self.key).finish()
    }
}

impl Item {
    #[must_use]
    pub fn from_block(block: BlockRef) -> Self {
        Self {
            key: block.key.clone(),
            components: DataComponentMap::common_item_components(),
            craft_remainder: None,
        }
    }

    #[must_use]
    pub fn from_block_custom_name(_block: BlockRef, name: &'static str) -> Self {
        Self {
            key: Identifier::vanilla_static(name),
            components: DataComponentMap::common_item_components(),
            craft_remainder: None,
        }
    }

    /// Builder method to set a component on this item. Used during static initialization.
    #[must_use]
    pub fn builder_set<T: crate::data_components::Component>(
        mut self,
        component: crate::data_components::DataComponentType<T>,
        value: Option<T>,
    ) -> Self {
        self.components.set(component, value);
        self
    }

    /// Returns the item stack that remains after this item is used in crafting.
    /// For example, milk_bucket returns an empty bucket.
    #[must_use]
    pub fn get_crafting_remainder(&self) -> ItemStack {
        match &self.craft_remainder {
            Some(remainder_key) => {
                if let Some(remainder_item) = REGISTRY.items.by_key(remainder_key) {
                    ItemStack::new(remainder_item)
                } else {
                    ItemStack::empty()
                }
            }
            None => ItemStack::empty(),
        }
    }
}

pub type ItemRef = &'static Item;

impl PartialEq for ItemRef {
    #[allow(clippy::disallowed_methods)] // This IS the PartialEq impl; ptr::eq is correct here
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(*self, *other)
    }
}

impl Eq for ItemRef {}

pub struct ItemRegistry {
    items_by_id: Vec<ItemRef>,
    items_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl Default for ItemRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            items_by_id: Vec::new(),
            items_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, item: ItemRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register items after the registry has been frozen"
        );

        let id = self.items_by_id.len();
        self.items_by_key.insert(item.key.clone(), id);
        self.items_by_id.push(item);

        id
    }

    /// Replaces a item at a given index.
    /// Returns true if the item was replaced and false if the item wasn't replaced
    #[must_use]
    pub fn replace(&mut self, item: ItemRef, id: usize) -> bool {
        if id >= self.items_by_id.len() {
            return false;
        }
        self.items_by_id[id] = item;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ItemRef)> + '_ {
        self.items_by_id
            .iter()
            .enumerate()
            .map(|(id, &item)| (id, item))
    }
}

crate::impl_registry!(ItemRegistry, Item, items_by_id, items_by_key, items);
crate::impl_tagged_registry!(ItemRegistry, items_by_key, "item");
