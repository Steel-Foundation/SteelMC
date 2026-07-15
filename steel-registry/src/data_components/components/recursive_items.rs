//! Item components whose codecs recursively contain item stack templates.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use super::Bees;
use crate::ItemStackTemplate;
use crate::data_components::registry::ValidatePersistentComponent;
use crate::data_components::vanilla_components::{BEES, BUNDLE_CONTENTS};

macro_rules! impl_template_wrapper_codecs {
    ($type:ty, $field:ident) => {
        impl WriteTo for $type {
            fn write(&self, writer: &mut impl Write) -> Result<()> {
                self.$field.write(writer)
            }
        }

        impl ReadFrom for $type {
            fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
                Ok(Self::new(ItemStackTemplate::read(data)?))
            }
        }

        impl ToNbtTag for $type {
            fn to_nbt_tag(self) -> NbtTag {
                self.$field.to_nbt_tag()
            }
        }

        impl FromNbtTag for $type {
            fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
                Some(Self::new(ItemStackTemplate::from_nbt_tag(tag)?))
            }
        }

        impl HashComponent for $type {
            fn hash_component(&self, hasher: &mut ComponentHasher) {
                self.$field.hash_component(hasher);
            }
        }
    };
}

/// The item template produced after consuming an item.
#[derive(Debug, Clone, PartialEq)]
pub struct UseRemainder {
    convert_into: ItemStackTemplate,
}

impl UseRemainder {
    #[must_use]
    pub const fn new(convert_into: ItemStackTemplate) -> Self {
        Self { convert_into }
    }

    #[must_use]
    pub const fn convert_into(&self) -> &ItemStackTemplate {
        &self.convert_into
    }
}

impl_template_wrapper_codecs!(UseRemainder, convert_into);

impl ValidatePersistentComponent for UseRemainder {
    fn validate_persistent(&self) -> Result<()> {
        self.convert_into.validate_persistent_encoding()
    }
}

/// The non-empty projectile templates loaded into a crossbow.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ChargedProjectiles {
    items: Vec<ItemStackTemplate>,
}

impl ChargedProjectiles {
    pub const MAX_SIZE: usize = 1024;

    #[must_use]
    pub const fn empty() -> Self {
        Self { items: Vec::new() }
    }

    pub fn new(items: Vec<ItemStackTemplate>) -> Result<Self> {
        if items.len() > Self::MAX_SIZE {
            return Err(Error::other(format!(
                "Got {} charged projectiles, but maximum is {}",
                items.len(),
                Self::MAX_SIZE
            )));
        }
        Ok(Self { items })
    }

    #[must_use]
    pub fn items(&self) -> &[ItemStackTemplate] {
        &self.items
    }
}

impl WriteTo for ChargedProjectiles {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_template_list(&self.items, Some(Self::MAX_SIZE), writer)
    }
}

impl ReadFrom for ChargedProjectiles {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(read_template_list(data, Some(Self::MAX_SIZE))?)
    }
}

impl ToNbtTag for ChargedProjectiles {
    fn to_nbt_tag(self) -> NbtTag {
        template_list_nbt(&self.items)
    }
}

impl FromNbtTag for ChargedProjectiles {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::new(template_list_from_nbt(tag, Some(Self::MAX_SIZE))?).ok()
    }
}

impl HashComponent for ChargedProjectiles {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_template_list(&self.items, hasher);
    }
}

impl ValidatePersistentComponent for ChargedProjectiles {
    fn validate_persistent(&self) -> Result<()> {
        validate_templates(self.items.iter())
    }
}

/// The ordered item templates stored in a bundle.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BundleContents {
    items: Vec<ItemStackTemplate>,
}

impl BundleContents {
    #[must_use]
    pub const fn empty() -> Self {
        Self { items: Vec::new() }
    }

    #[must_use]
    pub const fn new(items: Vec<ItemStackTemplate>) -> Self {
        Self { items }
    }

    #[must_use]
    pub fn items(&self) -> &[ItemStackTemplate] {
        &self.items
    }

    /// Validates the checked rational weight used by Vanilla's strict item-stack validation.
    pub(crate) fn validate_weight(&self) -> Result<()> {
        self.compute_weight().map(|_| ())
    }

    fn compute_weight(&self) -> Result<CheckedFraction> {
        let mut weight = CheckedFraction::ZERO;
        for item in &self.items {
            let item_weight = bundle_item_weight(item)?.multiply(item.count())?;
            weight = weight.add(item_weight)?;
        }
        Ok(weight)
    }
}

