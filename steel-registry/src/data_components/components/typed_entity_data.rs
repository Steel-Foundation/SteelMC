//! Typed entity and block-entity item component data.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use super::CustomData;
use crate::block_entity_type::BlockEntityTypeRef;
use crate::entity_type::EntityTypeRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Entity type plus custom entity data without its redundant `id` field.
#[derive(Debug, Clone)]
pub struct EntityData {
    entity_type: EntityTypeRef,
    data: CustomData,
}

impl EntityData {
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, data: CustomData) -> Self {
        Self {
            entity_type,
            data: data.without_field("id"),
        }
    }

    #[must_use]
    pub const fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    #[must_use]
    pub const fn data(&self) -> &CustomData {
        &self.data
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let data = CustomData::from_nbt_value(tag)?;
        let id = typed_data_id(&data)?;
        let entity_type = REGISTRY.entity_types.by_key(&id)?;
        Some(Self::new(entity_type, data))
    }
}

impl PartialEq for EntityData {
    fn eq(&self, other: &Self) -> bool {
        self.entity_type.key == other.entity_type.key && self.data == other.data
    }
}

impl WriteTo for EntityData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_registry_id(self.entity_type, writer)?;
        self.data.write(writer)
    }
}

impl ReadFrom for EntityData {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = read_registry_id(data, "entity type")?;
        let entity_type = REGISTRY
            .entity_types
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown entity type id: {id}")))?;
        Ok(Self::new(entity_type, CustomData::read(data)?))
    }
}

impl ToNbtTag for EntityData {
    fn to_nbt_tag(self) -> NbtTag {
        typed_data_nbt(&self.entity_type.key, &self.data)
    }
}

impl FromNbtTag for EntityData {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for EntityData {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        typed_data_nbt(&self.entity_type.key, &self.data).hash_component(hasher);
    }
}

/// Block-entity type plus custom block-entity data without its redundant `id` field.
#[derive(Debug, Clone)]
pub struct BlockEntityData {
    block_entity_type: BlockEntityTypeRef,
    data: CustomData,
}

impl BlockEntityData {
    #[must_use]
    pub fn new(block_entity_type: BlockEntityTypeRef, data: CustomData) -> Self {
        Self {
            block_entity_type,
            data: data.without_field("id"),
        }
    }

    #[must_use]
    pub const fn block_entity_type(&self) -> BlockEntityTypeRef {
        self.block_entity_type
    }

    #[must_use]
    pub const fn data(&self) -> &CustomData {
        &self.data
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let data = CustomData::from_nbt_value(tag)?;
        let id = typed_data_id(&data)?;
        let block_entity_type = REGISTRY.block_entity_types.by_key(&id)?;
        Some(Self::new(block_entity_type, data))
    }
}

impl PartialEq for BlockEntityData {
    fn eq(&self, other: &Self) -> bool {
        self.block_entity_type.key == other.block_entity_type.key && self.data == other.data
    }
}

impl WriteTo for BlockEntityData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_registry_id(self.block_entity_type, writer)?;
        self.data.write(writer)
    }
}

impl ReadFrom for BlockEntityData {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = read_registry_id(data, "block entity type")?;
        let block_entity_type = REGISTRY
            .block_entity_types
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown block entity type id: {id}")))?;
        Ok(Self::new(block_entity_type, CustomData::read(data)?))
    }
}

impl ToNbtTag for BlockEntityData {
    fn to_nbt_tag(self) -> NbtTag {
        typed_data_nbt(&self.block_entity_type.key, &self.data)
    }
}

impl FromNbtTag for BlockEntityData {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for BlockEntityData {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        typed_data_nbt(&self.block_entity_type.key, &self.data).hash_component(hasher);
    }
}

fn typed_data_nbt(type_key: &Identifier, data: &CustomData) -> NbtTag {
    let mut compound = data.copy_tag();
    compound.insert("id", type_key.to_string());
    NbtTag::Compound(compound)
}

fn typed_data_id(data: &CustomData) -> Option<Identifier> {
    Identifier::from_str(&data.as_compound().get("id")?.string()?.to_string()).ok()
}

fn write_registry_id<T: RegistryEntry>(value: &T, writer: &mut impl Write) -> Result<()> {
    let id = value
        .try_id()
        .ok_or_else(|| Error::other(format!("Unknown registry value: {}", value.key())))?;
    let id = i32::try_from(id)
        .map_err(|_| Error::other(format!("Registry id out of protocol range: {id}")))?;
    VarInt(id).write(writer)
}

fn read_registry_id(data: &mut Cursor<&[u8]>, name: &str) -> Result<usize> {
    let id = VarInt::read(data)?.0;
    usize::try_from(id).map_err(|_| Error::other(format!("Negative {name} id: {id}")))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::ToNbtTag as _;
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{BlockEntityData, EntityData};
    use crate::data_components::components::CustomData;
    use crate::data_components::vanilla_components::ENTITY_DATA;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse<T: simdnbt::FromNbtTag>(tag: NbtTag) -> Option<T> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        T::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn entity_data_separates_type_from_payload_on_the_network() {
        init_test_registry();
        let pig = REGISTRY
            .entity_types
            .by_key(&steel_utils::Identifier::vanilla_static("pig"))
            .expect("pig should be registered");
        let mut payload = NbtCompound::new();
        payload.insert("id", "minecraft:cow");
        payload.insert("CustomNameVisible", true);
        let value = EntityData::new(
            pig,
            CustomData::try_from_compound(payload).expect("valid custom data"),
        );
        assert!(value.data().as_compound().get("id").is_none());

        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        assert_eq!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("entity data should encode");
        assert_eq!(
            EntityData::read(&mut Cursor::new(network.as_slice()))
                .expect("entity data should decode"),
            value
        );
    }

    #[test]
    fn block_entity_data_uses_the_block_entity_registry() {
        init_test_registry();
        let chest = REGISTRY
            .block_entity_types
            .by_key(&steel_utils::Identifier::vanilla_static("chest"))
            .expect("chest block entity should be registered");
        let value = BlockEntityData::new(chest, CustomData::default());
        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("block entity data should encode");
        assert_eq!(
            BlockEntityData::read(&mut Cursor::new(network.as_slice()))
                .expect("block entity data should decode"),
            value
        );
    }

    #[test]
    fn extracted_spawn_eggs_use_typed_empty_entity_data() {
        init_test_registry();
        let egg = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("pig_spawn_egg"))
            .expect("pig spawn egg should be registered");
        let entity_data = egg
            .components
            .get(ENTITY_DATA)
            .expect("spawn egg should carry entity data");
        assert_eq!(
            entity_data.entity_type().key,
            steel_utils::Identifier::vanilla_static("pig")
        );
        assert!(entity_data.data().is_empty());
    }
}
