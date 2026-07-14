//! Registry-backed holder sets used by vanilla codecs.

use std::fmt::Debug;
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::{
    Identifier,
    codec::VarInt,
    hash::{ComponentHasher, HashComponent},
    serial::{ReadFrom, WriteTo},
};

use crate::attribute::Attribute;
use crate::banner_pattern::BannerPattern;
use crate::blocks::Block;
use crate::damage_type::DamageType;
use crate::enchantment::Enchantment;
use crate::entity_type::EntityType;
use crate::items::Item;
use crate::jukebox_song::JukeboxSong;
use crate::mob_effect::MobEffect;
use crate::potion::Potion;
use crate::trim_material::TrimMaterial;
use crate::trim_pattern::TrimPattern;
use crate::villager_type::VillagerType;
use crate::{REGISTRY, RegistryEntry, RegistryExt, TaggedRegistryExt};

/// Registry operations required by a [`RegistryHolderSet`].
///
/// The trait keeps the holder-set codec independent from Steel's concrete
/// registries. Plugin-owned entry types can provide the same operations through
/// their own registry implementation.
pub trait RegistryHolderSetEntry: RegistryEntry + Debug + Send + Sync {
    /// Human-readable registry name used in codec errors.
    const REGISTRY_NAME: &'static str;

    /// Looks up an entry by its protocol registry ID.
    fn holder_set_by_id(id: usize) -> Option<&'static Self>;

    /// Looks up an entry by its registry key.
    fn holder_set_by_key(key: &Identifier) -> Option<&'static Self>;

    /// Returns whether the registry contains this tag.
    fn holder_set_tag_exists(tag: &Identifier) -> bool;

    /// Returns whether an entry belongs to this tag.
    fn holder_set_tag_contains(entry: &'static Self, tag: &Identifier) -> bool;
}

/// Vanilla's homogeneous holder-set representation for a registry.
#[derive(Debug, PartialEq)]
pub enum RegistryHolderSet<T: RegistryHolderSetEntry> {
    /// A named registry tag.
    Tag(Identifier),
    /// An ordered list of direct registry references.
    Direct(Vec<&'static T>),
}

impl<T: RegistryHolderSetEntry> Clone for RegistryHolderSet<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Tag(tag) => Self::Tag(tag.clone()),
            Self::Direct(entries) => Self::Direct(entries.clone()),
        }
    }
}

impl<T: RegistryHolderSetEntry> RegistryHolderSet<T> {
    /// Returns whether this holder set contains `entry`.
    #[must_use]
    pub fn contains(&self, entry: &'static T) -> bool {
        match self {
            Self::Tag(tag) => T::holder_set_tag_contains(entry, tag),
            Self::Direct(entries) => entries.contains(&entry),
        }
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if let Some(value) = tag.string() {
            let value = value.to_string();
            if let Some(tag) = value.strip_prefix('#') {
                let tag = Identifier::from_str(tag).ok()?;
                if !T::holder_set_tag_exists(&tag) {
                    return None;
                }
                return Some(Self::Tag(tag));
            }

            let key = Identifier::from_str(&value).ok()?;
            return Some(Self::Direct(vec![T::holder_set_by_key(&key)?]));
        }

        let list = tag.list()?;
        if list.as_nbt_tags().is_empty() {
            return Some(Self::Direct(Vec::new()));
        }
        let values = list.strings()?;
        let mut entries = Vec::with_capacity(values.len());
        for value in values {
            let key = Identifier::from_str(&value.to_string()).ok()?;
            entries.push(T::holder_set_by_key(&key)?);
        }
        Some(Self::Direct(entries))
    }
}

impl<T: RegistryHolderSetEntry> WriteTo for RegistryHolderSet<T> {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        match self {
            Self::Tag(tag) => {
                if !T::holder_set_tag_exists(tag) {
                    return Err(Error::other(format!(
                        "Unknown {} tag: {tag}",
                        T::REGISTRY_NAME
                    )));
                }
                VarInt(0).write(writer)?;
                tag.write(writer)
            }
            Self::Direct(entries) => {
                let count = i32::try_from(entries.len()).map_err(|_| {
                    Error::other(format!(
                        "{} holder set too large: {}",
                        T::REGISTRY_NAME,
                        entries.len()
                    ))
                })?;
                let encoded_count = count.checked_add(1).ok_or_else(|| {
                    Error::other(format!(
                        "{} holder set count exceeds protocol range",
                        T::REGISTRY_NAME
                    ))
                })?;
                VarInt(encoded_count).write(writer)?;
                for entry in entries {
                    let id = entry.try_id().ok_or_else(|| {
                        Error::other(format!("Unknown {}: {}", T::REGISTRY_NAME, entry.key()))
                    })?;
                    let id = i32::try_from(id).map_err(|_| {
                        Error::other(format!(
                            "{} id out of protocol range: {id}",
                            T::REGISTRY_NAME
                        ))
                    })?;
                    VarInt(id).write(writer)?;
                }
                Ok(())
            }
        }
    }
}

