use rustc_hash::FxHashMap;

use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;

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

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
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

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<ItemRef> {
        self.items_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, item: ItemRef) -> &usize {
        self.items_by_key.get(&item.key).expect("Item not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<ItemRef> {
        self.items_by_key.get(key).and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ItemRef)> + '_ {
        self.items_by_id
            .iter()
            .enumerate()
            .map(|(id, &item)| (id, item))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items_by_id.is_empty()
    }

    // Tag-related methods

    /// Registers a tag with a list of item keys.
    /// Item keys that don't exist in the registry are silently skipped.
    pub fn register_tag(&mut self, tag: Identifier, item_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifier: Vec<Identifier> = item_keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                // Only include if the item actually exists
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifier);
    }

    /// Gives the access to all blocks to delete and add new entries
    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let new_items = f(existing)
            .into_iter()
            .filter(|item| {
                let exists = self.items_by_key.contains_key(item);
                if !exists {
                    tracing::error!("item {item} not found in registry, skipping from tag {tag}");
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), new_items);
    }

    /// Checks if an item is in a given tag.
    #[must_use]
    pub fn is_in_tag(&self, item: ItemRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|items| items.contains(&item.key))
    }

    /// Gets all items in a tag.
    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<ItemRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    /// Iterates over all items in a tag.
    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = ItemRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    /// Gets all tag keys.
    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> + '_ {
        self.tags.keys()
    }
}

impl RegistryExt for ItemRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}
