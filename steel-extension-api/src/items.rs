//! Item stacks as plain values, and player inventory access.
//!
//! [`ItemSpec`] describes a stack: item key, count, display name, lore, enchantment glint,
//! unbreakable flag, vanilla enchantments and private extension data. Extension data is a list of
//! string entries stored inside the item's `minecraft:custom_data` component under
//! `steel_extensions`, so it survives restarts, trades and chest storage like any item NBT. Use a
//! key namespaced by your extension, such as `prison-enchants:jackhammer`.
//!
//! Reading a stack returns only the fields above; writing a spec replaces the whole stack.
//! Use [`Server::set_item_data`](crate::Server::set_item_data) to change extension data without
//! touching other components.
//!
//! Inventory slots use vanilla `Inventory` indices: 0–8 hotbar, 9–35 main, 36–39 armor
//! (feet to head), 40 off hand.

use stabby::{string::String as ApiString, vec::Vec as ApiVec};

/// One vanilla enchantment on an item.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantmentLevel {
    /// Enchantment key, such as `minecraft:efficiency`.
    pub key: ApiString,
    /// Level (1 or higher).
    pub level: u32,
}

/// One private extension data entry.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataEntry {
    /// Namespaced key, such as `myextension:owner`.
    pub key: ApiString,
    /// Value.
    pub value: ApiString,
}

/// Enchantment glint override.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Glint {
    /// Vanilla behavior (glint when enchanted).
    #[default]
    Default,
    /// Always glint.
    On,
    /// Never glint.
    Off,
}

/// A description of an item stack.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpec {
    /// Item key; `minecraft:air` for an empty stack.
    pub item: ApiString,
    /// Display name (JSON text or plain); empty for the default name.
    pub name: ApiString,
    /// Lore lines (JSON text or plain).
    pub lore: ApiVec<ApiString>,
    /// Vanilla enchantments.
    pub enchantments: ApiVec<EnchantmentLevel>,
    /// Private extension data.
    pub data: ApiVec<DataEntry>,
    /// Stack size.
    pub count: i32,
    /// Glint override.
    pub glint: Glint,
    /// Whether the item never loses durability.
    pub unbreakable: bool,
}

impl ItemSpec {
    /// A single item of `item`, such as `minecraft:diamond_pickaxe`.
    #[must_use]
    pub fn new(item: &str) -> Self {
        Self {
            item: item.into(),
            name: ApiString::default(),
            lore: ApiVec::new(),
            enchantments: ApiVec::new(),
            data: ApiVec::new(),
            count: 1,
            glint: Glint::Default,
            unbreakable: false,
        }
    }

    /// An empty stack.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            count: 0,
            ..Self::new("minecraft:air")
        }
    }

    /// Whether the stack is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item.as_str() == "minecraft:air"
    }

    /// Sets the stack size.
    #[must_use]
    pub const fn count(mut self, count: i32) -> Self {
        self.count = count;
        self
    }

    /// Sets the display name (JSON text or plain).
    #[must_use]
    pub fn name(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the display name from `&` color codes, without the vanilla italic style.
    #[must_use]
    pub fn name_legacy(self, name: &str) -> Self {
        let text = crate::text::legacy(name).italic(false).to_json();
        self.name(&text)
    }

    /// Replaces the lore (JSON text or plain lines).
    #[must_use]
    pub fn lore<'a>(mut self, lines: impl IntoIterator<Item = &'a str>) -> Self {
        self.lore = lines.into_iter().map(ApiString::from).collect();
        self
    }

    /// Replaces the lore from `&` color-coded lines, without the vanilla italic style.
    #[must_use]
    pub fn lore_legacy<'a>(mut self, lines: impl IntoIterator<Item = &'a str>) -> Self {
        self.lore = lines
            .into_iter()
            .map(|line| ApiString::from(crate::text::legacy(line).italic(false).to_json()))
            .collect();
        self
    }

    /// Sets the glint override.
    #[must_use]
    pub const fn glint(mut self, glint: Glint) -> Self {
        self.glint = glint;
        self
    }

    /// Sets the unbreakable flag.
    #[must_use]
    pub const fn unbreakable(mut self, unbreakable: bool) -> Self {
        self.unbreakable = unbreakable;
        self
    }

    /// Adds or replaces a vanilla enchantment.
    #[must_use]
    pub fn enchant(mut self, key: &str, level: u32) -> Self {
        let mut list: Vec<EnchantmentLevel> = self.enchantments.iter().cloned().collect();
        list.retain(|entry| entry.key.as_str() != key);
        list.push(EnchantmentLevel {
            key: key.into(),
            level,
        });
        self.enchantments = list.into_iter().collect();
        self
    }

    /// Adds or replaces a extension data entry.
    #[must_use]
    pub fn with_data(mut self, key: &str, value: &str) -> Self {
        let mut list: Vec<DataEntry> = self.data.iter().cloned().collect();
        list.retain(|entry| entry.key.as_str() != key);
        list.push(DataEntry {
            key: key.into(),
            value: value.into(),
        });
        self.data = list.into_iter().collect();
        self
    }

    /// A extension data value.
    #[must_use]
    pub fn data(&self, key: &str) -> Option<&str> {
        self.data
            .iter()
            .find(|entry| entry.key.as_str() == key)
            .map(|entry| entry.value.as_str())
    }
}

