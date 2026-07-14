//! Vanilla `minecraft:block_state` item component.

use std::collections::BTreeMap;
use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{PrefixedRead, PrefixedWrite, ReadFrom, WriteTo};

/// String-valued block properties applied when a block item is placed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BlockItemStateProperties {
    properties: BTreeMap<String, String>,
}

impl BlockItemStateProperties {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            properties: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn new(properties: BTreeMap<String, String>) -> Self {
        Self { properties }
    }

    #[must_use]
    pub const fn properties(&self) -> &BTreeMap<String, String> {
        &self.properties
    }

    #[must_use]
    pub fn get(&self, property: &str) -> Option<&str> {
        self.properties.get(property).map(String::as_str)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }
}

impl WriteTo for BlockItemStateProperties {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_count(self.properties.len(), writer)?;
        for (name, value) in &self.properties {
            name.write_prefixed_bound::<VarInt>(writer, i16::MAX as usize)?;
            value.write_prefixed_bound::<VarInt>(writer, i16::MAX as usize)?;
        }
        Ok(())
    }
}

impl ReadFrom for BlockItemStateProperties {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_count(data)?;
        let mut properties = BTreeMap::new();
        for _ in 0..count {
            let name = String::read_prefixed_bound::<VarInt>(data, i16::MAX as usize)?;
            let value = String::read_prefixed_bound::<VarInt>(data, i16::MAX as usize)?;
            properties.insert(name, value);
        }
        Ok(Self::new(properties))
    }
}

impl ToNbtTag for BlockItemStateProperties {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        for (name, value) in self.properties {
            compound.insert(name, value);
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for BlockItemStateProperties {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let properties = compound
            .iter()
            .map(|(name, value)| Some((name.to_string(), value.string()?.to_string())))
            .collect::<Option<BTreeMap<_, _>>>()?;
        Some(Self::new(properties))
    }
}

impl HashComponent for BlockItemStateProperties {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(self.properties.len());
        for (name, value) in &self.properties {
            let mut key_hasher = ComponentHasher::new();
            key_hasher.put_string(name);
            let mut value_hasher = ComponentHasher::new();
            value_hasher.put_string(value);
            entries.push(HashEntry::new(key_hasher, value_hasher));
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

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(count)
        .map_err(|_| Error::other("Block-state property map exceeds protocol range"))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count).map_err(|_| Error::other(format!("Negative map size: {count}")))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::BlockItemStateProperties;
    use crate::data_components::vanilla_components::BLOCK_STATE;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse(tag: NbtTag) -> Option<BlockItemStateProperties> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        BlockItemStateProperties::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn property_maps_round_trip_both_codecs_and_hash_as_maps() {
        let value = BlockItemStateProperties::new(BTreeMap::from([
            ("facing".to_owned(), "north".to_owned()),
            ("level".to_owned(), "15".to_owned()),
        ]));
        let mut compound = NbtCompound::new();
        compound.insert("facing", "north");
        compound.insert("level", "15");
        let nbt = NbtTag::Compound(compound);
        assert_eq!(value.clone().to_nbt_tag(), nbt);
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        assert_eq!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value.write(&mut network).expect("properties should encode");
        assert_eq!(
            BlockItemStateProperties::read(&mut Cursor::new(network.as_slice()))
                .expect("properties should decode"),
            value
        );
    }

    #[test]
    fn empty_properties_use_an_empty_compound() {
        let empty = BlockItemStateProperties::empty();
        assert!(empty.is_empty());
        assert_eq!(
            empty.clone().to_nbt_tag(),
            NbtTag::Compound(NbtCompound::new())
        );
    }

    #[test]
    fn extracted_block_items_keep_placement_properties() {
        init_test_registry();
        let light = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("light"))
            .expect("light should be registered");
        assert_eq!(
            light.components.get(BLOCK_STATE),
            Some(BlockItemStateProperties::new(BTreeMap::from([(
                "level".to_owned(),
                "15".to_owned(),
            )])))
        );
    }
}
