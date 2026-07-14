//! Registry-dispatched partial data-component predicates.

use std::fmt::{self, Debug, Formatter};
use std::io::{Cursor, Error, Result, Write};

use rustc_hash::{FxHashMap, FxHashSet};
use simdnbt::ToNbtTag as _;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};
use steel_utils::nbt::NbtNumeric;
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier};
use text_components::TextComponent;

use crate::attribute::{Attribute, AttributeModifierOperation};
use crate::data_components::{ComponentData, ComponentEntryRef, DataComponentMap};
use crate::enchantment::Enchantment;
use crate::equipment::EquipmentSlotGroup;
use crate::item_predicate::{
    DoubleBounds, IntBounds, ItemPredicate, NbtPredicate, decode_optional, hash_entries,
    push_hash_entry, read_len, read_network_nbt, write_len,
};
use crate::jukebox_song::JukeboxSong;
use crate::potion::Potion;
use crate::trim_material::TrimMaterial;
use crate::trim_pattern::TrimPattern;
use crate::villager_type::VillagerType;
use crate::{REGISTRY, RegistryEntry, RegistryExt, RegistryHolderSet};

/// Typed payload behavior for a registered partial component predicate.
pub trait DataComponentPredicateCodec:
    DowncastType + Clone + Debug + PartialEq + HashComponent + Send + Sync + 'static
{
    fn from_nbt_value(tag: &NbtTag) -> Option<Self>;
    fn to_nbt_value(&self) -> NbtTag;
}

trait ErasedDataComponentPredicate: ErasedType + Debug + Send + Sync {
    fn clone_predicate(&self) -> Box<dyn ErasedDataComponentPredicate>;
    fn predicate_eq(&self, other: &dyn ErasedDataComponentPredicate) -> bool;
}

impl<T: DataComponentPredicateCodec> ErasedDataComponentPredicate for T {
    fn clone_predicate(&self) -> Box<dyn ErasedDataComponentPredicate> {
        Box::new(self.clone())
    }

    fn predicate_eq(&self, other: &dyn ErasedDataComponentPredicate) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }
}

type PredicateReader = fn(&NbtTag) -> Option<Box<dyn ErasedDataComponentPredicate>>;
type PredicateWriter = fn(&dyn ErasedDataComponentPredicate) -> NbtTag;
type PredicateHasher = fn(&dyn ErasedDataComponentPredicate, &mut ComponentHasher);

/// Registered discriminator and codecs for one concrete predicate value.
pub struct DataComponentPredicateType {
    pub key: Identifier,
    expected_type_key: DowncastTypeKey,
    reader: PredicateReader,
    writer: PredicateWriter,
    hasher: PredicateHasher,
}

impl DataComponentPredicateType {
    #[must_use]
    pub const fn of<T: DataComponentPredicateCodec>(key: Identifier) -> Self {
        Self {
            key,
            expected_type_key: T::TYPE_KEY,
            reader: read_predicate::<T>,
            writer: write_predicate::<T>,
            hasher: hash_predicate::<T>,
        }
    }
}

impl Debug for DataComponentPredicateType {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DataComponentPredicateType")
            .field("key", &self.key)
            .field("expected_type_key", &self.expected_type_key)
            .finish_non_exhaustive()
    }
}

pub type DataComponentPredicateTypeRef = &'static DataComponentPredicateType;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PredicateDiscriminator {
    Concrete(DataComponentPredicateTypeRef),
    Any(ComponentEntryRef),
}

impl PredicateDiscriminator {
    const fn key(&self) -> &Identifier {
        match *self {
            Self::Concrete(predicate_type) => &predicate_type.key,
            Self::Any(component) => &component.key,
        }
    }
}

impl Debug for PredicateDiscriminator {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PredicateDiscriminator")
            .field(self.key())
            .finish()
    }
}

/// Type-erased predicate value retaining Steel's deterministic concrete key.
pub struct DataComponentPredicateData {
    discriminator: PredicateDiscriminator,
    value: Option<Box<dyn ErasedDataComponentPredicate>>,
}

impl DataComponentPredicateData {
    #[must_use]
    pub fn new<T: DataComponentPredicateCodec>(
        predicate_type: DataComponentPredicateTypeRef,
        value: T,
    ) -> Self {
        assert_eq!(
            predicate_type.expected_type_key,
            T::TYPE_KEY,
            "component predicate value does not match its registered type"
        );
        Self {
            discriminator: PredicateDiscriminator::Concrete(predicate_type),
            value: Some(Box::new(value)),
        }
    }

    #[must_use]
    pub const fn any(component: ComponentEntryRef) -> Self {
        Self {
            discriminator: PredicateDiscriminator::Any(component),
            value: None,
        }
    }

    #[must_use]
    pub const fn predicate_type(&self) -> Option<DataComponentPredicateTypeRef> {
        match self.discriminator {
            PredicateDiscriminator::Concrete(predicate_type) => Some(predicate_type),
            PredicateDiscriminator::Any(_) => None,
        }
    }

    #[must_use]
    pub const fn any_component(&self) -> Option<ComponentEntryRef> {
        match self.discriminator {
            PredicateDiscriminator::Concrete(_) => None,
            PredicateDiscriminator::Any(component) => Some(component),
        }
    }

    #[must_use]
    pub fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.value.as_deref()?.downcast_ref::<T>()
    }

    #[must_use]
    pub const fn key(&self) -> &Identifier {
        self.discriminator.key()
    }

    fn from_persistent_entry(key: &Identifier, tag: &NbtTag) -> Option<Self> {
        if let Some(predicate_type) = REGISTRY.data_component_predicate_types.by_key(key) {
            return Some(Self {
                discriminator: PredicateDiscriminator::Concrete(predicate_type),
                value: Some((predicate_type.reader)(tag)?),
            });
        }
        let component = REGISTRY.data_components.by_key(key)?;
        tag.compound()?;
        Some(Self::any(component))
    }

    fn to_nbt_value(&self) -> NbtTag {
        match (self.discriminator, self.value.as_deref()) {
            (PredicateDiscriminator::Concrete(predicate_type), Some(value)) => {
                (predicate_type.writer)(value)
            }
            (PredicateDiscriminator::Any(_), None) => NbtTag::Compound(NbtCompound::new()),
            _ => panic!("component predicate discriminator and value disagree"),
        }
    }

    fn hash_value(&self, hasher: &mut ComponentHasher) {
        match (self.discriminator, self.value.as_deref()) {
            (PredicateDiscriminator::Concrete(predicate_type), Some(value)) => {
                (predicate_type.hasher)(value, hasher);
            }
            (PredicateDiscriminator::Any(_), None) => {
                hasher.start_map();
                hasher.end_map();
            }
            _ => panic!("component predicate discriminator and value disagree"),
        }
    }
}

impl Clone for DataComponentPredicateData {
    fn clone(&self) -> Self {
        Self {
            discriminator: self.discriminator,
            value: self.value.as_ref().map(|value| value.clone_predicate()),
        }
    }
}

impl Debug for DataComponentPredicateData {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DataComponentPredicateData")
            .field("key", self.key())
            .field("value", &self.value)
            .finish()
    }
}

impl PartialEq for DataComponentPredicateData {
    fn eq(&self, other: &Self) -> bool {
        if self.discriminator != other.discriminator {
            return false;
        }
        match (self.value.as_deref(), other.value.as_deref()) {
            (Some(left), Some(right)) => left.predicate_eq(right),
            (None, None) => true,
            _ => false,
        }
    }
}

