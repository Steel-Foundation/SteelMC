use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};

/// Enchantments stored on an item. Maps enchantment key to level.
///
/// Used by both the `minecraft:enchantments` component (on enchanted items)
/// and the `minecraft:stored_enchantments` component (on enchanted books).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemEnchantments {
    pub levels: FxHashMap<Identifier, u32>,
    pub show_in_tooltip: bool,
}

impl ItemEnchantments {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            levels: FxHashMap::default(),
            show_in_tooltip: true,
        }
    }

    #[must_use]
    pub fn get_level(&self, enchantment: &Identifier) -> u32 {
        self.levels.get(enchantment).copied().unwrap_or(0)
    }

    pub fn set(&mut self, enchantment: Identifier, level: u32) {
        if level == 0 {
            self.levels.remove(&enchantment);
        } else {
            self.levels.insert(enchantment, level);
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Identifier, &u32)> {
        self.levels.iter()
    }
}

impl Default for ItemEnchantments {
    fn default() -> Self {
        Self::empty()
    }
}

impl HashComponent for ItemEnchantments {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_map();
        let mut entries: Vec<_> = self
            .levels
            .iter()
            .map(|(key, &level)| {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string(&key.to_string());
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_int(level as i32);
                HashEntry::new(key_hasher, value_hasher)
            })
            .collect();
        sort_map_entries(&mut entries);
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}
