//! Vanilla `minecraft:map_decorations` item component.

use std::collections::BTreeMap;
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag, read_tag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::{NbtNumeric as _, vanilla_nbt_heap_size};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::map_decoration_type::MapDecorationType;
use crate::{REGISTRY, RegistryExt, RegistryReference};

const DEFAULT_NBT_QUOTA: u64 = 2_097_152;

/// One named decoration placed on a map.
#[derive(Debug, Clone)]
pub struct MapDecorationEntry {
    decoration_type: RegistryReference<MapDecorationType>,
    x: f64,
    z: f64,
    rotation: f32,
}

impl PartialEq for MapDecorationEntry {
    fn eq(&self, other: &Self) -> bool {
        self.decoration_type == other.decoration_type
            && java_double_equals(self.x, other.x)
            && java_double_equals(self.z, other.z)
            && java_float_equals(self.rotation, other.rotation)
    }
}

impl MapDecorationEntry {
    #[must_use]
    pub const fn new(
        decoration_type: RegistryReference<MapDecorationType>,
        x: f64,
        z: f64,
        rotation: f32,
    ) -> Self {
        Self {
            decoration_type,
            x,
            z,
            rotation,
        }
    }

    #[must_use]
    pub const fn decoration_type(&self) -> RegistryReference<MapDecorationType> {
        self.decoration_type
    }

    #[must_use]
    pub const fn x(&self) -> f64 {
        self.x
    }

    #[must_use]
    pub const fn z(&self) -> f64 {
        self.z
    }

    #[must_use]
    pub const fn rotation(&self) -> f32 {
        self.rotation
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("type", self.decoration_type.to_nbt_tag());
        compound.insert("x", self.x);
        compound.insert("z", self.z);
        compound.insert("rotation", self.rotation);
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let key = Identifier::from_str(
            &compound
                .get("type")?
                .string()?
                .to_owned()
                .try_into_string()
                .ok()?,
        )
        .ok()?;
        let decoration_type = REGISTRY.map_decoration_types.by_key(&key)?;
        Some(Self::new(
            RegistryReference::new(decoration_type),
            compound.get("x")?.codec_f64()?,
            compound.get("z")?.codec_f64()?,
            compound.get("rotation")?.codec_f32()?,
        ))
    }
}

impl HashComponent for MapDecorationEntry {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(4);
        push_hash_entry(&mut entries, "type", &self.decoration_type);
        push_hash_entry(&mut entries, "x", &self.x);
        push_hash_entry(&mut entries, "z", &self.z);
        push_hash_entry(&mut entries, "rotation", &self.rotation);
        hash_entries(hasher, &mut entries);
    }
}

/// Decorations keyed by their arbitrary map-local IDs.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MapDecorations {
    decorations: BTreeMap<String, MapDecorationEntry>,
}

impl MapDecorations {
    pub const EMPTY: Self = Self::empty();

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            decorations: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn new(decorations: BTreeMap<String, MapDecorationEntry>) -> Self {
        Self { decorations }
    }

    #[must_use]
    pub const fn decorations(&self) -> &BTreeMap<String, MapDecorationEntry> {
        &self.decorations
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.decorations.is_empty()
    }

    /// Mirrors Vanilla's immutable `withDecoration` copy-and-put operation.
    #[must_use]
    pub fn with_decoration(&self, id: String, entry: MapDecorationEntry) -> Self {
        let mut decorations = self.decorations.clone();
        decorations.insert(id, entry);
        Self::new(decorations)
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        for (id, decoration) in &self.decorations {
            compound.insert(id.as_str(), decoration.to_nbt_tag_ref());
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let decorations = compound
            .iter()
            .map(|(id, entry)| {
                Some((
                    id.to_owned().try_into_string().ok()?,
                    MapDecorationEntry::from_owned_nbt(entry)?,
                ))
            })
            .collect::<Option<BTreeMap<_, _>>>()?;
        Some(Self::new(decorations))
    }
}

impl WriteTo for MapDecorations {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut encoded = Vec::new();
        self.to_nbt_tag_ref().write(&mut encoded);
        writer.write_all(&encoded)
    }
}

impl ReadFrom for MapDecorations {
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
            .ok_or_else(|| Error::other("Map decorations network value is malformed"))
    }
}

impl ToNbtTag for MapDecorations {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for MapDecorations {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for MapDecorations {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(self.decorations.len());
        for (id, decoration) in &self.decorations {
            push_hash_entry(&mut entries, id, decoration);
        }
        hash_entries(hasher, &mut entries);
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

fn hash_entries(hasher: &mut ComponentHasher, entries: &mut [HashEntry]) {
    sort_map_entries(entries);
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

const fn java_double_equals(left: f64, right: f64) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

const fn java_float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{MapDecorationEntry, MapDecorations};
    use crate::data_components::vanilla_components::MAP_DECORATIONS;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt, RegistryReference, vanilla_items};

    fn parse(tag: NbtTag) -> Option<MapDecorations> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        MapDecorations::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn decoration_maps_round_trip_codec_derived_network_and_hash() {
        init_test_registry();
        let player = REGISTRY
            .map_decoration_types
            .by_key(&steel_utils::Identifier::vanilla_static("player"))
            .expect("player decoration should be registered");
        let value = MapDecorations::new(BTreeMap::from([(
            "home".to_owned(),
            MapDecorationEntry::new(RegistryReference::new(player), 12.5, -3.0, 45.0),
        )]));

        let mut entry = NbtCompound::new();
        entry.insert("type", "minecraft:player");
        entry.insert("x", 12.5_f64);
        entry.insert("z", -3.0_f64);
        entry.insert("rotation", 45.0_f32);
        let mut decorations = NbtCompound::new();
        decorations.insert("home", entry);
        let nbt = NbtTag::Compound(decorations);

        assert_eq!(value.clone().to_nbt_tag(), nbt);
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        assert_eq!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("decorations should encode");
        assert_eq!(
            MapDecorations::read(&mut Cursor::new(network.as_slice()))
                .expect("decorations should decode"),
            value
        );
    }

    #[test]
    fn extracted_filled_map_has_empty_decorations() {
        init_test_registry();
        let filled_map = REGISTRY
            .items
            .by_key(&vanilla_items::FILLED_MAP.key)
            .expect("filled map should be registered");
        assert_eq!(
            filled_map.components.get(MAP_DECORATIONS),
            Some(MapDecorations::EMPTY)
        );
    }

    #[test]
    fn entry_equality_matches_java_record_float_semantics() {
        init_test_registry();
        let player = REGISTRY
            .map_decoration_types
            .by_key(&steel_utils::Identifier::vanilla_static("player"))
            .expect("player decoration should be registered");
        let player = RegistryReference::new(player);

        assert_eq!(
            MapDecorationEntry::new(player, f64::NAN, 0.0, f32::NAN),
            MapDecorationEntry::new(
                player,
                f64::from_bits(0x7ff0_0000_0000_0001),
                0.0,
                f32::from_bits(0x7f80_0001),
            )
        );
        assert_ne!(
            MapDecorationEntry::new(player, 0.0, 0.0, 0.0),
            MapDecorationEntry::new(player, -0.0, 0.0, 0.0)
        );
    }
}
