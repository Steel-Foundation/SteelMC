//! Vanilla `minecraft:debug_stick_state` item component.

use std::collections::BTreeMap;
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag, read_tag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::vanilla_nbt_heap_size;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::blocks::BlockRef;
use crate::{REGISTRY, RegistryExt};

const DEFAULT_NBT_QUOTA: u64 = 2_097_152;

/// Selected property for one block in a debug stick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugStickProperty {
    block: BlockRef,
    property: &'static str,
}

impl DebugStickProperty {
    #[must_use]
    pub const fn block(&self) -> BlockRef {
        self.block
    }

    #[must_use]
    pub const fn property(&self) -> &'static str {
        self.property
    }
}

/// Per-block property selections stored by a debug stick.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DebugStickState {
    properties: BTreeMap<String, DebugStickProperty>,
}

impl DebugStickState {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            properties: BTreeMap::new(),
        }
    }

    pub fn new(entries: impl IntoIterator<Item = (BlockRef, String)>) -> Result<Self> {
        let mut properties = BTreeMap::new();
        for (block, name) in entries {
            let property = block
                .properties
                .iter()
                .find(|property| property.get_name() == name)
                .ok_or_else(|| Error::other(format!("Block {} has no property {name}", block.key)))?
                .get_name();
            properties.insert(
                block.key.to_string(),
                DebugStickProperty { block, property },
            );
        }
        Ok(Self { properties })
    }

    pub fn properties(&self) -> impl Iterator<Item = &DebugStickProperty> {
        self.properties.values()
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        for property in self.properties.values() {
            compound.insert(property.block.key.to_string(), property.property);
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let mut entries = Vec::with_capacity(compound.len());
        for (key, value) in compound.iter() {
            let key = Identifier::from_str(&key.to_string()).ok()?;
            let block = REGISTRY.blocks.by_key(&key)?;
            entries.push((block, value.string()?.to_string()));
        }
        Self::new(entries).ok()
    }
}

impl WriteTo for DebugStickState {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut encoded = Vec::new();
        self.to_nbt_tag_ref().write(&mut encoded);
        writer.write_all(&encoded)
    }
}

impl ReadFrom for DebugStickState {
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
            .ok_or_else(|| Error::other("Debug stick state network value is malformed"))
    }
}

impl ToNbtTag for DebugStickState {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for DebugStickState {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for DebugStickState {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(self.properties.len());
        for property in self.properties.values() {
            let mut key_hasher = ComponentHasher::new();
            key_hasher.put_string(&property.block.key.to_string());
            let mut value_hasher = ComponentHasher::new();
            value_hasher.put_string(property.property);
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::DebugStickState;
    use crate::data_components::vanilla_components::DEBUG_STICK_STATE;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt, vanilla_blocks};

    fn parse(tag: NbtTag) -> Option<DebugStickState> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        DebugStickState::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn block_property_map_validates_properties_and_uses_codec_derived_network() {
        init_test_registry();
        let value = DebugStickState::new([(&vanilla_blocks::REDSTONE_WIRE, "power".to_owned())])
            .expect("redstone wire has a power property");
        let mut compound = NbtCompound::new();
        compound.insert("minecraft:redstone_wire", "power");
        let nbt = NbtTag::Compound(compound);
        assert_eq!(value.clone().to_nbt_tag(), nbt);
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        assert_eq!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("debug state should encode");
        assert_eq!(
            DebugStickState::read(&mut Cursor::new(network.as_slice()))
                .expect("debug state should decode"),
            value
        );
        assert!(DebugStickState::new([(&vanilla_blocks::STONE, "missing".to_owned())]).is_err());
    }

    #[test]
    fn extracted_debug_stick_has_an_empty_state() {
        init_test_registry();
        let item = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("debug_stick"))
            .expect("debug stick should be registered");
        assert_eq!(
            item.components.get(DEBUG_STICK_STATE),
            Some(DebugStickState::empty())
        );
    }
}
