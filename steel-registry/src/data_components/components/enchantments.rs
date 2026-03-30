use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::{REGISTRY, RegistryExt};

/// Enchantments stored on an item. Maps enchantment key to level.
///
/// Used by both the `minecraft:enchantments` component (on enchanted items)
/// and the `minecraft:stored_enchantments` component (on enchanted books).
///
/// Vanilla moved tooltip visibility to the separate `TOOLTIP_DISPLAY` component.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemEnchantments {
    pub levels: FxHashMap<Identifier, u32>,
}

impl ItemEnchantments {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            levels: FxHashMap::default(),
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

    /// Vanilla `Mutable.upgrade`: keeps the higher of existing vs new level.
    pub fn upgrade(&mut self, enchantment: Identifier, level: u32) {
        if level > 0 {
            let existing = self.get_level(&enchantment);
            self.levels
                .insert(enchantment, existing.max(level).min(255));
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

/// Network format: VarInt count, then (VarInt enchantment_id, VarInt level) pairs.
impl WriteTo for ItemEnchantments {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        VarInt(self.levels.len() as i32).write(writer)?;
        for (key, &level) in &self.levels {
            let id = REGISTRY
                .enchantments
                .id_from_key(key)
                .ok_or_else(|| std::io::Error::other(format!("Unknown enchantment: {key}")))?;
            VarInt(id as i32).write(writer)?;
            VarInt(level as i32).write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for ItemEnchantments {
    fn read(data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
        let count = VarInt::read(data)?.0;
        if !(0..=256).contains(&count) {
            return Err(std::io::Error::other(format!(
                "Enchantment count out of range: {count}"
            )));
        }
        let count = count as usize;
        let mut levels = FxHashMap::default();
        for _ in 0..count {
            let id = VarInt::read(data)?.0 as usize;
            let level = VarInt::read(data)?.0 as u32;
            let enchantment = REGISTRY
                .enchantments
                .by_id(id)
                .ok_or_else(|| std::io::Error::other(format!("Unknown enchantment id: {id}")))?;
            levels.insert(enchantment.key.clone(), level);
        }
        Ok(Self { levels })
    }
}

/// NBT format: compound with enchantment identifiers as keys and int levels as values.
impl ToNbtTag for ItemEnchantments {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        for (key, level) in &self.levels {
            compound.insert(key.to_string(), NbtTag::Int(*level as i32));
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for ItemEnchantments {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let mut levels = FxHashMap::default();
        for (key, value) in compound.iter() {
            let key_str = key.to_str();
            if let Ok(ident) = key_str.parse::<Identifier>()
                && let Some(level) = value.int()
                && level > 0
            {
                levels.insert(ident, level as u32);
            }
        }
        Some(Self { levels })
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