/// Registry of concrete partial component predicate types.
pub struct DataComponentPredicateTypeRegistry {
    types_by_id: Vec<DataComponentPredicateTypeRef>,
    types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl DataComponentPredicateTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types_by_id: Vec::new(),
            types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    DataComponentPredicateTypeRegistry,
    DataComponentPredicateTypeRef,
    types_by_id,
    types_by_key,
    allows_registering
);
crate::impl_registry!(
    DataComponentPredicateTypeRegistry,
    DataComponentPredicateType,
    types_by_id,
    types_by_key,
    data_component_predicate_types
);

fn read_predicate<T: DataComponentPredicateCodec>(
    tag: &NbtTag,
) -> Option<Box<dyn ErasedDataComponentPredicate>> {
    T::from_nbt_value(tag).map(|value| Box::new(value) as Box<dyn ErasedDataComponentPredicate>)
}

fn write_predicate<T: DataComponentPredicateCodec>(
    value: &dyn ErasedDataComponentPredicate,
) -> NbtTag {
    let Some(value) = value.downcast_ref::<T>() else {
        panic!("registered component predicate writer received the wrong concrete type");
    };
    value.to_nbt_value()
}

fn hash_predicate<T: DataComponentPredicateCodec>(
    value: &dyn ErasedDataComponentPredicate,
    hasher: &mut ComponentHasher,
) {
    let Some(value) = value.downcast_ref::<T>() else {
        panic!("registered component predicate hasher received the wrong concrete type");
    };
    value.hash_component(hasher);
}

/// Exact component values required by a component matcher.
#[derive(Clone, PartialEq)]
pub struct DataComponentExactPredicate {
    values: Vec<(ComponentEntryRef, ComponentData)>,
}

impl Debug for DataComponentExactPredicate {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.values.iter().map(|(entry, value)| (&entry.key, value)))
            .finish()
    }
}

impl DataComponentExactPredicate {
    pub const EMPTY: Self = Self { values: Vec::new() };

    /// Creates an exact predicate only when every persistent value can round-trip.
    ///
    /// Vanilla's direct stream codec can admit values rejected by a component's
    /// persistent codec. Steel rejects those here because exact predicates are
    /// nested in item components and must not make their containing stack unsavable.
    #[must_use]
    pub fn new(values: Vec<(ComponentEntryRef, ComponentData)>) -> Option<Self> {
        let mut keys = FxHashSet::default();
        values
            .iter()
            .all(|(entry, value)| {
                entry.validates(value)
                    && keys.insert(entry.key.clone())
                    && (!entry.is_persistent() || entry.validate_persistent_encoding(value).is_ok())
            })
            .then_some(Self { values })
    }

    #[must_use]
    pub fn all_of(components: &DataComponentMap) -> Option<Self> {
        let values = components
            .keys()
            .map(|key| {
                Some((
                    REGISTRY.data_components.by_key(key)?,
                    components.get_raw(key)?.clone(),
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        Self::new(values)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[must_use]
    pub fn values(&self) -> &[(ComponentEntryRef, ComponentData)] {
        &self.values
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let mut values = Vec::with_capacity(compound.len());
        for (key, value) in compound.iter() {
            let key = key.to_owned().try_into_string().ok()?.parse().ok()?;
            let entry = REGISTRY.data_components.by_key(&key)?;
            if !entry.is_persistent() {
                return None;
            }
            values.push((entry, entry.read_nbt_owned(value)?));
        }
        Self::new(values)
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        for (entry, value) in &self.values {
            if !entry.is_persistent() {
                continue;
            }
            let Ok(value) = entry.write_nbt(value) else {
                panic!("validated exact component predicate failed to encode");
            };
            compound.insert(entry.key.to_string(), value);
        }
        NbtTag::Compound(compound)
    }
}

impl WriteTo for DataComponentExactPredicate {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_len(self.values.len(), writer)?;
        for (entry, value) in &self.values {
            write_registry_id(*entry, writer, "data component")?;
            let mut encoded = Vec::new();
            entry.write_network(value, &mut encoded)?;
            writer.write_all(&encoded)?;
        }
        Ok(())
    }
}

impl ReadFrom for DataComponentExactPredicate {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_len(data, usize::MAX, "exact component predicate")?;
        let mut values = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let entry = read_component_entry(data)?;
            values.push((entry, entry.read_network(data)?));
        }
        Self::new(values)
            .ok_or_else(|| Error::other("duplicate or mismatched exact component predicate"))
    }
}

impl HashComponent for DataComponentExactPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        for (entry, value) in &self.values {
            if !entry.is_persistent() {
                continue;
            }
            let Ok(value_hash) = entry.compute_hash(value) else {
                panic!("validated exact component predicate failed to hash");
            };
            entries.push(HashEntry::from_hashes(
                entry.key.compute_hash() as u32,
                value_hash as u32,
            ));
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Exact and partial component conditions flattened into block/item predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct DataComponentMatchers {
    exact: DataComponentExactPredicate,
    partial: Vec<DataComponentPredicateData>,
}

impl DataComponentMatchers {
    pub const ANY: Self = Self {
        exact: DataComponentExactPredicate::EMPTY,
        partial: Vec::new(),
    };

    #[must_use]
    pub fn new(
        exact: DataComponentExactPredicate,
        partial: Vec<DataComponentPredicateData>,
    ) -> Option<Self> {
        let mut keys = FxHashSet::default();
        partial
            .iter()
            .all(|predicate| keys.insert(predicate.key().clone()))
            .then_some(Self { exact, partial })
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.partial.is_empty()
    }

    #[must_use]
    pub const fn exact(&self) -> &DataComponentExactPredicate {
        &self.exact
    }

    #[must_use]
    pub fn partial(&self) -> &[DataComponentPredicateData] {
        &self.partial
    }

    pub(crate) fn from_fields(compound: &NbtCompound) -> Option<Self> {
        let exact = compound.get("components").map_or(
            Some(DataComponentExactPredicate::EMPTY),
            DataComponentExactPredicate::from_owned_nbt,
        )?;
        let partial = if let Some(tag) = compound.get("predicates") {
            let values = tag.compound()?;
            let mut predicates = Vec::with_capacity(values.len());
            for (key, value) in values.iter() {
                let key = key.to_owned().try_into_string().ok()?.parse().ok()?;
                predicates.push(DataComponentPredicateData::from_persistent_entry(
                    &key, value,
                )?);
            }
            predicates
        } else {
            Vec::new()
        };
        Self::new(exact, partial)
    }

    pub(crate) fn write_fields(&self, compound: &mut NbtCompound) {
        if !self.exact.is_empty() {
            compound.insert("components", self.exact.to_nbt_value());
        }
        if !self.partial.is_empty() {
            let mut predicates = NbtCompound::new();
            for predicate in &self.partial {
                predicates.insert(predicate.key().to_string(), predicate.to_nbt_value());
            }
            compound.insert("predicates", predicates);
        }
    }

    pub(crate) fn hash_fields(&self, entries: &mut Vec<HashEntry>) {
        if !self.exact.is_empty() {
            push_hash_entry(entries, "components", &self.exact);
        }
        if !self.partial.is_empty() {
            let mut value_hasher = ComponentHasher::new();
            self.hash_partial(&mut value_hasher);
            super::item_predicate::push_prehashed_entry(entries, "predicates", value_hasher);
        }
    }

    fn hash_partial(&self, hasher: &mut ComponentHasher) {
        let mut entries = self
            .partial
            .iter()
            .map(|predicate| {
                let mut key_hasher = ComponentHasher::new();
                predicate.key().hash_component(&mut key_hasher);
                let mut value_hasher = ComponentHasher::new();
                predicate.hash_value(&mut value_hasher);
                HashEntry::new(key_hasher, value_hasher)
            })
            .collect::<Vec<_>>();
        hash_entries(hasher, &mut entries);
    }
}

impl WriteTo for DataComponentMatchers {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.exact.write(writer)?;
        if self.partial.len() > 64 {
            return Err(Error::other("partial component predicate count exceeds 64"));
        }
        write_len(self.partial.len(), writer)?;
        for predicate in &self.partial {
            match predicate.discriminator {
                PredicateDiscriminator::Concrete(predicate_type) => {
                    true.write(writer)?;
                    write_registry_id(predicate_type, writer, "component predicate type")?;
                }
                PredicateDiscriminator::Any(component) => {
                    false.write(writer)?;
                    write_registry_id(component, writer, "data component")?;
                }
            }
            let mut encoded = Vec::new();
            predicate.to_nbt_value().write(&mut encoded);
            writer.write_all(&encoded)?;
        }
        Ok(())
    }
}

impl ReadFrom for DataComponentMatchers {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let exact = DataComponentExactPredicate::read(data)?;
        let count = read_len(data, 64, "partial component predicate")?;
        let mut partial = Vec::with_capacity(count);
        for _ in 0..count {
            let discriminator = if bool::read(data)? {
                let id = read_registry_id(data, "component predicate type")?;
                PredicateDiscriminator::Concrete(
                    REGISTRY
                        .data_component_predicate_types
                        .by_id(id)
                        .ok_or_else(|| {
                            Error::other(format!("unknown component predicate type id: {id}"))
                        })?,
                )
            } else {
                PredicateDiscriminator::Any(read_component_entry(data)?)
            };
            let tag = read_network_nbt(data)?;
            let value = match discriminator {
                PredicateDiscriminator::Concrete(predicate_type) => Some(
                    (predicate_type.reader)(&tag)
                        .ok_or_else(|| Error::other("invalid component predicate payload"))?,
                ),
                PredicateDiscriminator::Any(_) => {
                    if tag.compound().is_none() {
                        return Err(Error::other(
                            "any-value predicate payload is not a compound",
                        ));
                    }
                    None
                }
            };
            partial.push(DataComponentPredicateData {
                discriminator,
                value,
            });
        }
        Self::new(exact, partial)
            .ok_or_else(|| Error::other("duplicate partial component predicate"))
    }
}

impl HashComponent for DataComponentMatchers {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        self.hash_fields(&mut entries);
        hash_entries(hasher, &mut entries);
    }
}