fn bundle_item_weight(item: &ItemStackTemplate) -> Result<CheckedFraction> {
    if let Some(bundle) = item.get(BUNDLE_CONTENTS) {
        return bundle.compute_weight()?.add(CheckedFraction::new(1, 16)?);
    }
    if item
        .get(BEES)
        .is_some_and(|bees: &Bees| !bees.bees().is_empty())
    {
        return Ok(CheckedFraction::ONE);
    }
    CheckedFraction::new(1, item.max_stack_size())
}

/// Positive subset of Commons Lang `Fraction` used by `BundleContents`.
#[derive(Clone, Copy)]
struct CheckedFraction {
    numerator: i32,
    denominator: i32,
}

impl CheckedFraction {
    const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    fn new(numerator: i32, denominator: i32) -> Result<Self> {
        if numerator < 0 || denominator <= 0 {
            return Err(Error::other("Invalid bundle weight fraction"));
        }
        let divisor = gcd(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    /// Mirrors the positive-number branches of Commons Lang `Fraction.addSub`.
    fn add(self, other: Self) -> Result<Self> {
        if self.numerator == 0 {
            return Ok(other);
        }
        if other.numerator == 0 {
            return Ok(self);
        }

        let denominator_gcd = gcd(self.denominator, other.denominator);
        if denominator_gcd == 1 {
            let left = checked_mul(self.numerator, other.denominator)?;
            let right = checked_mul(other.numerator, self.denominator)?;
            return Ok(Self {
                numerator: checked_add(left, right)?,
                denominator: checked_mul(self.denominator, other.denominator)?,
            });
        }

        let left = i64::from(self.numerator) * i64::from(other.denominator / denominator_gcd);
        let right = i64::from(other.numerator) * i64::from(self.denominator / denominator_gcd);
        let sum = left + right;
        let reduction = gcd_i64(sum % i64::from(denominator_gcd), i64::from(denominator_gcd));
        let numerator = i32::try_from(sum / reduction)
            .map_err(|_| Error::other("Excessive total bundle weight"))?;
        let reduction =
            i32::try_from(reduction).map_err(|_| Error::other("Excessive total bundle weight"))?;
        let denominator = checked_mul(
            self.denominator / denominator_gcd,
            other.denominator / reduction,
        )?;
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// Mirrors multiplying by Commons Lang `Fraction.getFraction(value, 1)`.
    fn multiply(self, value: i32) -> Result<Self> {
        if value < 0 {
            return Err(Error::other("Invalid bundle item count"));
        }
        if self.numerator == 0 || value == 0 {
            return Ok(Self::ZERO);
        }
        let reduction = gcd(value, self.denominator);
        Self::new(
            checked_mul(self.numerator, value / reduction)?,
            self.denominator / reduction,
        )
    }
}

const fn gcd(mut left: i32, mut right: i32) -> i32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.abs()
}

const fn gcd_i64(mut left: i64, mut right: i64) -> i64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.abs()
}

fn checked_mul(left: i32, right: i32) -> Result<i32> {
    left.checked_mul(right)
        .ok_or_else(|| Error::other("Excessive total bundle weight"))
}

fn checked_add(left: i32, right: i32) -> Result<i32> {
    left.checked_add(right)
        .ok_or_else(|| Error::other("Excessive total bundle weight"))
}

impl WriteTo for BundleContents {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_template_list(&self.items, None, writer)
    }
}

impl ReadFrom for BundleContents {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(read_template_list(data, None)?))
    }
}

impl ToNbtTag for BundleContents {
    fn to_nbt_tag(self) -> NbtTag {
        template_list_nbt(&self.items)
    }
}

impl FromNbtTag for BundleContents {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Some(Self::new(template_list_from_nbt(tag, None)?))
    }
}

impl HashComponent for BundleContents {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_template_list(&self.items, hasher);
    }
}

impl ValidatePersistentComponent for BundleContents {
    fn validate_persistent(&self) -> Result<()> {
        validate_templates(self.items.iter())
    }
}

/// Up to 256 dense optional container slots.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ItemContainerContents {
    items: Vec<Option<ItemStackTemplate>>,
}

impl ItemContainerContents {
    pub const MAX_SIZE: usize = 256;

    #[must_use]
    pub const fn empty() -> Self {
        Self { items: Vec::new() }
    }

    pub fn new(items: Vec<Option<ItemStackTemplate>>) -> Result<Self> {
        if items.len() > Self::MAX_SIZE {
            return Err(Error::other(format!(
                "Got {} container slots, but maximum is {}",
                items.len(),
                Self::MAX_SIZE
            )));
        }
        Ok(Self { items })
    }

    #[must_use]
    pub fn items(&self) -> &[Option<ItemStackTemplate>] {
        &self.items
    }

