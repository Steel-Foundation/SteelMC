//! Vanilla item, block, state, and NBT predicate codec values.

use std::fmt::Debug;
use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag, read_tag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::{
    NbtNumeric, normalize_nbt_compound, parse_snbt_compound, to_canonical_snbt,
    vanilla_nbt_heap_size,
};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryHolderSet;
use crate::blocks::Block;
use crate::data_component_predicate::DataComponentMatchers;
use crate::items::Item;

const DEFAULT_NBT_QUOTA: u64 = 2_097_152;
const MAX_UTF_LENGTH: usize = 32_767;

/// Vanilla integer min/max bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntBounds {
    min: Option<i32>,
    max: Option<i32>,
}

impl IntBounds {
    pub const ANY: Self = Self {
        min: None,
        max: None,
    };

    #[must_use]
    pub fn new(min: Option<i32>, max: Option<i32>) -> Option<Self> {
        if min.zip(max).is_some_and(|(min, max)| min > max) {
            return None;
        }
        Some(Self { min, max })
    }

    #[must_use]
    pub const fn exactly(value: i32) -> Self {
        Self {
            min: Some(value),
            max: Some(value),
        }
    }

    #[must_use]
    pub const fn min(&self) -> Option<i32> {
        self.min
    }

    #[must_use]
    pub const fn max(&self) -> Option<i32> {
        self.max
    }

    #[must_use]
    pub const fn is_any(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if let Some(value) = tag.codec_i32() {
            return Some(Self::exactly(value));
        }
        let compound = tag.compound()?;
        Self::new(
            decode_optional(compound, "min", NbtNumeric::codec_i32)?,
            decode_optional(compound, "max", NbtNumeric::codec_i32)?,
        )
    }

    pub(crate) fn as_nbt_tag(&self) -> NbtTag {
        if let (Some(min), Some(max)) = (self.min, self.max)
            && min == max
        {
            return NbtTag::Int(min);
        }
        let mut compound = NbtCompound::new();
        if let Some(min) = self.min {
            compound.insert("min", min);
        }
        if let Some(max) = self.max {
            compound.insert("max", max);
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for IntBounds {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.as_nbt_tag().hash_component(hasher);
    }
}

/// Vanilla double min/max bounds.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoubleBounds {
    min: Option<f64>,
    max: Option<f64>,
}

impl DoubleBounds {
    pub const ANY: Self = Self {
        min: None,
        max: None,
    };

    #[must_use]
    pub fn new(min: Option<f64>, max: Option<f64>) -> Option<Self> {
        if min
            .zip(max)
            .is_some_and(|(min, max)| java_double_compare(min, max).is_gt())
        {
            return None;
        }
        Some(Self { min, max })
    }

    #[must_use]
    pub const fn exactly(value: f64) -> Self {
        Self {
            min: Some(value),
            max: Some(value),
        }
    }

    #[must_use]
    pub const fn min(&self) -> Option<f64> {
        self.min
    }

    #[must_use]
    pub const fn max(&self) -> Option<f64> {
        self.max
    }

    #[must_use]
    pub const fn is_any(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if let Some(value) = tag.codec_f64() {
            return Some(Self::exactly(value));
        }
        let compound = tag.compound()?;
        Self::new(
            decode_optional(compound, "min", NbtNumeric::codec_f64)?,
            decode_optional(compound, "max", NbtNumeric::codec_f64)?,
        )
    }

    pub(crate) fn as_nbt_tag(&self) -> NbtTag {
        if let (Some(min), Some(max)) = (self.min, self.max)
            && java_double_equals(min, max)
        {
            return NbtTag::Double(min);
        }
        let mut compound = NbtCompound::new();
        if let Some(min) = self.min {
            compound.insert("min", min);
        }
        if let Some(max) = self.max {
            compound.insert("max", max);
        }
        NbtTag::Compound(compound)
    }
}

impl PartialEq for DoubleBounds {
    fn eq(&self, other: &Self) -> bool {
        option_double_equals(self.min, other.min) && option_double_equals(self.max, other.max)
    }
}

impl HashComponent for DoubleBounds {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        if let (Some(min), Some(max)) = (self.min, self.max)
            && java_double_equals(min, max)
        {
            hasher.put_double(min);
            return;
        }
        let mut entries = Vec::new();
        if let Some(min) = self.min {
            push_hash_entry(&mut entries, "min", &min);
        }
        if let Some(max) = self.max {
            push_hash_entry(&mut entries, "max", &max);
        }
        hash_entries(hasher, &mut entries);
    }
}

