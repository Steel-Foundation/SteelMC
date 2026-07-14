use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use steel_utils::{DowncastType, Identifier};
use text_components::TextComponent;

pub mod item;

use crate::{
    REGISTRY, RegistryExt, TaggedRegistryExt,
    blocks::BlockRef,
    data_components::{
        DataComponentMap,
        vanilla_components::{ITEM_MODEL, ITEM_NAME},
    },
    item_stack::ItemStack,
};

/// A Minecraft item type.
pub struct Item {
    pub key: Identifier,
    pub components: DataComponentMap,
    /// The item key returned when this item is used in crafting (e.g., "bucket" from `milk_bucket`).
    /// Stored as an Identifier to avoid circular reference issues during initialization.
    pub craft_remainder: Option<Identifier>,
    /// Cached registry ID, set during registration for O(1) lookup on hot paths.
    pub id: OnceLock<usize>,
}

impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item").field("key", &self.key).finish()
    }
}

impl Item {
    /// Creates an item with Vanilla's mandatory name and default model components.
    #[must_use]
    pub fn new(
        key: Identifier,
        item_name: TextComponent,
        craft_remainder: Option<Identifier>,
    ) -> Self {
        let mut components = DataComponentMap::common_item_components();
        components.set(ITEM_NAME, Some(item_name));
        components.set(ITEM_MODEL, Some(key.clone()));
        Self {
            key,
            components,
            craft_remainder,
            id: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn from_block(block: BlockRef, item_name: TextComponent) -> Self {
        Self::new(block.key.clone(), item_name, None)
    }

    #[must_use]
    pub fn from_block_custom_name(
        _block: BlockRef,
        name: &'static str,
        item_name: TextComponent,
    ) -> Self {
        Self::new(Identifier::vanilla_static(name), item_name, None)
    }

    /// Builder method to set a component on this item. Used during static initialization.
    #[must_use]
    pub fn builder_set<T: crate::data_components::Component + DowncastType>(
        mut self,
        component: crate::data_components::DataComponentType<T>,
        value: Option<T>,
    ) -> Self {
        self.components.set(component, value);
        self
    }

    /// Returns the item stack that remains after this item is used in crafting.
    /// For example, `milk_bucket` returns an empty bucket.
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

    /// Returns `true` if this item is tagged with the given tag.
    pub fn has_tag(&'static self, tag: &Identifier) -> bool {
        REGISTRY.items.is_in_tag(self, tag)
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
        let cached = item.id.get_or_init(|| id);
        assert_eq!(*cached, id, "item registered with conflicting id");
        self.items_by_key.insert(item.key.clone(), id);
        self.items_by_id.push(item);

        id
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ItemRef)> + '_ {
        self.items_by_id
            .iter()
            .enumerate()
            .map(|(id, &item)| (id, item))
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;
    use text_components::TextComponent;

    use super::Item;
    use crate::data_components::vanilla_components::{ITEM_MODEL, ITEM_NAME};

    #[test]
    fn new_item_uses_its_key_as_the_default_model() {
        let key = Identifier::new_static("steel", "test_item");
        let name = TextComponent::plain("Test Item");
        let item = Item::new(key.clone(), name.clone(), None);

        assert_eq!(item.components.get_ref(ITEM_MODEL), Some(&key));
        assert_eq!(item.components.get_ref(ITEM_NAME), Some(&name));
    }
}

crate::impl_registry_ext!(ItemRegistry, Item, items_by_id, items_by_key);
crate::impl_tagged_registry!(ItemRegistry, items_by_key, "item");

crate::impl_registry_entry_eq!(Item);

impl crate::RegistryEntry for Item {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}