    fn from_slots(slots: Vec<ContainerSlot>) -> Option<Self> {
        if slots.len() > Self::MAX_SIZE {
            return None;
        }
        let size = slots.iter().map(|slot| slot.index + 1).max().unwrap_or(0);
        let mut items = vec![None; size];
        for slot in slots {
            items[slot.index] = Some(slot.item);
        }
        Self::new(items).ok()
    }

    fn slots(&self) -> impl Iterator<Item = (usize, &ItemStackTemplate)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.as_ref().map(|item| (index, item)))
    }
}

impl WriteTo for ItemContainerContents {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_count(self.items.len(), Some(Self::MAX_SIZE), writer)?;
        for item in &self.items {
            item.is_some().write(writer)?;
            if let Some(item) = item {
                item.write(writer)?;
            }
        }
        Ok(())
    }
}

impl ReadFrom for ItemContainerContents {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_count(data, Some(Self::MAX_SIZE))?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(if bool::read(data)? {
                Some(ItemStackTemplate::read(data)?)
            } else {
                None
            });
        }
        Self::new(items)
    }
}

impl ToNbtTag for ItemContainerContents {
    fn to_nbt_tag(self) -> NbtTag {
        let slots = self
            .slots()
            .map(|(index, item)| container_slot_nbt(index, item))
            .collect();
        if self.items.iter().all(Option::is_none) {
            NbtTag::List(NbtList::Empty)
        } else {
            NbtTag::List(NbtList::Compound(slots))
        }
    }
}

impl FromNbtTag for ItemContainerContents {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let list = tag.list()?;
        if list.to_owned().as_nbt_tags().is_empty() {
            return Some(Self::empty());
        }
        let compounds = list.compounds()?;
        if compounds.len() > Self::MAX_SIZE {
            return None;
        }
        let slots = compounds
            .into_iter()
            .map(ContainerSlot::from_nbt_compound)
            .collect::<Option<Vec<_>>>()?;
        Self::from_slots(slots)
    }
}

impl HashComponent for ItemContainerContents {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for (index, item) in self.slots() {
            let mut slot_hasher = ComponentHasher::new();
            hash_container_slot(index, item, &mut slot_hasher);
            hasher.put_raw_bytes(&slot_hasher.finish().to_le_bytes());
        }
        hasher.end_list();
    }
}