const fn option_double_equals(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => java_double_equals(left, right),
        (None, None) => true,
        _ => false,
    }
}

const fn java_double_equals(left: f64, right: f64) -> bool {
    java_double_bits(left) == java_double_bits(right)
}

fn java_double_compare(left: f64, right: f64) -> std::cmp::Ordering {
    if left < right {
        return std::cmp::Ordering::Less;
    }
    if left > right {
        return std::cmp::Ordering::Greater;
    }
    java_double_bits(left).cmp(&java_double_bits(right))
}

const fn java_double_bits(value: f64) -> i64 {
    if value.is_nan() {
        return 0x7ff8_0000_0000_0000;
    }
    i64::from_ne_bytes(value.to_bits().to_ne_bytes())
}

/// One exact or ranged block-state property matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatePropertyValueMatcher {
    Exact(String),
    Range {
        min: Option<String>,
        max: Option<String>,
    },
}

impl StatePropertyValueMatcher {
    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if let Some(value) = tag.string() {
            return Some(Self::Exact(value.to_string()));
        }
        let compound = tag.compound()?;
        Some(Self::Range {
            min: optional_string(compound, "min")?,
            max: optional_string(compound, "max")?,
        })
    }

    pub(crate) fn to_nbt_tag_ref(&self) -> NbtTag {
        match self {
            Self::Exact(value) => NbtTag::String(value.clone().into()),
            Self::Range { min, max } => {
                let mut compound = NbtCompound::new();
                if let Some(min) = min {
                    compound.insert("min", min.as_str());
                }
                if let Some(max) = max {
                    compound.insert("max", max.as_str());
                }
                NbtTag::Compound(compound)
            }
        }
    }

    fn write_network(&self, writer: &mut impl Write) -> Result<()> {
        match self {
            Self::Exact(value) => {
                true.write(writer)?;
                write_utf(value, writer)
            }
            Self::Range { min, max } => {
                false.write(writer)?;
                write_optional_utf(min.as_deref(), writer)?;
                write_optional_utf(max.as_deref(), writer)
            }
        }
    }

    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        if bool::read(data)? {
            Ok(Self::Exact(read_utf(data)?))
        } else {
            Ok(Self::Range {
                min: read_optional_utf(data)?,
                max: read_optional_utf(data)?,
            })
        }
    }
}

impl HashComponent for StatePropertyValueMatcher {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.to_nbt_tag_ref().hash_component(hasher);
    }
}

/// A named state property and its value matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatePropertyMatcher {
    name: String,
    value: StatePropertyValueMatcher,
}

impl StatePropertyMatcher {
    #[must_use]
    pub const fn new(name: String, value: StatePropertyValueMatcher) -> Self {
        Self { name, value }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn value(&self) -> &StatePropertyValueMatcher {
        &self.value
    }
}

/// Vanilla state-properties predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatePropertiesPredicate {
    properties: Vec<StatePropertyMatcher>,
}

impl StatePropertiesPredicate {
    #[must_use]
    pub fn new(properties: Vec<StatePropertyMatcher>) -> Option<Self> {
        let mut names = rustc_hash::FxHashSet::default();
        properties
            .iter()
            .all(|property| names.insert(property.name.clone()))
            .then_some(Self { properties })
    }

    #[must_use]
    pub fn properties(&self) -> &[StatePropertyMatcher] {
        &self.properties
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let mut properties = Vec::with_capacity(compound.len());
        for (name, value) in compound.iter() {
            properties.push(StatePropertyMatcher::new(
                name.to_owned().try_into_string().ok()?,
                StatePropertyValueMatcher::from_owned_nbt(value)?,
            ));
        }
        Self::new(properties)
    }

