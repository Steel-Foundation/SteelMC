//! Vanilla `minecraft:bees` item component.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use super::EntityData;

/// One entity stored inside a beehive block item.
#[derive(Debug, Clone, PartialEq)]
pub struct BeehiveOccupant {
    entity_data: EntityData,
    ticks_in_hive: i32,
    min_ticks_in_hive: i32,
}

impl BeehiveOccupant {
    #[must_use]
    pub const fn new(entity_data: EntityData, ticks_in_hive: i32, min_ticks_in_hive: i32) -> Self {
        Self {
            entity_data,
            ticks_in_hive,
            min_ticks_in_hive,
        }
    }

    #[must_use]
    pub const fn entity_data(&self) -> &EntityData {
        &self.entity_data
    }

    #[must_use]
    pub const fn ticks_in_hive(&self) -> i32 {
        self.ticks_in_hive
    }

    #[must_use]
    pub const fn min_ticks_in_hive(&self) -> i32 {
        self.min_ticks_in_hive
    }

    fn to_nbt_compound(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("entity_data", self.entity_data.clone().to_nbt_tag());
        compound.insert("ticks_in_hive", self.ticks_in_hive);
        compound.insert("min_ticks_in_hive", self.min_ticks_in_hive);
        compound
    }

    fn from_nbt_compound(compound: &NbtCompound) -> Option<Self> {
        let entity_data = EntityData::from_owned_nbt(compound.get("entity_data")?)?;
        let ticks_in_hive = compound.get("ticks_in_hive")?.codec_i32()?;
        let min_ticks_in_hive = compound.get("min_ticks_in_hive")?.codec_i32()?;
        Some(Self::new(entity_data, ticks_in_hive, min_ticks_in_hive))
    }
}

impl WriteTo for BeehiveOccupant {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.entity_data.write(writer)?;
        VarInt(self.ticks_in_hive).write(writer)?;
        VarInt(self.min_ticks_in_hive).write(writer)
    }
}

impl ReadFrom for BeehiveOccupant {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            EntityData::read(data)?,
            VarInt::read(data)?.0,
            VarInt::read(data)?.0,
        ))
    }
}

impl HashComponent for BeehiveOccupant {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(3);
        push_hash_entry(&mut entries, "entity_data", &self.entity_data);
        push_hash_entry(&mut entries, "ticks_in_hive", &self.ticks_in_hive);
        push_hash_entry(&mut entries, "min_ticks_in_hive", &self.min_ticks_in_hive);
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

/// Ordered occupants stored inside a beehive block item.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Bees {
    bees: Vec<BeehiveOccupant>,
}

impl Bees {
    #[must_use]
    pub const fn empty() -> Self {
        Self { bees: Vec::new() }
    }

    #[must_use]
    pub const fn new(bees: Vec<BeehiveOccupant>) -> Self {
        Self { bees }
    }

    #[must_use]
    pub fn bees(&self) -> &[BeehiveOccupant] {
        &self.bees
    }
}

impl WriteTo for Bees {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_count(self.bees.len(), writer)?;
        for bee in &self.bees {
            bee.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for Bees {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_count(data)?;
        let mut bees = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            bees.push(BeehiveOccupant::read(data)?);
        }
        Ok(Self::new(bees))
    }
}

impl ToNbtTag for Bees {
    fn to_nbt_tag(self) -> NbtTag {
        if self.bees.is_empty() {
            NbtTag::List(NbtList::Empty)
        } else {
            NbtTag::List(NbtList::Compound(
                self.bees
                    .iter()
                    .map(BeehiveOccupant::to_nbt_compound)
                    .collect(),
            ))
        }
    }
}

impl FromNbtTag for Bees {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let values = tag.list()?.to_owned().as_nbt_tags();
        let bees = values
            .iter()
            .map(|tag| BeehiveOccupant::from_nbt_compound(tag.compound()?))
            .collect::<Option<Vec<_>>>()?;
        Some(Self::new(bees))
    }
}

impl HashComponent for Bees {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for bee in &self.bees {
            hasher.put_component_hash(bee);
        }
        hasher.end_list();
    }
}

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(count)
        .map_err(|_| Error::other("Bee occupant list exceeds protocol range"))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count).map_err(|_| Error::other(format!("Negative bee count: {count}")))
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
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{BeehiveOccupant, Bees};
    use crate::data_components::components::{CustomData, EntityData};
    use crate::data_components::vanilla_components::BEES;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse(tag: NbtTag) -> Option<Bees> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        Bees::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn bee_occupants_round_trip_both_codecs_and_hash_as_a_list() {
        init_test_registry();
        let bee = REGISTRY
            .entity_types
            .by_key(&steel_utils::Identifier::vanilla_static("bee"))
            .expect("bee should be registered");
        let mut payload = NbtCompound::new();
        payload.insert("HasNectar", true);
        let value = Bees::new(vec![BeehiveOccupant::new(
            EntityData::new(
                bee,
                CustomData::try_from_compound(payload).expect("valid bee data"),
            ),
            17,
            600,
        )]);
        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        assert_eq!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value.write(&mut network).expect("bees should encode");
        assert_eq!(
            Bees::read(&mut Cursor::new(network.as_slice())).expect("bees should decode"),
            value
        );
    }

    #[test]
    fn extracted_beehives_start_with_no_occupants() {
        init_test_registry();
        for key in ["bee_nest", "beehive"] {
            let item = REGISTRY
                .items
                .by_key(&steel_utils::Identifier::vanilla(key.to_owned()))
                .unwrap_or_else(|| panic!("{key} should be registered"));
            assert_eq!(item.components.get(BEES), Some(Bees::empty()));
        }
    }
}