impl ValidatePersistentComponent for ItemContainerContents {
    fn validate_persistent(&self) -> Result<()> {
        validate_templates(self.items.iter().filter_map(Option::as_ref))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ContainerSlot {
    index: usize,
    item: ItemStackTemplate,
}

impl ContainerSlot {
    fn from_nbt_compound(compound: simdnbt::borrow::NbtCompound<'_, '_>) -> Option<Self> {
        let index = compound.get("slot")?.codec_i32()?;
        let index = usize::try_from(index).ok()?;
        if index >= ItemContainerContents::MAX_SIZE {
            return None;
        }
        Some(Self {
            index,
            item: ItemStackTemplate::from_nbt_tag(compound.get("item")?)?,
        })
    }
}

fn container_slot_nbt(index: usize, item: &ItemStackTemplate) -> NbtCompound {
    let mut compound = NbtCompound::new();
    compound.insert("slot", index as i32);
    compound.insert("item", item.to_nbt_tag_ref());
    compound
}

fn hash_container_slot(index: usize, item: &ItemStackTemplate, hasher: &mut ComponentHasher) {
    let mut entries = Vec::with_capacity(2);
    push_hash_entry(&mut entries, "slot", &(index as i32));
    push_hash_entry(&mut entries, "item", item);
    sort_map_entries(&mut entries);
    hasher.start_map();
    for entry in &entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

/// The non-empty absorbed block item carried by a sulfur cube.
#[derive(Debug, Clone, PartialEq)]
pub struct SulfurCubeContent {
    absorbed_block_item_stack: ItemStackTemplate,
}

impl SulfurCubeContent {
    #[must_use]
    pub const fn new(absorbed_block_item_stack: ItemStackTemplate) -> Self {
        Self {
            absorbed_block_item_stack,
        }
    }

    #[must_use]
    pub const fn absorbed_block_item_stack(&self) -> &ItemStackTemplate {
        &self.absorbed_block_item_stack
    }
}

impl_template_wrapper_codecs!(SulfurCubeContent, absorbed_block_item_stack);

impl ValidatePersistentComponent for SulfurCubeContent {
    fn validate_persistent(&self) -> Result<()> {
        self.absorbed_block_item_stack
            .validate_persistent_encoding()
    }
}

fn write_template_list(
    items: &[ItemStackTemplate],
    max: Option<usize>,
    writer: &mut impl Write,
) -> Result<()> {
    write_count(items.len(), max, writer)?;
    for item in items {
        item.write(writer)?;
    }
    Ok(())
}

fn validate_templates<'a>(items: impl IntoIterator<Item = &'a ItemStackTemplate>) -> Result<()> {
    for item in items {
        item.validate_persistent_encoding()?;
    }
    Ok(())
}

fn read_template_list(
    data: &mut Cursor<&[u8]>,
    max: Option<usize>,
) -> Result<Vec<ItemStackTemplate>> {
    let count = read_count(data, max)?;
    let mut items = Vec::with_capacity(count.min(65_536));
    for _ in 0..count {
        items.push(ItemStackTemplate::read(data)?);
    }
    Ok(items)
}

fn write_count(count: usize, max: Option<usize>, writer: &mut impl Write) -> Result<()> {
    if let Some(max) = max
        && count > max
    {
        return Err(Error::other(format!(
            "{count} elements exceeded max size of {max}"
        )));
    }
    let count = i32::try_from(count).map_err(|_| Error::other("List is too large"))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>, max: Option<usize>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    let count = usize::try_from(count).map_err(|_| Error::other("Negative list length"))?;
    if let Some(max) = max
        && count > max
    {
        return Err(Error::other(format!(
            "{count} elements exceeded max size of {max}"
        )));
    }
    Ok(count)
}

fn template_list_nbt(items: &[ItemStackTemplate]) -> NbtTag {
    if items.is_empty() {
        return NbtTag::List(NbtList::Empty);
    }
    NbtTag::List(NbtList::Compound(
        items
            .iter()
            .map(|item| match item.to_nbt_tag_ref() {
                NbtTag::Compound(compound) => compound,
                _ => unreachable!("item stack template primary codec is a compound"),
            })
            .collect(),
    ))
}

fn template_list_from_nbt(
    tag: simdnbt::borrow::NbtTag,
    max: Option<usize>,
) -> Option<Vec<ItemStackTemplate>> {
    let list = tag.list()?;
    if list.to_owned().as_nbt_tags().is_empty() {
        return Some(Vec::new());
    }

    if let Some(values) = list.strings() {
        if max.is_some_and(|max| values.len() > max) {
            return None;
        }
        return values
            .iter()
            .map(|value| ItemStackTemplate::from_nbt_identifier(&value.to_str()))
            .collect();
    }

    let compounds = list.compounds()?;
    if max.is_some_and(|max| compounds.len() > max) {
        return None;
    }
    compounds
        .into_iter()
        .map(ItemStackTemplate::from_nbt_compound)
        .collect()
}

fn hash_template_list(items: &[ItemStackTemplate], hasher: &mut ComponentHasher) {
    hasher.start_list();
    for item in items {
        hasher.put_component_hash(item);
    }
    hasher.end_list();
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key.hash_component(&mut key_hasher);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::NbtList;
    use simdnbt::{FromNbtTag, ToNbtTag};
    use steel_utils::hash::HashComponent;
    use steel_utils::serial::{ReadFrom, WriteTo};

    use super::{
        BundleContents, ChargedProjectiles, ItemContainerContents, SulfurCubeContent, UseRemainder,
    };
    use crate::data_components::DataComponentPatch;
    use crate::data_components::components::{BeehiveOccupant, Bees, CustomData, EntityData};
    use crate::data_components::vanilla_components::{
        BEES, BUNDLE_CONTENTS, CHARGED_PROJECTILES, CONTAINER, MAX_STACK_SIZE, USE_REMAINDER,
    };
    use crate::test_support::init_test_registry;
    use crate::{ItemStackTemplate, REGISTRY, vanilla_entities, vanilla_items};

    fn round_trip<T>(value: T)
    where
        T: Clone
            + std::fmt::Debug
            + PartialEq
            + WriteTo
            + ReadFrom
            + ToNbtTag
            + FromNbtTag
            + HashComponent,
    {
        let nbt = value.clone().to_nbt_tag();
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).expect("NBT should parse");
        let decoded = T::from_nbt_tag(borrowed.as_tag()).expect("NBT should decode");
        assert_eq!(decoded, value);
        assert_eq!(decoded.compute_hash(), value.compute_hash());

        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("network value should encode");
        assert_eq!(
            T::read(&mut Cursor::new(network.as_slice())).expect("network value should decode"),
            value
        );
    }

    #[test]
    fn recursive_item_components_round_trip_both_codecs() {
        init_test_registry();
        let arrow = ItemStackTemplate::new(&vanilla_items::ARROW);
        let diamond = ItemStackTemplate::new(&vanilla_items::DIAMOND);
        round_trip(UseRemainder::new(ItemStackTemplate::new(
            &vanilla_items::BOWL,
        )));
        round_trip(
            ChargedProjectiles::new(vec![arrow.clone()]).expect("one projectile should fit"),
        );
        round_trip(BundleContents::new(vec![diamond.clone()]));
        round_trip(
            ItemContainerContents::new(vec![None, Some(diamond)]).expect("two slots should fit"),
        );
        round_trip(SulfurCubeContent::new(arrow));
    }

    #[test]
    fn sparse_container_persistence_and_dense_network_preserve_vanilla_shapes() {
        init_test_registry();
        let contents = ItemContainerContents::new(vec![
            None,
            Some(ItemStackTemplate::new(&vanilla_items::DIAMOND)),
            None,
        ])
        .expect("three slots should fit");
        let nbt = contents.clone().to_nbt_tag();
        let compounds = nbt.list().and_then(NbtList::compounds).expect("slot list");
        assert_eq!(compounds.len(), 1);
        assert_eq!(compounds[0].int("slot"), Some(1));

        let mut network = Vec::new();
        contents
            .write(&mut network)
            .expect("contents should encode");
        assert_eq!(network[0], 3);
    }

    #[test]
    fn recursive_collection_limits_are_enforced() {
        init_test_registry();
        let projectile = ItemStackTemplate::new(&vanilla_items::ARROW);
        assert!(
            ChargedProjectiles::new(vec![projectile; ChargedProjectiles::MAX_SIZE + 1]).is_err()
        );
        assert!(
            ItemContainerContents::new(vec![None; ItemContainerContents::MAX_SIZE + 1]).is_err()
        );
    }

    #[test]
    fn bundle_weight_matches_nested_bundle_and_beehive_rules() {
        init_test_registry();

        let mut inner_patch = DataComponentPatch::new();
        inner_patch.set(
            BUNDLE_CONTENTS,
            BundleContents::new(vec![ItemStackTemplate::new(&vanilla_items::STONE)]),
        );
        let nested_bundle =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, inner_patch)
                .expect("nested bundle should persist");
        let nested_weight = BundleContents::new(vec![nested_bundle])
            .compute_weight()
            .expect("small nested bundle weight should compute");
        assert_eq!(
            (nested_weight.numerator, nested_weight.denominator),
            (5, 64)
        );

        let occupant = BeehiveOccupant::new(
            EntityData::new(&vanilla_entities::BEE, CustomData::default()),
            0,
            0,
        );
        let mut beehive_patch = DataComponentPatch::new();
        beehive_patch.set(BEES, Bees::new(vec![occupant]));
        let beehive =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::BEEHIVE, 1, beehive_patch)
                .expect("occupied beehive should persist");
        let beehive_weight = BundleContents::new(vec![beehive])
            .compute_weight()
            .expect("occupied beehive weight should compute");
        assert_eq!(
            (beehive_weight.numerator, beehive_weight.denominator),
            (1, 1)
        );
    }

    #[test]
    fn bundle_weight_rejects_commons_fraction_denominator_overflow() {
        init_test_registry();

        let items = [97, 89, 83, 79, 73]
            .into_iter()
            .map(|max_stack_size| {
                let mut patch = DataComponentPatch::new();
                patch.set(MAX_STACK_SIZE, max_stack_size);
                ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, patch)
                    .expect("prime max stack size should be persistable")
            })
            .collect();

        assert!(BundleContents::new(items).validate_weight().is_err());
    }

    #[test]
    fn extracted_item_prototypes_use_recursive_component_values() {
        init_test_registry();
        assert_eq!(
            REGISTRY
                .items
                .iter()
                .filter(|(_, item)| item.components.has(USE_REMAINDER))
                .count(),
            7
        );
        assert_eq!(
            REGISTRY
                .items
                .iter()
                .filter(|(_, item)| item.components.has(CHARGED_PROJECTILES))
                .count(),
            1
        );
        assert_eq!(
            REGISTRY
                .items
                .iter()
                .filter(|(_, item)| item.components.has(BUNDLE_CONTENTS))
                .count(),
            17
        );
        assert_eq!(
            REGISTRY
                .items
                .iter()
                .filter(|(_, item)| item.components.has(CONTAINER))
                .count(),
            44
        );

        assert_eq!(
            vanilla_items::MILK_BUCKET
                .components
                .get(USE_REMAINDER)
                .map(|remainder| remainder.convert_into().item()),
            Some(&*vanilla_items::BUCKET)
        );
    }
}