    pub(crate) fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        for property in &self.properties {
            compound.insert(property.name.clone(), property.value.to_nbt_tag_ref());
        }
        NbtTag::Compound(compound)
    }
}

impl WriteTo for StatePropertiesPredicate {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_len(self.properties.len(), writer)?;
        for property in &self.properties {
            write_utf(&property.name, writer)?;
            property.value.write_network(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for StatePropertiesPredicate {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_len(data, usize::MAX, "state property")?;
        let mut properties = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            properties.push(StatePropertyMatcher::new(
                read_utf(data)?,
                StatePropertyValueMatcher::read_network(data)?,
            ));
        }
        Self::new(properties).ok_or_else(|| Error::other("duplicate state property matcher"))
    }
}

impl HashComponent for StatePropertiesPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.to_nbt_tag_ref().hash_component(hasher);
    }
}

/// Vanilla partial-NBT predicate.
#[derive(Debug, Clone)]
pub struct NbtPredicate {
    tag: NbtCompound,
}

impl NbtPredicate {
    #[must_use]
    pub fn new(tag: NbtCompound) -> Option<Self> {
        normalize_nbt_compound(tag).map(|tag| Self { tag })
    }

    #[must_use]
    pub const fn tag(&self) -> &NbtCompound {
        &self.tag
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        match tag {
            NbtTag::String(value) => {
                let value = value.to_owned().try_into_string().ok()?;
                Self::new(parse_snbt_compound(&value).ok()?)
            }
            NbtTag::Compound(compound) => Self::new(compound.clone()),
            _ => None,
        }
    }

    pub(crate) fn to_nbt_tag_ref(&self) -> NbtTag {
        let Some(snbt) = to_canonical_snbt(&NbtTag::Compound(self.tag.clone())) else {
            panic!("normalized NBT predicate became malformed");
        };
        NbtTag::String(snbt.into())
    }
}

impl PartialEq for NbtPredicate {
    fn eq(&self, other: &Self) -> bool {
        steel_utils::nbt::nbt_compounds_equal(&self.tag, &other.tag)
    }
}

impl WriteTo for NbtPredicate {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut encoded = Vec::new();
        NbtTag::Compound(self.tag.clone()).write(&mut encoded);
        writer.write_all(&encoded)
    }
}

impl ReadFrom for NbtPredicate {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let NbtTag::Compound(compound) = read_network_nbt(data)? else {
            return Err(Error::other(
                "NBT predicate network value is not a compound",
            ));
        };
        Self::new(compound).ok_or_else(|| Error::other("NBT predicate contains malformed UTF-8"))
    }
}

impl HashComponent for NbtPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let NbtTag::String(value) = self.to_nbt_tag_ref() else {
            unreachable!("NBT predicate codec always writes a string");
        };
        hasher.put_string(&value.to_string());
    }
}

/// Vanilla block predicate used by adventure-mode item components.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockPredicate {
    blocks: Option<RegistryHolderSet<Block>>,
    state: Option<StatePropertiesPredicate>,
    nbt: Option<NbtPredicate>,
    components: DataComponentMatchers,
}

impl BlockPredicate {
    #[must_use]
    pub const fn new(
        blocks: Option<RegistryHolderSet<Block>>,
        state: Option<StatePropertiesPredicate>,
        nbt: Option<NbtPredicate>,
        components: DataComponentMatchers,
    ) -> Self {
        Self {
            blocks,
            state,
            nbt,
            components,
        }
    }

    #[must_use]
    pub const fn blocks(&self) -> Option<&RegistryHolderSet<Block>> {
        self.blocks.as_ref()
    }

    #[must_use]
    pub const fn state(&self) -> Option<&StatePropertiesPredicate> {
        self.state.as_ref()
    }

    #[must_use]
    pub const fn nbt(&self) -> Option<&NbtPredicate> {
        self.nbt.as_ref()
    }