fn write_registry_id(
    entry: &impl RegistryEntry,
    writer: &mut impl Write,
    name: &str,
) -> Result<()> {
    let id = entry
        .try_id()
        .ok_or_else(|| Error::other(format!("unknown {name}: {}", entry.key())))?;
    let id = i32::try_from(id).map_err(|_| Error::other(format!("{name} id out of range")))?;
    VarInt(id).write(writer)
}

fn read_registry_id(data: &mut Cursor<&[u8]>, name: &str) -> Result<usize> {
    let id = VarInt::read(data)?.0;
    usize::try_from(id).map_err(|_| Error::other(format!("negative {name} id: {id}")))
}

fn read_component_entry(data: &mut Cursor<&[u8]>) -> Result<ComponentEntryRef> {
    let id = read_registry_id(data, "data component")?;
    REGISTRY
        .data_components
        .by_id(id)
        .ok_or_else(|| Error::other(format!("unknown data component id: {id}")))
}

macro_rules! impl_predicate_downcast_type {
    ($type:ty, $key:literal) => {
        // SAFETY: This Steel-owned key uniquely identifies the concrete
        // predicate implementation within the process.
        unsafe impl DowncastType for $type {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($key);
        }
    };
}

/// Generic collection predicate shared by container, firework, book, and attribute checks.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionPredicate<P> {
    contains: Option<Vec<P>>,
    counts: Option<Vec<CollectionCountPredicate<P>>>,
    size: Option<IntBounds>,
}

impl<P> CollectionPredicate<P> {
    #[must_use]
    pub const fn new(
        contains: Option<Vec<P>>,
        counts: Option<Vec<CollectionCountPredicate<P>>>,
        size: Option<IntBounds>,
    ) -> Self {
        Self {
            contains,
            counts,
            size,
        }
    }

    #[must_use]
    pub const fn contains(&self) -> Option<&Vec<P>> {
        self.contains.as_ref()
    }

    #[must_use]
    pub const fn counts(&self) -> Option<&Vec<CollectionCountPredicate<P>>> {
        self.counts.as_ref()
    }

    #[must_use]
    pub const fn size(&self) -> Option<&IntBounds> {
        self.size.as_ref()
    }

    fn from_nbt_with(tag: &NbtTag, decode: impl Fn(&NbtTag) -> Option<P> + Copy) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode_optional(compound, "contains", |tag| decode_list(tag, decode))?,
            decode_optional(compound, "count", |tag| {
                decode_list(tag, |tag| {
                    CollectionCountPredicate::from_nbt_with(tag, decode)
                })
            })?,
            decode_optional(compound, "size", IntBounds::from_owned_nbt)?,
        ))
    }

    fn to_nbt_with(&self, encode: impl Fn(&P) -> NbtTag + Copy) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(contains) = &self.contains {
            compound.insert("contains", encode_list(contains, encode));
        }
        if let Some(counts) = &self.counts {
            compound.insert(
                "count",
                encode_list(counts, |entry| entry.to_nbt_with(encode)),
            );
        }
        if let Some(size) = &self.size {
            compound.insert("size", size.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }

    fn hash_with(&self, hasher: &mut ComponentHasher, hash: impl Fn(&P) -> i32 + Copy) {
        let mut entries = Vec::new();
        if let Some(contains) = &self.contains {
            let mut value_hasher = ComponentHasher::new();
            hash_list_with(contains, &mut value_hasher, hash);
            super::item_predicate::push_prehashed_entry(&mut entries, "contains", value_hasher);
        }
        if let Some(counts) = &self.counts {
            let mut value_hasher = ComponentHasher::new();
            value_hasher.start_list();
            for entry in counts {
                let mut entry_hasher = ComponentHasher::new();
                entry.hash_with(&mut entry_hasher, hash);
                value_hasher.put_raw_bytes(&(entry_hasher.finish() as u32).to_le_bytes());
            }
            value_hasher.end_list();
            super::item_predicate::push_prehashed_entry(&mut entries, "count", value_hasher);
        }
        if let Some(size) = &self.size {
            push_hash_entry(&mut entries, "size", size);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// One element predicate and the accepted number of matching elements.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionCountPredicate<P> {
    test: P,
    count: IntBounds,
}

impl<P> CollectionCountPredicate<P> {
    #[must_use]
    pub const fn new(test: P, count: IntBounds) -> Self {
        Self { test, count }
    }

    #[must_use]
    pub const fn test(&self) -> &P {
        &self.test
    }

    #[must_use]
    pub const fn count(&self) -> IntBounds {
        self.count
    }

    fn from_nbt_with(tag: &NbtTag, decode: impl Fn(&NbtTag) -> Option<P>) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode(compound.get("test")?)?,
            IntBounds::from_owned_nbt(compound.get("count")?)?,
        ))
    }

    fn to_nbt_with(&self, encode: impl Fn(&P) -> NbtTag) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("test", encode(&self.test));
        compound.insert("count", self.count.as_nbt_tag());
        NbtTag::Compound(compound)
    }

    fn hash_with(&self, hasher: &mut ComponentHasher, hash: impl Fn(&P) -> i32) {
        let mut entries = Vec::new();
        let mut key_hasher = ComponentHasher::new();
        "test".hash_component(&mut key_hasher);
        entries.push(HashEntry::from_hashes(
            key_hasher.finish() as u32,
            hash(&self.test) as u32,
        ));
        push_hash_entry(&mut entries, "count", &self.count);
        hash_entries(hasher, &mut entries);
    }
}