impl<T: RegistryHolderSetEntry> ReadFrom for RegistryHolderSet<T> {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let encoded_count = VarInt::read(data)?.0;
        if encoded_count == 0 {
            let tag = Identifier::read(data)?;
            if !T::holder_set_tag_exists(&tag) {
                return Err(Error::other(format!(
                    "Unknown {} tag: {tag}",
                    T::REGISTRY_NAME
                )));
            }
            return Ok(Self::Tag(tag));
        }

        let count = encoded_count
            .checked_sub(1)
            .and_then(|count| usize::try_from(count).ok())
            .ok_or_else(|| {
                Error::other(format!(
                    "Invalid {} holder set count: {encoded_count}",
                    T::REGISTRY_NAME
                ))
            })?;
        let mut entries = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            let id = VarInt::read(data)?.0;
            let id = usize::try_from(id)
                .map_err(|_| Error::other(format!("Negative {} id: {id}", T::REGISTRY_NAME)))?;
            let entry = T::holder_set_by_id(id)
                .ok_or_else(|| Error::other(format!("Unknown {} id: {id}", T::REGISTRY_NAME)))?;
            entries.push(entry);
        }
        Ok(Self::Direct(entries))
    }
}

impl<T: RegistryHolderSetEntry> ToNbtTag for RegistryHolderSet<T> {
    fn to_nbt_tag(self) -> NbtTag {
        match self {
            Self::Tag(tag) => NbtTag::String(format!("#{tag}").into()),
            Self::Direct(entries) if entries.is_empty() => NbtTag::List(NbtList::Empty),
            Self::Direct(entries) if entries.len() == 1 => {
                NbtTag::String(entries[0].key().to_string().into())
            }
            Self::Direct(entries) => NbtTag::List(NbtList::String(
                entries
                    .into_iter()
                    .map(|entry| entry.key().to_string().into())
                    .collect(),
            )),
        }
    }
}

impl<T: RegistryHolderSetEntry> FromNbtTag for RegistryHolderSet<T> {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl<T: RegistryHolderSetEntry> HashComponent for RegistryHolderSet<T> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        match self {
            Self::Tag(tag) => hasher.put_string(&format!("#{tag}")),
            Self::Direct(entries) if entries.len() == 1 => {
                hasher.put_string(&entries[0].key().to_string());
            }
            Self::Direct(entries) => {
                hasher.start_list();
                for entry in entries {
                    hasher.put_component_hash(&entry.key().to_string());
                }
                hasher.end_list();
            }
        }
    }
}

macro_rules! impl_registry_holder_set_entry {
    ($entry:ty, $registry:ident, $name:literal) => {
        impl RegistryHolderSetEntry for $entry {
            const REGISTRY_NAME: &'static str = $name;

            fn holder_set_by_id(id: usize) -> Option<&'static Self> {
                REGISTRY.$registry.by_id(id)
            }

            fn holder_set_by_key(key: &Identifier) -> Option<&'static Self> {
                REGISTRY.$registry.by_key(key)
            }

            fn holder_set_tag_exists(tag: &Identifier) -> bool {
                REGISTRY.$registry.get_tag(tag).is_some()
            }

            fn holder_set_tag_contains(entry: &'static Self, tag: &Identifier) -> bool {
                REGISTRY.$registry.is_in_tag(entry, tag)
            }
        }
    };
}