    #[must_use]
    pub const fn components(&self) -> &DataComponentMatchers {
        &self.components
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode_optional(compound, "blocks", RegistryHolderSet::from_owned_nbt)?,
            decode_optional(compound, "state", StatePropertiesPredicate::from_owned_nbt)?,
            decode_optional(compound, "nbt", NbtPredicate::from_owned_nbt)?,
            DataComponentMatchers::from_fields(compound)?,
        ))
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(blocks) = &self.blocks {
            compound.insert("blocks", blocks.clone().to_nbt_tag());
        }
        if let Some(state) = &self.state {
            compound.insert("state", state.to_nbt_tag_ref());
        }
        if let Some(nbt) = &self.nbt {
            compound.insert("nbt", nbt.to_nbt_tag_ref());
        }
        self.components.write_fields(&mut compound);
        NbtTag::Compound(compound)
    }
}

impl WriteTo for BlockPredicate {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.blocks.write(writer)?;
        self.state.write(writer)?;
        self.nbt.write(writer)?;
        self.components.write(writer)
    }
}

impl ReadFrom for BlockPredicate {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            Option::<RegistryHolderSet<Block>>::read(data)?,
            Option::<StatePropertiesPredicate>::read(data)?,
            Option::<NbtPredicate>::read(data)?,
            DataComponentMatchers::read(data)?,
        ))
    }
}

impl HashComponent for BlockPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(blocks) = &self.blocks {
            push_hash_entry(&mut entries, "blocks", blocks);
        }
        if let Some(state) = &self.state {
            push_hash_entry(&mut entries, "state", state);
        }
        if let Some(nbt) = &self.nbt {
            push_hash_entry(&mut entries, "nbt", nbt);
        }
        self.components.hash_fields(&mut entries);
        hash_entries(hasher, &mut entries);
    }
}

/// Non-empty list of block predicates used by `can_break` and `can_place_on`.
#[derive(Debug, Clone, PartialEq)]
pub struct AdventureModePredicate {
    predicates: Vec<BlockPredicate>,
}

impl AdventureModePredicate {
    #[must_use]
    pub fn new(predicates: Vec<BlockPredicate>) -> Option<Self> {
        (!predicates.is_empty()).then_some(Self { predicates })
    }

    #[must_use]
    pub fn predicates(&self) -> &[BlockPredicate] {
        &self.predicates
    }
}

impl WriteTo for AdventureModePredicate {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_len(self.predicates.len(), writer)?;
        for predicate in &self.predicates {
            predicate.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for AdventureModePredicate {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_len(data, usize::MAX, "adventure-mode predicate")?;
        let mut predicates = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            predicates.push(BlockPredicate::read(data)?);
        }
        Self::new(predicates)
            .ok_or_else(|| Error::other("adventure-mode predicate list cannot be empty"))
    }
}

impl ToNbtTag for AdventureModePredicate {
    fn to_nbt_tag(self) -> NbtTag {
        if self.predicates.len() == 1 {
            return self.predicates[0].to_nbt_tag_ref();
        }
        NbtTag::List(NbtList::Compound(
            self.predicates
                .iter()
                .map(|predicate| {
                    let NbtTag::Compound(compound) = predicate.to_nbt_tag_ref() else {
                        unreachable!("block predicate codec always writes a compound");
                    };
                    compound
                })
                .collect(),
        ))
    }
}

impl FromNbtTag for AdventureModePredicate {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let tag = tag.to_owned();
        let predicates = if tag.compound().is_some() {
            vec![BlockPredicate::from_owned_nbt(&tag)?]
        } else {
            tag.list()?
                .compounds()?
                .iter()
                .map(|compound| BlockPredicate::from_owned_nbt(&NbtTag::Compound(compound.clone())))
                .collect::<Option<Vec<_>>>()?
        };
        Self::new(predicates)
    }
}

impl HashComponent for AdventureModePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        if self.predicates.len() == 1 {
            self.predicates[0].hash_component(hasher);
            return;
        }
        hasher.start_list();
        for predicate in &self.predicates {
            hasher.put_component_hash(predicate);
        }
        hasher.end_list();
    }
}

/// Vanilla item predicate used by lock codes and nested component predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemPredicate {
    items: Option<RegistryHolderSet<Item>>,
    count: IntBounds,
    components: DataComponentMatchers,
}

