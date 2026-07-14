//! Vanilla `minecraft:container_loot` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag, read_tag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::vanilla_nbt_heap_size;
use steel_utils::serial::{ReadFrom, WriteTo};

const DEFAULT_NBT_QUOTA: u64 = 2_097_152;

/// Loot-table resource key and optional deterministic seed for a container item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededContainerLoot {
    loot_table: Identifier,
    seed: i64,
}

impl SeededContainerLoot {
    #[must_use]
    pub const fn new(loot_table: Identifier, seed: i64) -> Self {
        Self { loot_table, seed }
    }

    #[must_use]
    pub const fn loot_table(&self) -> &Identifier {
        &self.loot_table
    }

    #[must_use]
    pub const fn seed(&self) -> i64 {
        self.seed
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("loot_table", self.loot_table.to_string());
        if self.seed != 0 {
            compound.insert("seed", self.seed);
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let loot_table =
            Identifier::from_str(&compound.get("loot_table")?.string()?.to_string()).ok()?;
        let seed = match compound.get("seed") {
            Some(tag) => codec_i64(tag)?,
            None => 0,
        };
        Some(Self::new(loot_table, seed))
    }
}

impl WriteTo for SeededContainerLoot {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut encoded = Vec::new();
        self.to_nbt_tag_ref().write(&mut encoded);
        writer.write_all(&encoded)
    }
}

impl ReadFrom for SeededContainerLoot {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let tag =
            read_tag(data).map_err(|error| Error::other(format!("Invalid NBT: {error:?}")))?;
        let Some(heap_size) = vanilla_nbt_heap_size(&tag) else {
            return Err(Error::other("NBT contains malformed modified UTF-8"));
        };
        if heap_size > DEFAULT_NBT_QUOTA {
            return Err(Error::other(format!(
                "NBT exceeds Vanilla's {DEFAULT_NBT_QUOTA}-byte heap quota"
            )));
        }
        Self::from_owned_nbt(&tag)
            .ok_or_else(|| Error::other("Container loot network value is malformed"))
    }
}

impl ToNbtTag for SeededContainerLoot {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for SeededContainerLoot {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for SeededContainerLoot {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "loot_table", &self.loot_table);
        if self.seed != 0 {
            push_hash_entry(&mut entries, "seed", &self.seed);
        }
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn codec_i64(tag: &NbtTag) -> Option<i64> {
    match tag {
        NbtTag::Byte(value) => Some(i64::from(*value)),
        NbtTag::Short(value) => Some(i64::from(*value)),
        NbtTag::Int(value) => Some(i64::from(*value)),
        NbtTag::Long(value) => Some(*value),
        NbtTag::Float(value) => Some(*value as i64),
        NbtTag::Double(value) => Some(*value as i64),
        _ => None,
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::SeededContainerLoot;

    fn parse(tag: NbtTag) -> Option<SeededContainerLoot> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        SeededContainerLoot::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn loot_table_resource_keys_and_optional_seed_round_trip() {
        let loot =
            SeededContainerLoot::new(Identifier::vanilla_static("chests/simple_dungeon"), 42);
        let mut compound = NbtCompound::new();
        compound.insert("loot_table", "minecraft:chests/simple_dungeon");
        compound.insert("seed", 42_i64);
        let nbt = NbtTag::Compound(compound);
        assert_eq!(loot.clone().to_nbt_tag(), nbt);
        assert_eq!(parse(nbt.clone()), Some(loot.clone()));
        assert_eq!(loot.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        loot.write(&mut network)
            .expect("container loot should encode");
        assert_eq!(
            SeededContainerLoot::read(&mut Cursor::new(network.as_slice()))
                .expect("container loot should decode"),
            loot
        );
    }

    #[test]
    fn omitted_seed_defaults_to_zero_without_registry_membership_validation() {
        let mut compound = NbtCompound::new();
        compound.insert("loot_table", "steel:unknown_table");
        assert_eq!(
            parse(NbtTag::Compound(compound)),
            Some(SeededContainerLoot::new(
                Identifier::new_static("steel", "unknown_table"),
                0,
            ))
        );
    }
}