/// Durability and current-damage bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamagePredicate {
    durability: IntBounds,
    damage: IntBounds,
}

impl DamagePredicate {
    #[must_use]
    pub const fn new(durability: IntBounds, damage: IntBounds) -> Self {
        Self { durability, damage }
    }

    #[must_use]
    pub const fn durability(&self) -> IntBounds {
        self.durability
    }

    #[must_use]
    pub const fn damage(&self) -> IntBounds {
        self.damage
    }
}

impl DataComponentPredicateCodec for DamagePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            compound
                .get("durability")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
            compound
                .get("damage")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
        ))
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !self.durability.is_any() {
            compound.insert("durability", self.durability.as_nbt_tag());
        }
        if !self.damage.is_any() {
            compound.insert("damage", self.damage.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for DamagePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_nbt_codec(self, hasher);
    }
}

impl_predicate_downcast_type!(DamagePredicate, "steel:data_component_predicate/damage");

/// One enchantment holder-set and accepted level range.
#[derive(Debug, Clone, PartialEq)]
pub struct EnchantmentPredicate {
    enchantments: Option<RegistryHolderSet<Enchantment>>,
    levels: IntBounds,
}

impl EnchantmentPredicate {
    #[must_use]
    pub const fn new(
        enchantments: Option<RegistryHolderSet<Enchantment>>,
        levels: IntBounds,
    ) -> Self {
        Self {
            enchantments,
            levels,
        }
    }

    #[must_use]
    pub const fn enchantments(&self) -> Option<&RegistryHolderSet<Enchantment>> {
        self.enchantments.as_ref()
    }

    #[must_use]
    pub const fn levels(&self) -> IntBounds {
        self.levels
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode_optional(compound, "enchantments", RegistryHolderSet::from_owned_nbt)?,
            compound
                .get("levels")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
        ))
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(enchantments) = &self.enchantments {
            compound.insert("enchantments", enchantments.clone().to_nbt_tag());
        }
        if !self.levels.is_any() {
            compound.insert("levels", self.levels.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

/// Applied-enchantment predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct EnchantmentsPredicate(Vec<EnchantmentPredicate>);

impl EnchantmentsPredicate {
    #[must_use]
    pub const fn new(enchantments: Vec<EnchantmentPredicate>) -> Self {
        Self(enchantments)
    }

    #[must_use]
    pub fn enchantments(&self) -> &[EnchantmentPredicate] {
        &self.0
    }
}

impl DataComponentPredicateCodec for EnchantmentsPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        decode_list(tag, EnchantmentPredicate::from_nbt_value).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        encode_list(&self.0, EnchantmentPredicate::to_nbt_value)
    }
}

impl HashComponent for EnchantmentsPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_nbt_codec(self, hasher);
    }
}

impl_predicate_downcast_type!(
    EnchantmentsPredicate,
    "steel:data_component_predicate/enchantments"
);

/// Stored-enchantment predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEnchantmentsPredicate(Vec<EnchantmentPredicate>);

impl StoredEnchantmentsPredicate {
    #[must_use]
    pub const fn new(enchantments: Vec<EnchantmentPredicate>) -> Self {
        Self(enchantments)
    }

    #[must_use]
    pub fn enchantments(&self) -> &[EnchantmentPredicate] {
        &self.0
    }
}

impl DataComponentPredicateCodec for StoredEnchantmentsPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        decode_list(tag, EnchantmentPredicate::from_nbt_value).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        encode_list(&self.0, EnchantmentPredicate::to_nbt_value)
    }
}

impl HashComponent for StoredEnchantmentsPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_nbt_codec(self, hasher);
    }
}

impl_predicate_downcast_type!(
    StoredEnchantmentsPredicate,
    "steel:data_component_predicate/stored_enchantments"
);

/// Accepted registered potion values.
#[derive(Debug, Clone, PartialEq)]
pub struct PotionsPredicate(RegistryHolderSet<Potion>);

impl PotionsPredicate {
    #[must_use]
    pub const fn new(potions: RegistryHolderSet<Potion>) -> Self {
        Self(potions)
    }

    #[must_use]
    pub const fn potions(&self) -> &RegistryHolderSet<Potion> {
        &self.0
    }
}

impl DataComponentPredicateCodec for PotionsPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        RegistryHolderSet::from_owned_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.clone().to_nbt_tag()
    }
}

impl HashComponent for PotionsPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    PotionsPredicate,
    "steel:data_component_predicate/potion_contents"
);

/// Partial custom-data NBT predicate.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomDataPredicate(NbtPredicate);

impl CustomDataPredicate {
    #[must_use]
    pub const fn new(value: NbtPredicate) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(&self) -> &NbtPredicate {
        &self.0
    }
}

impl DataComponentPredicateCodec for CustomDataPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        NbtPredicate::from_owned_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.to_nbt_tag_ref()
    }
}

impl HashComponent for CustomDataPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    CustomDataPredicate,
    "steel:data_component_predicate/custom_data"
);

/// Nested item predicates over container contents.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerPredicate(Option<CollectionPredicate<ItemPredicate>>);

impl ContainerPredicate {
    #[must_use]
    pub const fn new(items: Option<CollectionPredicate<ItemPredicate>>) -> Self {
        Self(items)
    }

    #[must_use]
    pub const fn items(&self) -> Option<&CollectionPredicate<ItemPredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for ContainerPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "items", |tag| {
            CollectionPredicate::from_nbt_with(tag, ItemPredicate::from_owned_nbt)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(self.0.as_ref(), "items", ItemPredicate::to_nbt_tag_ref)
    }
}

impl HashComponent for ContainerPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "items",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    ContainerPredicate,
    "steel:data_component_predicate/container"
);

/// Nested item predicates over bundle contents.
#[derive(Debug, Clone, PartialEq)]
pub struct BundlePredicate(Option<CollectionPredicate<ItemPredicate>>);

impl BundlePredicate {
    #[must_use]
    pub const fn new(items: Option<CollectionPredicate<ItemPredicate>>) -> Self {
        Self(items)
    }

    #[must_use]
    pub const fn items(&self) -> Option<&CollectionPredicate<ItemPredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for BundlePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "items", |tag| {
            CollectionPredicate::from_nbt_with(tag, ItemPredicate::from_owned_nbt)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(self.0.as_ref(), "items", ItemPredicate::to_nbt_tag_ref)
    }
}

impl HashComponent for BundlePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "items",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    BundlePredicate,
    "steel:data_component_predicate/bundle_contents"
);

/// Fields matched within one firework explosion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireworkPredicate {
    shape: Option<crate::data_components::components::FireworkExplosionShape>,
    has_twinkle: Option<bool>,
    has_trail: Option<bool>,
}

impl FireworkPredicate {
    #[must_use]
    pub const fn new(
        shape: Option<crate::data_components::components::FireworkExplosionShape>,
        has_twinkle: Option<bool>,
        has_trail: Option<bool>,
    ) -> Self {
        Self {
            shape,
            has_twinkle,
            has_trail,
        }
    }

    #[must_use]
    pub const fn shape(
        &self,
    ) -> Option<crate::data_components::components::FireworkExplosionShape> {
        self.shape
    }

    #[must_use]
    pub const fn has_twinkle(&self) -> Option<bool> {
        self.has_twinkle
    }