impl ItemPredicate {
    #[must_use]
    pub const fn new(
        items: Option<RegistryHolderSet<Item>>,
        count: IntBounds,
        components: DataComponentMatchers,
    ) -> Self {
        Self {
            items,
            count,
            components,
        }
    }

    #[must_use]
    pub const fn any() -> Self {
        Self::new(None, IntBounds::ANY, DataComponentMatchers::ANY)
    }

    #[must_use]
    pub const fn items(&self) -> Option<&RegistryHolderSet<Item>> {
        self.items.as_ref()
    }

    #[must_use]
    pub const fn count(&self) -> IntBounds {
        self.count
    }

    #[must_use]
    pub const fn components(&self) -> &DataComponentMatchers {
        &self.components
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode_optional(compound, "items", RegistryHolderSet::from_owned_nbt)?,
            compound
                .get("count")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
            DataComponentMatchers::from_fields(compound)?,
        ))
    }

    pub(crate) fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(items) = &self.items {
            compound.insert("items", items.clone().to_nbt_tag());
        }
        if !self.count.is_any() {
            compound.insert("count", self.count.as_nbt_tag());
        }
        self.components.write_fields(&mut compound);
        NbtTag::Compound(compound)
    }
}

impl HashComponent for ItemPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(items) = &self.items {
            push_hash_entry(&mut entries, "items", items);
        }
        if !self.count.is_any() {
            push_hash_entry(&mut entries, "count", &self.count);
        }
        self.components.hash_fields(&mut entries);
        hash_entries(hasher, &mut entries);
    }
}

/// Vanilla lock-code component value.
#[derive(Debug, Clone, PartialEq)]
pub struct LockCode {
    predicate: ItemPredicate,
}

impl LockCode {
    pub const NO_LOCK: Self = Self {
        predicate: ItemPredicate::any(),
    };

    #[must_use]
    pub const fn new(predicate: ItemPredicate) -> Self {
        Self { predicate }
    }

    #[must_use]
    pub const fn predicate(&self) -> &ItemPredicate {
        &self.predicate
    }
}

impl WriteTo for LockCode {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut encoded = Vec::new();
        self.predicate.to_nbt_tag_ref().write(&mut encoded);
        writer.write_all(&encoded)
    }
}

impl ReadFrom for LockCode {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let tag = read_network_nbt(data)?;
        ItemPredicate::from_owned_nbt(&tag)
            .map(Self::new)
            .ok_or_else(|| Error::other("invalid lock item predicate"))
    }
}

impl ToNbtTag for LockCode {
    fn to_nbt_tag(self) -> NbtTag {
        self.predicate.to_nbt_tag_ref()
    }
}

impl FromNbtTag for LockCode {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        ItemPredicate::from_owned_nbt(&tag.to_owned()).map(Self::new)
    }
}

impl HashComponent for LockCode {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.predicate.hash_component(hasher);
    }
}

#[expect(
    clippy::option_option,
    reason = "the outer option rejects malformed present fields while the inner option represents absence"
)]
fn optional_string(compound: &NbtCompound, key: &str) -> Option<Option<String>> {
    decode_optional(compound, key, |tag| {
        tag.string()?.to_owned().try_into_string().ok()
    })
}

#[expect(
    clippy::option_option,
    reason = "the outer option rejects malformed present fields while the inner option represents absence"
)]
pub(crate) fn decode_optional<T>(
    compound: &NbtCompound,
    key: &str,
    decode: impl FnOnce(&NbtTag) -> Option<T>,
) -> Option<Option<T>> {
    match compound.get(key) {
        Some(tag) => Some(Some(decode(tag)?)),
        None => Some(None),
    }
}

pub(crate) fn read_network_nbt(data: &mut Cursor<&[u8]>) -> Result<NbtTag> {
    let tag = read_tag(data).map_err(|error| Error::other(format!("invalid NBT: {error:?}")))?;
    let Some(heap_size) = vanilla_nbt_heap_size(&tag) else {
        return Err(Error::other("NBT contains malformed modified UTF-8"));
    };
    if heap_size > DEFAULT_NBT_QUOTA {
        return Err(Error::other(format!(
            "NBT exceeds Vanilla's {DEFAULT_NBT_QUOTA}-byte heap quota"
        )));
    }
    Ok(tag)
}