#[stabby::host_api]
/// Inventory bindings on `Server`.
pub mod api {
    use super::ItemSpec;
    use crate::{ApiResult, PlayerId, Server};
    use stabby::str::Str;
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;

    #[cfg(feature = "host")]
    use crate::host::items as host;

    impl<'a> Server<'a> {
        /// The stack in an inventory slot.
        pub fn slot(&self, player: PlayerId, slot: i32) -> ApiResult<ItemSpec> {
            host::slot(self.host(), player, slot)
                .map_err(ApiString::from)
                .into()
        }

        /// Replaces the stack in an inventory slot; an empty spec clears it.
        pub fn set_slot(&self, player: PlayerId, slot: i32, item: ItemSpec) -> ApiResult<()> {
            host::set_slot(self.host(), player, slot, &item)
                .map_err(ApiString::from)
                .into()
        }

        /// Selected hotbar slot (0–8).
        pub fn held_slot(&self, player: PlayerId) -> ApiResult<i32> {
            host::held_slot(self.host(), player)
                .map_err(ApiString::from)
                .into()
        }

        /// Selects a hotbar slot (0–8).
        pub fn set_held_slot(&self, player: PlayerId, slot: i32) -> ApiResult<()> {
            host::set_held_slot(self.host(), player, slot)
                .map_err(ApiString::from)
                .into()
        }

        /// Adds a stack to the inventory, dropping what does not fit; returns the dropped count.
        pub fn give(&self, player: PlayerId, item: ItemSpec) -> ApiResult<i32> {
            host::give(self.host(), player, &item)
                .map_err(ApiString::from)
                .into()
        }

        /// Sets (or removes, with an empty value) one extension data entry on a slot's stack,
        /// keeping every other component.
        pub fn set_item_data(
            &self,
            player: PlayerId,
            slot: i32,
            key: Str<'_>,
            value: Str<'_>,
        ) -> ApiResult<()> {
            host::set_item_data(self.host(), player, slot, key.as_str(), value.as_str())
                .map_err(ApiString::from)
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_replaces_entries_by_key() {
        let spec = ItemSpec::new("minecraft:diamond_pickaxe")
            .with_data("p:level", "1")
            .with_data("p:level", "2")
            .enchant("minecraft:efficiency", 3)
            .enchant("minecraft:efficiency", 5);
        assert_eq!(spec.data("p:level"), Some("2"));
        assert_eq!(spec.data.len(), 1);
        assert_eq!(spec.enchantments.len(), 1);
        assert_eq!(spec.enchantments[0].level, 5);
        assert!(!spec.is_empty());
        assert!(ItemSpec::empty().is_empty());
    }
}