    #[must_use]
    pub const fn has_trail(&self) -> Option<bool> {
        self.has_trail
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            shape: decode_optional(compound, "shape", |tag| {
                match tag.string()?.to_owned().try_into_string().ok()?.as_str() {
                    "small_ball" => {
                        Some(crate::data_components::components::FireworkExplosionShape::SmallBall)
                    }
                    "large_ball" => {
                        Some(crate::data_components::components::FireworkExplosionShape::LargeBall)
                    }
                    "star" => {
                        Some(crate::data_components::components::FireworkExplosionShape::Star)
                    }
                    "creeper" => {
                        Some(crate::data_components::components::FireworkExplosionShape::Creeper)
                    }
                    "burst" => {
                        Some(crate::data_components::components::FireworkExplosionShape::Burst)
                    }
                    _ => None,
                }
            })?,
            has_twinkle: decode_optional(compound, "has_twinkle", NbtNumeric::codec_bool)?,
            has_trail: decode_optional(compound, "has_trail", NbtNumeric::codec_bool)?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(shape) = self.shape {
            compound.insert("shape", shape.serialized_name());
        }
        if let Some(has_twinkle) = self.has_twinkle {
            compound.insert("has_twinkle", has_twinkle);
        }
        if let Some(has_trail) = self.has_trail {
            compound.insert("has_trail", has_trail);
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for FireworkPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(shape) = self.shape {
            push_hash_entry(&mut entries, "shape", shape.serialized_name());
        }
        if let Some(has_twinkle) = self.has_twinkle {
            push_hash_entry(&mut entries, "has_twinkle", &has_twinkle);
        }
        if let Some(has_trail) = self.has_trail {
            push_hash_entry(&mut entries, "has_trail", &has_trail);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Predicate over the `firework_explosion` component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireworkExplosionPredicate(FireworkPredicate);

impl FireworkExplosionPredicate {
    #[must_use]
    pub const fn new(predicate: FireworkPredicate) -> Self {
        Self(predicate)
    }

    #[must_use]
    pub const fn predicate(&self) -> &FireworkPredicate {
        &self.0
    }
}

impl DataComponentPredicateCodec for FireworkExplosionPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        FireworkPredicate::from_nbt_value(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.to_nbt_value()
    }
}

impl HashComponent for FireworkExplosionPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    FireworkExplosionPredicate,
    "steel:data_component_predicate/firework_explosion"
);

/// Predicate over firework explosions and flight duration.
#[derive(Debug, Clone, PartialEq)]
pub struct FireworksPredicate {
    explosions: Option<CollectionPredicate<FireworkPredicate>>,
    flight_duration: IntBounds,
}

impl FireworksPredicate {
    #[must_use]
    pub const fn new(
        explosions: Option<CollectionPredicate<FireworkPredicate>>,
        flight_duration: IntBounds,
    ) -> Self {
        Self {
            explosions,
            flight_duration,
        }
    }

    #[must_use]
    pub const fn explosions(&self) -> Option<&CollectionPredicate<FireworkPredicate>> {
        self.explosions.as_ref()
    }

    #[must_use]
    pub const fn flight_duration(&self) -> IntBounds {
        self.flight_duration
    }
}

impl DataComponentPredicateCodec for FireworksPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            explosions: decode_optional(compound, "explosions", |tag| {
                CollectionPredicate::from_nbt_with(tag, FireworkPredicate::from_nbt_value)
            })?,
            flight_duration: compound
                .get("flight_duration")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(explosions) = &self.explosions {
            compound.insert(
                "explosions",
                explosions.to_nbt_with(FireworkPredicate::to_nbt_value),
            );
        }
        if !self.flight_duration.is_any() {
            compound.insert("flight_duration", self.flight_duration.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for FireworksPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(explosions) = &self.explosions {
            let mut value_hasher = ComponentHasher::new();
            explosions.hash_with(&mut value_hasher, HashComponent::compute_hash);
            super::item_predicate::push_prehashed_entry(&mut entries, "explosions", value_hasher);
        }
        if !self.flight_duration.is_any() {
            push_hash_entry(&mut entries, "flight_duration", &self.flight_duration);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(
    FireworksPredicate,
    "steel:data_component_predicate/fireworks"
);

/// Predicate for one writable-book page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritableBookPagePredicate(String);

impl WritableBookPagePredicate {
    #[must_use]
    pub const fn new(contents: String) -> Self {
        Self(contents)
    }

    #[must_use]
    pub fn contents(&self) -> &str {
        &self.0
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        tag.string()?.to_owned().try_into_string().ok().map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        NbtTag::String(self.0.clone().into())
    }
}

impl HashComponent for WritableBookPagePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

/// Predicate over writable-book pages.
#[derive(Debug, Clone, PartialEq)]
pub struct WritableBookPredicate(Option<CollectionPredicate<WritableBookPagePredicate>>);

impl WritableBookPredicate {
    #[must_use]
    pub const fn new(pages: Option<CollectionPredicate<WritableBookPagePredicate>>) -> Self {
        Self(pages)
    }

    #[must_use]
    pub const fn pages(&self) -> Option<&CollectionPredicate<WritableBookPagePredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for WritableBookPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "pages", |tag| {
            CollectionPredicate::from_nbt_with(tag, WritableBookPagePredicate::from_nbt_value)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(
            self.0.as_ref(),
            "pages",
            WritableBookPagePredicate::to_nbt_value,
        )
    }
}

impl HashComponent for WritableBookPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "pages",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    WritableBookPredicate,
    "steel:data_component_predicate/writable_book_content"
);

/// Predicate for one written-book page.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenBookPagePredicate(TextComponent);

impl WrittenBookPagePredicate {
    #[must_use]
    pub const fn new(contents: TextComponent) -> Self {
        Self(contents)
    }

    #[must_use]
    pub const fn contents(&self) -> &TextComponent {
        &self.0
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        TextComponent::from_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.to_codec_nbt()
    }
}

impl HashComponent for WrittenBookPagePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

/// Predicate over written-book metadata and pages.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenBookPredicate {
    pages: Option<CollectionPredicate<WrittenBookPagePredicate>>,
    author: Option<String>,
    title: Option<String>,
    generation: IntBounds,
    resolved: Option<bool>,
}

impl WrittenBookPredicate {
    #[must_use]
    pub const fn new(
        pages: Option<CollectionPredicate<WrittenBookPagePredicate>>,
        author: Option<String>,
        title: Option<String>,
        generation: IntBounds,
        resolved: Option<bool>,
    ) -> Self {
        Self {
            pages,
            author,
            title,
            generation,
            resolved,
        }
    }

    #[must_use]
    pub const fn pages(&self) -> Option<&CollectionPredicate<WrittenBookPagePredicate>> {
        self.pages.as_ref()
    }

    #[must_use]
    pub const fn author(&self) -> Option<&String> {
        self.author.as_ref()
    }

    #[must_use]
    pub const fn title(&self) -> Option<&String> {
        self.title.as_ref()
    }

    #[must_use]
    pub const fn generation(&self) -> IntBounds {
        self.generation
    }

    #[must_use]
    pub const fn resolved(&self) -> Option<bool> {
        self.resolved
    }
}

impl DataComponentPredicateCodec for WrittenBookPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            pages: decode_optional(compound, "pages", |tag| {
                CollectionPredicate::from_nbt_with(tag, WrittenBookPagePredicate::from_nbt_value)
            })?,
            author: decode_optional(compound, "author", owned_string)?,
            title: decode_optional(compound, "title", owned_string)?,
            generation: compound
                .get("generation")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
            resolved: decode_optional(compound, "resolved", NbtNumeric::codec_bool)?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(pages) = &self.pages {
            compound.insert(
                "pages",
                pages.to_nbt_with(WrittenBookPagePredicate::to_nbt_value),
            );
        }
        if let Some(author) = &self.author {
            compound.insert("author", author.as_str());
        }
        if let Some(title) = &self.title {
            compound.insert("title", title.as_str());
        }
        if !self.generation.is_any() {
            compound.insert("generation", self.generation.as_nbt_tag());
        }
        if let Some(resolved) = self.resolved {
            compound.insert("resolved", resolved);
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for WrittenBookPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(pages) = &self.pages {
            let mut value_hasher = ComponentHasher::new();
            pages.hash_with(&mut value_hasher, HashComponent::compute_hash);
            super::item_predicate::push_prehashed_entry(&mut entries, "pages", value_hasher);
        }
        if let Some(author) = &self.author {
            push_hash_entry(&mut entries, "author", author);
        }
        if let Some(title) = &self.title {
            push_hash_entry(&mut entries, "title", title);
        }
        if !self.generation.is_any() {
            push_hash_entry(&mut entries, "generation", &self.generation);
        }
        if let Some(resolved) = self.resolved {
            push_hash_entry(&mut entries, "resolved", &resolved);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(
    WrittenBookPredicate,
    "steel:data_component_predicate/written_book_content"
);

/// Predicate for one attribute-modifier entry.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeModifierEntryPredicate {
    attribute: Option<RegistryHolderSet<Attribute>>,
    id: Option<Identifier>,
    amount: DoubleBounds,
    operation: Option<AttributeModifierOperation>,
    slot: Option<EquipmentSlotGroup>,
}

impl AttributeModifierEntryPredicate {
    #[must_use]
    pub const fn new(
        attribute: Option<RegistryHolderSet<Attribute>>,
        id: Option<Identifier>,
        amount: DoubleBounds,
        operation: Option<AttributeModifierOperation>,
        slot: Option<EquipmentSlotGroup>,
    ) -> Self {
        Self {
            attribute,
            id,
            amount,
            operation,
            slot,
        }
    }

    #[must_use]
    pub const fn attribute(&self) -> Option<&RegistryHolderSet<Attribute>> {
        self.attribute.as_ref()
    }

    #[must_use]
    pub const fn id(&self) -> Option<&Identifier> {
        self.id.as_ref()
    }

    #[must_use]
    pub const fn amount(&self) -> DoubleBounds {
        self.amount
    }

    #[must_use]
    pub const fn operation(&self) -> Option<AttributeModifierOperation> {
        self.operation
    }

    #[must_use]
    pub const fn slot(&self) -> Option<EquipmentSlotGroup> {
        self.slot
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            attribute: decode_optional(compound, "attribute", RegistryHolderSet::from_owned_nbt)?,
            id: decode_optional(compound, "id", |tag| owned_string(tag)?.parse().ok())?,
            amount: compound
                .get("amount")
                .map_or(Some(DoubleBounds::ANY), DoubleBounds::from_owned_nbt)?,
            operation: decode_optional(compound, "operation", |tag| {
                AttributeModifierOperation::by_name(&owned_string(tag)?)
            })?,
            slot: decode_optional(compound, "slot", |tag| {
                EquipmentSlotGroup::by_name(&owned_string(tag)?)
            })?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(attribute) = &self.attribute {
            compound.insert("attribute", attribute.clone().to_nbt_tag());
        }
        if let Some(id) = &self.id {
            compound.insert("id", id.to_string());
        }
        if !self.amount.is_any() {
            compound.insert("amount", self.amount.as_nbt_tag());
        }
        if let Some(operation) = self.operation {
            compound.insert("operation", operation.name());
        }
        if let Some(slot) = self.slot {
            compound.insert("slot", slot.name());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for AttributeModifierEntryPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(attribute) = &self.attribute {
            push_hash_entry(&mut entries, "attribute", attribute);
        }
        if let Some(id) = &self.id {
            push_hash_entry(&mut entries, "id", id);
        }
        if !self.amount.is_any() {
            push_hash_entry(&mut entries, "amount", &self.amount);
        }
        if let Some(operation) = self.operation {
            push_hash_entry(&mut entries, "operation", operation.name());
        }
        if let Some(slot) = self.slot {
            push_hash_entry(&mut entries, "slot", slot.name());
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Predicate over item attribute modifiers.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeModifiersPredicate(
    Option<CollectionPredicate<AttributeModifierEntryPredicate>>,
);

impl AttributeModifiersPredicate {
    #[must_use]
    pub const fn new(
        modifiers: Option<CollectionPredicate<AttributeModifierEntryPredicate>>,
    ) -> Self {
        Self(modifiers)
    }

    #[must_use]
    pub const fn modifiers(&self) -> Option<&CollectionPredicate<AttributeModifierEntryPredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for AttributeModifiersPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "modifiers", |tag| {
            CollectionPredicate::from_nbt_with(tag, AttributeModifierEntryPredicate::from_nbt_value)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(
            self.0.as_ref(),
            "modifiers",
            AttributeModifierEntryPredicate::to_nbt_value,
        )
    }
}

impl HashComponent for AttributeModifiersPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "modifiers",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    AttributeModifiersPredicate,
    "steel:data_component_predicate/attribute_modifiers"
);

/// Predicate over armor trim material and pattern holders.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimPredicate {
    material: Option<RegistryHolderSet<TrimMaterial>>,
    pattern: Option<RegistryHolderSet<TrimPattern>>,
}

impl TrimPredicate {
    #[must_use]
    pub const fn new(
        material: Option<RegistryHolderSet<TrimMaterial>>,
        pattern: Option<RegistryHolderSet<TrimPattern>>,
    ) -> Self {
        Self { material, pattern }
    }

    #[must_use]
    pub const fn material(&self) -> Option<&RegistryHolderSet<TrimMaterial>> {
        self.material.as_ref()
    }

    #[must_use]
    pub const fn pattern(&self) -> Option<&RegistryHolderSet<TrimPattern>> {
        self.pattern.as_ref()
    }
}

impl DataComponentPredicateCodec for TrimPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            material: decode_optional(compound, "material", RegistryHolderSet::from_owned_nbt)?,
            pattern: decode_optional(compound, "pattern", RegistryHolderSet::from_owned_nbt)?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(material) = &self.material {
            compound.insert("material", material.clone().to_nbt_tag());
        }
        if let Some(pattern) = &self.pattern {
            compound.insert("pattern", pattern.clone().to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for TrimPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(material) = &self.material {
            push_hash_entry(&mut entries, "material", material);
        }
        if let Some(pattern) = &self.pattern {
            push_hash_entry(&mut entries, "pattern", pattern);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(TrimPredicate, "steel:data_component_predicate/trim");

/// Predicate over a jukebox-playable song holder.
#[derive(Debug, Clone, PartialEq)]
pub struct JukeboxPlayablePredicate(Option<RegistryHolderSet<JukeboxSong>>);

impl JukeboxPlayablePredicate {
    #[must_use]
    pub const fn new(song: Option<RegistryHolderSet<JukeboxSong>>) -> Self {
        Self(song)
    }

    #[must_use]
    pub const fn song(&self) -> Option<&RegistryHolderSet<JukeboxSong>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for JukeboxPlayablePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "song", RegistryHolderSet::from_owned_nbt).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(song) = &self.0 {
            compound.insert("song", song.clone().to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for JukeboxPlayablePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(song) = &self.0 {
            push_hash_entry(&mut entries, "song", song);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(
    JukeboxPlayablePredicate,
    "steel:data_component_predicate/jukebox_playable"
);

/// Predicate over registered villager variants.
#[derive(Debug, Clone, PartialEq)]
pub struct VillagerTypePredicate(RegistryHolderSet<VillagerType>);

impl VillagerTypePredicate {
    #[must_use]
    pub const fn new(villager_types: RegistryHolderSet<VillagerType>) -> Self {
        Self(villager_types)
    }

    #[must_use]
    pub const fn villager_types(&self) -> &RegistryHolderSet<VillagerType> {
        &self.0
    }
}

impl DataComponentPredicateCodec for VillagerTypePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        RegistryHolderSet::from_owned_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.clone().to_nbt_tag()
    }
}

impl HashComponent for VillagerTypePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    VillagerTypePredicate,
    "steel:data_component_predicate/villager_variant"
);

/// Vanilla component-predicate types in protocol registry order.
pub mod vanilla_data_component_predicate_types {
    use super::{
        AttributeModifiersPredicate, BundlePredicate, ContainerPredicate, CustomDataPredicate,
        DamagePredicate, DataComponentPredicateType, DataComponentPredicateTypeRegistry,
        EnchantmentsPredicate, FireworkExplosionPredicate, FireworksPredicate,
        JukeboxPlayablePredicate, PotionsPredicate, StoredEnchantmentsPredicate, TrimPredicate,
        VillagerTypePredicate, WritableBookPredicate, WrittenBookPredicate,
    };
    use steel_utils::Identifier;

    pub static DAMAGE: DataComponentPredicateType =
        DataComponentPredicateType::of::<DamagePredicate>(Identifier::vanilla_static("damage"));
    pub static ENCHANTMENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<EnchantmentsPredicate>(Identifier::vanilla_static(
            "enchantments",
        ));
    pub static STORED_ENCHANTMENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<StoredEnchantmentsPredicate>(Identifier::vanilla_static(
            "stored_enchantments",
        ));
    pub static POTION_CONTENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<PotionsPredicate>(Identifier::vanilla_static(
            "potion_contents",
        ));
    pub static CUSTOM_DATA: DataComponentPredicateType =
        DataComponentPredicateType::of::<CustomDataPredicate>(Identifier::vanilla_static(
            "custom_data",
        ));
    pub static CONTAINER: DataComponentPredicateType =
        DataComponentPredicateType::of::<ContainerPredicate>(Identifier::vanilla_static(
            "container",
        ));
    pub static BUNDLE_CONTENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<BundlePredicate>(Identifier::vanilla_static(
            "bundle_contents",
        ));
    pub static FIREWORK_EXPLOSION: DataComponentPredicateType =
        DataComponentPredicateType::of::<FireworkExplosionPredicate>(Identifier::vanilla_static(
            "firework_explosion",
        ));
    pub static FIREWORKS: DataComponentPredicateType =
        DataComponentPredicateType::of::<FireworksPredicate>(Identifier::vanilla_static(
            "fireworks",
        ));
    pub static WRITABLE_BOOK_CONTENT: DataComponentPredicateType =
        DataComponentPredicateType::of::<WritableBookPredicate>(Identifier::vanilla_static(
            "writable_book_content",
        ));
    pub static WRITTEN_BOOK_CONTENT: DataComponentPredicateType =
        DataComponentPredicateType::of::<WrittenBookPredicate>(Identifier::vanilla_static(
            "written_book_content",
        ));
    pub static ATTRIBUTE_MODIFIERS: DataComponentPredicateType =
        DataComponentPredicateType::of::<AttributeModifiersPredicate>(Identifier::vanilla_static(
            "attribute_modifiers",
        ));
    pub static TRIM: DataComponentPredicateType =
        DataComponentPredicateType::of::<TrimPredicate>(Identifier::vanilla_static("trim"));
    pub static JUKEBOX_PLAYABLE: DataComponentPredicateType =
        DataComponentPredicateType::of::<JukeboxPlayablePredicate>(Identifier::vanilla_static(
            "jukebox_playable",
        ));
    pub static VILLAGER_VARIANT: DataComponentPredicateType =
        DataComponentPredicateType::of::<VillagerTypePredicate>(Identifier::vanilla_static(
            "villager/variant",
        ));

    pub fn register_data_component_predicate_types(
        registry: &mut DataComponentPredicateTypeRegistry,
    ) {
        registry.register(&DAMAGE);
        registry.register(&ENCHANTMENTS);
        registry.register(&STORED_ENCHANTMENTS);
        registry.register(&POTION_CONTENTS);
        registry.register(&CUSTOM_DATA);
        registry.register(&CONTAINER);
        registry.register(&BUNDLE_CONTENTS);
        registry.register(&FIREWORK_EXPLOSION);
        registry.register(&FIREWORKS);
        registry.register(&WRITABLE_BOOK_CONTENT);
        registry.register(&WRITTEN_BOOK_CONTENT);
        registry.register(&ATTRIBUTE_MODIFIERS);
        registry.register(&TRIM);
        registry.register(&JUKEBOX_PLAYABLE);
        registry.register(&VILLAGER_VARIANT);
    }
}

fn decode_list<T>(tag: &NbtTag, decode: impl Fn(&NbtTag) -> Option<T>) -> Option<Vec<T>> {
    tag.list()?.as_nbt_tags().iter().map(decode).collect()
}

fn encode_list<T>(values: &[T], encode: impl Fn(&T) -> NbtTag) -> NbtTag {
    NbtTag::List(NbtList::from(values.iter().map(encode).collect::<Vec<_>>()))
}

fn hash_list_with<T>(values: &[T], hasher: &mut ComponentHasher, hash: impl Fn(&T) -> i32) {
    hasher.start_list();
    for value in values {
        hasher.put_raw_bytes(&(hash(value) as u32).to_le_bytes());
    }
    hasher.end_list();
}

fn collection_field_nbt<P>(
    collection: Option<&CollectionPredicate<P>>,
    name: &str,
    encode: impl Fn(&P) -> NbtTag + Copy,
) -> NbtTag {
    let mut compound = NbtCompound::new();
    if let Some(collection) = collection {
        compound.insert(name, collection.to_nbt_with(encode));
    }
    NbtTag::Compound(compound)
}

fn hash_optional_collection_field<P>(
    collection: Option<&CollectionPredicate<P>>,
    name: &str,
    hasher: &mut ComponentHasher,
    hash: impl Fn(&P) -> i32 + Copy,
) {
    let mut entries = Vec::new();
    if let Some(collection) = collection {
        let mut value_hasher = ComponentHasher::new();
        collection.hash_with(&mut value_hasher, hash);
        super::item_predicate::push_prehashed_entry(&mut entries, name, value_hasher);
    }
    hash_entries(hasher, &mut entries);
}

fn hash_nbt_codec<T: DataComponentPredicateCodec>(value: &T, hasher: &mut ComponentHasher) {
    value.to_nbt_value().hash_component(hasher);
}

fn owned_string(tag: &NbtTag) -> Option<String> {
    tag.string()?.to_owned().try_into_string().ok()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::data_components::components::OminousBottleAmplifier;
    use crate::data_components::vanilla_components::{
        CAN_BREAK, DAMAGE, LOCK, OMINOUS_BOTTLE_AMPLIFIER,
    };
    use crate::data_components::{ComponentData, DataComponentMap};
    use crate::item_predicate::{AdventureModePredicate, BlockPredicate, LockCode};
    use crate::test_support::init_test_registry;
    use crate::{RegistryHolderSet, vanilla_blocks, vanilla_items};
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};

    #[test]
    fn vanilla_predicate_types_follow_registry_order() {
        init_test_registry();
        let expected = [
            "damage",
            "enchantments",
            "stored_enchantments",
            "potion_contents",
            "custom_data",
            "container",
            "bundle_contents",
            "firework_explosion",
            "fireworks",
            "writable_book_content",
            "written_book_content",
            "attribute_modifiers",
            "trim",
            "jukebox_playable",
            "villager/variant",
        ];

        assert_eq!(
            REGISTRY.data_component_predicate_types.len(),
            expected.len()
        );
        for (id, path) in expected.into_iter().enumerate() {
            assert_eq!(
                REGISTRY
                    .data_component_predicate_types
                    .by_id(id)
                    .map(|entry| entry.key.clone()),
                Some(Identifier::vanilla_static(path))
            );
        }
    }

    #[test]
    fn every_builtin_predicate_payload_round_trips_persistence_and_network() {
        init_test_registry();

        let mut twinkle = NbtCompound::new();
        twinkle.insert("has_twinkle", true);
        let samples = [
            ("damage", NbtTag::Compound(NbtCompound::new())),
            ("enchantments", NbtTag::List(NbtList::Compound(Vec::new()))),
            (
                "stored_enchantments",
                NbtTag::List(NbtList::Compound(Vec::new())),
            ),
            ("potion_contents", NbtTag::String("minecraft:water".into())),
            ("custom_data", NbtTag::String("{}".into())),
            ("container", NbtTag::Compound(NbtCompound::new())),
            ("bundle_contents", NbtTag::Compound(NbtCompound::new())),
            ("firework_explosion", NbtTag::Compound(twinkle)),
            ("fireworks", NbtTag::Compound(NbtCompound::new())),
            (
                "writable_book_content",
                NbtTag::Compound(NbtCompound::new()),
            ),
            ("written_book_content", NbtTag::Compound(NbtCompound::new())),
            ("attribute_modifiers", NbtTag::Compound(NbtCompound::new())),
            ("trim", NbtTag::Compound(NbtCompound::new())),
            ("jukebox_playable", NbtTag::Compound(NbtCompound::new())),
            (
                "villager/variant",
                NbtTag::String("minecraft:plains".into()),
            ),
        ];

        let predicates = samples
            .into_iter()
            .map(|(path, tag)| {
                DataComponentPredicateData::from_persistent_entry(
                    &Identifier::vanilla_static(path),
                    &tag,
                )
                .unwrap_or_else(|| panic!("{path} sample should decode"))
            })
            .collect::<Vec<_>>();
        let matchers = DataComponentMatchers::new(DataComponentExactPredicate::EMPTY, predicates)
            .expect("builtin predicate keys are unique");

        let mut fields = NbtCompound::new();
        matchers.write_fields(&mut fields);
        assert_eq!(
            DataComponentMatchers::from_fields(&fields),
            Some(matchers.clone())
        );

        let mut encoded = Vec::new();
        matchers
            .write(&mut encoded)
            .expect("predicate matcher network codec should encode");
        assert_eq!(
            DataComponentMatchers::read(&mut Cursor::new(encoded.as_slice()))
                .expect("predicate matcher network codec should decode"),
            matchers
        );
    }

    #[test]
    fn adventure_and_lock_components_round_trip_both_codecs() {
        init_test_registry();

        let block = BlockPredicate::new(
            Some(RegistryHolderSet::Direct(vec![&vanilla_blocks::STONE])),
            None,
            None,
            DataComponentMatchers::ANY,
        );
        let adventure =
            AdventureModePredicate::new(vec![block]).expect("one block predicate is persistable");
        round_trip_component(CAN_BREAK.key, ComponentData::new(adventure));

        let mut exact_components = DataComponentMap::new();
        exact_components.set(DAMAGE, Some(3));
        let damage_type = REGISTRY
            .data_component_predicate_types
            .by_key(&Identifier::vanilla_static("damage"))
            .expect("damage predicate type should exist");
        let partial = DataComponentPredicateData::new(
            damage_type,
            DamagePredicate::new(IntBounds::ANY, IntBounds::exactly(3)),
        );
        let matchers = DataComponentMatchers::new(
            DataComponentExactPredicate::all_of(&exact_components)
                .expect("exact components should persist"),
            vec![partial],
        )
        .expect("exact and partial maps use separate namespaces");
        let item = ItemPredicate::new(
            Some(RegistryHolderSet::Direct(vec![&vanilla_items::STONE])),
            IntBounds::exactly(1),
            matchers,
        );
        round_trip_component(LOCK.key, ComponentData::new(LockCode::new(item)));
    }

    #[test]
    fn exact_predicates_reject_component_values_that_cannot_persist() {
        init_test_registry();
        let entry = REGISTRY
            .data_components
            .by_key(&OMINOUS_BOTTLE_AMPLIFIER.key)
            .expect("ominous bottle amplifier should be registered");
        let value = ComponentData::new(OminousBottleAmplifier::new(5));

        assert!(DataComponentExactPredicate::new(vec![(entry, value.clone())]).is_none());

        let mut network = Vec::new();
        write_len(1, &mut network).expect("one exact predicate should encode");
        write_registry_id(entry, &mut network, "data component")
            .expect("registered component ID should encode");
        entry
            .write_network(&value, &mut network)
            .expect("Vanilla's stream codec accepts amplifier 5");
        assert!(
            DataComponentExactPredicate::read(&mut Cursor::new(network.as_slice())).is_err(),
            "nested network values must not create a lock that fails persistent encoding"
        );
    }

    #[test]
    fn boolean_predicate_fields_hash_as_codec_booleans() {
        let predicate = FireworkPredicate {
            shape: None,
            has_twinkle: Some(true),
            has_trail: None,
        };
        let mut key_hasher = ComponentHasher::new();
        "has_twinkle".hash_component(&mut key_hasher);
        let mut value_hasher = ComponentHasher::new();
        true.hash_component(&mut value_hasher);
        let mut entries = vec![HashEntry::new(key_hasher, value_hasher)];
        let mut expected = ComponentHasher::new();
        hash_entries(&mut expected, &mut entries);

        assert_eq!(predicate.compute_hash(), expected.finish());
        assert_ne!(
            predicate.compute_hash(),
            predicate.to_nbt_value().compute_hash(),
            "Codec.BOOL and an NBT byte intentionally have different HashOps tags"
        );
    }

    fn round_trip_component(key: Identifier, value: ComponentData) {
        let entry = REGISTRY
            .data_components
            .by_key(&key)
            .unwrap_or_else(|| panic!("missing component {key}"));
        let tag = entry
            .write_nbt(&value)
            .unwrap_or_else(|error| panic!("{key} persistent encode failed: {error}"));
        assert_eq!(entry.read_nbt_owned(&tag), Some(value.clone()));

        let mut encoded = Vec::new();
        entry
            .write_network(&value, &mut encoded)
            .unwrap_or_else(|error| panic!("{key} network encode failed: {error}"));
        assert_eq!(
            entry
                .read_network(&mut Cursor::new(encoded.as_slice()))
                .unwrap_or_else(|error| panic!("{key} network decode failed: {error}")),
            value
        );
        assert_eq!(
            entry
                .compute_hash(&value)
                .unwrap_or_else(|error| panic!("{key} hash failed: {error}")),
            value.downcast_ref::<AdventureModePredicate>().map_or_else(
                || {
                    value
                        .downcast_ref::<LockCode>()
                        .expect("test only uses adventure and lock values")
                        .compute_hash()
                },
                HashComponent::compute_hash,
            )
        );
    }
}