pub(crate) fn write_len(len: usize, writer: &mut impl Write) -> Result<()> {
    let len = i32::try_from(len).map_err(|_| Error::other("list exceeds protocol range"))?;
    VarInt(len).write(writer)
}

pub(crate) fn read_len(data: &mut Cursor<&[u8]>, max: usize, name: &str) -> Result<usize> {
    let encoded = VarInt::read(data)?.0;
    let len = usize::try_from(encoded)
        .map_err(|_| Error::other(format!("negative {name} count: {encoded}")))?;
    if len > max {
        return Err(Error::other(format!("{name} count {len} exceeds {max}")));
    }
    Ok(len)
}

fn write_utf(value: &str, writer: &mut impl Write) -> Result<()> {
    if value.encode_utf16().count() > MAX_UTF_LENGTH {
        return Err(Error::other("string exceeds Vanilla's UTF-16 length limit"));
    }
    if value.len() > MAX_UTF_LENGTH * 3 {
        return Err(Error::other("string exceeds Vanilla's UTF-8 length limit"));
    }
    write_len(value.len(), writer)?;
    writer.write_all(value.as_bytes())
}

fn read_utf(data: &mut Cursor<&[u8]>) -> Result<String> {
    use std::io::Read as _;

    let len = read_len(data, MAX_UTF_LENGTH * 3, "string byte")?;
    let mut bytes = vec![0; len];
    data.read_exact(&mut bytes)?;
    let value = String::from_utf8(bytes).map_err(Error::other)?;
    if value.encode_utf16().count() > MAX_UTF_LENGTH {
        return Err(Error::other("string exceeds Vanilla's UTF-16 length limit"));
    }
    Ok(value)
}

fn write_optional_utf(value: Option<&str>, writer: &mut impl Write) -> Result<()> {
    value.is_some().write(writer)?;
    if let Some(value) = value {
        write_utf(value, writer)?;
    }
    Ok(())
}

fn read_optional_utf(data: &mut Cursor<&[u8]>) -> Result<Option<String>> {
    bool::read(data)?.then(|| read_utf(data)).transpose()
}

pub(crate) fn push_hash_entry<T: HashComponent + ?Sized>(
    entries: &mut Vec<HashEntry>,
    key: &str,
    value: &T,
) {
    let mut key_hasher = ComponentHasher::new();
    key.hash_component(&mut key_hasher);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

pub(crate) fn push_prehashed_entry(
    entries: &mut Vec<HashEntry>,
    key: &str,
    value_hasher: ComponentHasher,
) {
    let mut key_hasher = ComponentHasher::new();
    key.hash_component(&mut key_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

pub(crate) fn hash_entries(hasher: &mut ComponentHasher, entries: &mut [HashEntry]) {
    sort_map_entries(entries);
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

#[cfg(test)]
mod tests {
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

    use super::{DoubleBounds, NbtPredicate};

    #[test]
    fn double_bounds_use_java_ordering() {
        assert!(DoubleBounds::new(Some(-0.0), Some(0.0)).is_some());
        assert!(DoubleBounds::new(Some(0.0), Some(-0.0)).is_none());
        assert!(DoubleBounds::new(Some(f64::NAN), Some(f64::NAN)).is_some());
        assert!(DoubleBounds::new(Some(1.0), Some(f64::NAN)).is_some());
        assert!(DoubleBounds::new(Some(f64::NAN), Some(1.0)).is_none());
    }

    #[test]
    fn nbt_predicate_persistence_round_trips_heterogeneous_lists() {
        let mut tag = NbtCompound::new();
        tag.insert(
            "values",
            NbtList::from(vec![NbtTag::Int(7), NbtTag::String("value".into())]),
        );
        let predicate = NbtPredicate::new(tag).expect("predicate NBT should normalize");

        let encoded = predicate.to_nbt_tag_ref();
        assert_eq!(encoded, NbtTag::String("{values:[7,\"value\"]}".into()));
        assert_eq!(NbtPredicate::from_owned_nbt(&encoded), Some(predicate));
    }
}