impl_registry_holder_set_entry!(Block, blocks, "block");
impl_registry_holder_set_entry!(BannerPattern, banner_patterns, "banner pattern");
impl_registry_holder_set_entry!(EntityType, entity_types, "entity type");
impl_registry_holder_set_entry!(Item, items, "item");
impl_registry_holder_set_entry!(DamageType, damage_types, "damage type");
impl_registry_holder_set_entry!(MobEffect, mob_effects, "mob effect");
impl_registry_holder_set_entry!(Enchantment, enchantments, "enchantment");
impl_registry_holder_set_entry!(Potion, potions, "potion");
impl_registry_holder_set_entry!(Attribute, attributes, "attribute");
impl_registry_holder_set_entry!(TrimMaterial, trim_materials, "trim material");
impl_registry_holder_set_entry!(TrimPattern, trim_patterns, "trim pattern");
impl_registry_holder_set_entry!(JukeboxSong, jukebox_songs, "jukebox song");
impl_registry_holder_set_entry!(VillagerType, villager_types, "villager type");

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
    use simdnbt::owned::{NbtList, NbtTag};
    use simdnbt::{FromNbtTag, ToNbtTag};
    use steel_utils::Identifier;
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom, WriteTo};

    use super::RegistryHolderSet;
    use crate::items::Item;
    use crate::test_support::init_test_registry;
    use crate::vanilla_item_tags::ItemTag;
    use crate::vanilla_items;

    fn with_borrowed_tag<R>(tag: NbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    #[test]
    fn persistent_codec_matches_vanilla_holder_set_shapes() {
        init_test_registry();

        let tag = RegistryHolderSet::<Item>::Tag(ItemTag::WOOL);
        assert_eq!(
            tag.clone().to_nbt_tag(),
            NbtTag::String("#minecraft:wool".into())
        );
        assert_eq!(
            with_borrowed_tag(
                tag.clone().to_nbt_tag(),
                RegistryHolderSet::<Item>::from_nbt_tag
            ),
            Some(tag)
        );
        assert_eq!(
            RegistryHolderSet::<Item>::Tag(ItemTag::WOOL).compute_hash(),
            NbtTag::String("#minecraft:wool".into()).compute_hash()
        );

        let singleton: RegistryHolderSet<Item> =
            RegistryHolderSet::Direct(vec![&vanilla_items::STICK]);
        assert_eq!(
            singleton.clone().to_nbt_tag(),
            NbtTag::String("minecraft:stick".into())
        );
        assert_eq!(
            with_borrowed_tag(
                singleton.clone().to_nbt_tag(),
                RegistryHolderSet::<Item>::from_nbt_tag
            ),
            Some(singleton)
        );

        let direct: RegistryHolderSet<Item> =
            RegistryHolderSet::Direct(vec![&vanilla_items::STICK, &vanilla_items::DIAMOND]);
        assert_eq!(
            with_borrowed_tag(
                direct.clone().to_nbt_tag(),
                RegistryHolderSet::<Item>::from_nbt_tag
            ),
            Some(direct.clone())
        );
        assert_eq!(
            direct.compute_hash(),
            direct.clone().to_nbt_tag().compute_hash()
        );

        let empty = RegistryHolderSet::<Item>::Direct(Vec::new());
        assert_eq!(empty.clone().to_nbt_tag(), NbtTag::List(NbtList::Empty));
        assert_eq!(
            with_borrowed_tag(
                NbtTag::List(NbtList::Empty),
                RegistryHolderSet::<Item>::from_nbt_tag
            ),
            Some(empty)
        );
    }

    #[test]
    fn network_codec_round_trips_tag_direct_and_empty_sets() {
        init_test_registry();

        for holder_set in [
            RegistryHolderSet::<Item>::Tag(ItemTag::WOOL),
            RegistryHolderSet::Direct(vec![&vanilla_items::STICK, &vanilla_items::DIAMOND]),
            RegistryHolderSet::Direct(Vec::new()),
        ] {
            let mut bytes = Vec::new();
            holder_set
                .write(&mut bytes)
                .expect("holder set should write");
            assert_eq!(
                RegistryHolderSet::<Item>::read(&mut Cursor::new(bytes.as_slice()))
                    .expect("holder set should read"),
                holder_set
            );
        }
    }

    #[test]
    fn codecs_reject_unknown_registry_values() {
        init_test_registry();

        let unknown_tag = NbtTag::String("#steel:missing".into());
        assert_eq!(
            with_borrowed_tag(unknown_tag, RegistryHolderSet::<Item>::from_nbt_tag),
            None
        );
        let unknown_entry = NbtTag::String("steel:missing".into());
        assert_eq!(
            with_borrowed_tag(unknown_entry, RegistryHolderSet::<Item>::from_nbt_tag),
            None
        );

        let mut invalid_count = Vec::new();
        VarInt(-1)
            .write(&mut invalid_count)
            .expect("test count should write");
        assert!(
            RegistryHolderSet::<Item>::read(&mut Cursor::new(invalid_count.as_slice())).is_err()
        );

        let mut unknown_id = Vec::new();
        VarInt(2)
            .write(&mut unknown_id)
            .expect("test count should write");
        VarInt(i32::MAX)
            .write(&mut unknown_id)
            .expect("test id should write");
        assert!(RegistryHolderSet::<Item>::read(&mut Cursor::new(unknown_id.as_slice())).is_err());
    }

    #[test]
    fn contains_resolves_tags_and_direct_entries() {
        init_test_registry();

        let tag = RegistryHolderSet::<Item>::Tag(ItemTag::WOOL);
        assert!(tag.contains(&vanilla_items::WHITE_WOOL));
        assert!(!tag.contains(&vanilla_items::STICK));

        let direct: RegistryHolderSet<Item> =
            RegistryHolderSet::Direct(vec![&vanilla_items::STICK]);
        assert!(direct.contains(&vanilla_items::STICK));
        assert!(!direct.contains(&vanilla_items::DIAMOND));
    }

    #[test]
    fn string_codec_rejects_malformed_identifiers() {
        init_test_registry();

        let malformed = NbtTag::String("not an identifier".into());
        assert_eq!(
            with_borrowed_tag(malformed, RegistryHolderSet::<Item>::from_nbt_tag),
            None
        );
        let missing_tag = Identifier::new_static("steel", "missing");
        assert!(
            RegistryHolderSet::<Item>::Tag(missing_tag)
                .write(&mut Vec::new())
                .is_err()
        );
    }
}
