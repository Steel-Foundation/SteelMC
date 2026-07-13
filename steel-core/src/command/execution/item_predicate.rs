//! Vanilla item-predicate command argument parsing and matching.

use std::cmp::Ordering;

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::{
    REGISTRY, RegistryExt as _, TaggedRegistryExt as _,
    attribute::{AttributeModifierOperation, AttributeRef},
    data_components::{
        ComponentData, ComponentDataDiscriminant, ComponentEntry, vanilla_components,
    },
    enchantment::EnchantmentRef,
    equipment::EquipmentSlotGroup,
    item_stack::ItemStack,
    items::ItemRef,
};
use steel_utils::{Identifier, nbt::parse_snbt_argument};
use text_components::TextComponent;

use crate::command::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, ReaderCursor, StringReader, SuggestionsBuilder,
};

use super::{
    argument::{matches_substring, parse_identifier},
    item::{component_value_is_valid, numeric_i32, read_component_value},
};

const VANILLA_DATA_COMPONENT_PREDICATE_KEYS: &[&str] = &[
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

/// A fully decoded item predicate retained until command execution.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemPredicate {
    target: ItemPredicateTarget,
    conditions: Vec<ItemPredicateCondition>,
}

#[derive(Clone, Debug, PartialEq)]
enum ItemPredicateTarget {
    Any,
    Item(ItemRef),
    Tag(Identifier),
}

#[derive(Clone, Debug, PartialEq)]
struct ItemPredicateCondition {
    alternatives: Vec<ItemPredicateTerm>,
}

#[derive(Clone, Debug, PartialEq)]
enum ItemPredicateTerm {
    Always,
    ComponentPresence(Identifier),
    ComponentValue {
        key: Identifier,
        value: Box<ComponentData>,
    },
    Count(IntRange),
    DataPredicate(DataComponentPredicate),
    Not(Box<Self>),
}

#[derive(Clone, Debug, PartialEq)]
enum DataComponentPredicate {
    Damage(DamagePredicate),
    Enchantments {
        stored: bool,
        predicates: Vec<EnchantmentPredicate>,
    },
    AttributeModifiers(AttributeModifiersPredicate),
}

impl ItemPredicate {
    #[must_use]
    pub(crate) fn matches(&self, stack: &ItemStack) -> bool {
        self.target.matches(stack)
            && self
                .conditions
                .iter()
                .all(|condition| condition.matches(stack))
    }
}

impl ItemPredicateTarget {
    fn matches(&self, stack: &ItemStack) -> bool {
        match self {
            Self::Any => true,
            Self::Item(item) => stack.is(item),
            Self::Tag(tag) => REGISTRY.items.is_in_tag(stack.item(), tag),
        }
    }
}

impl ItemPredicateCondition {
    fn matches(&self, stack: &ItemStack) -> bool {
        self.alternatives
            .iter()
            .any(|alternative| alternative.matches(stack))
    }
}

impl ItemPredicateTerm {
    fn matches(&self, stack: &ItemStack) -> bool {
        match self {
            Self::Always => true,
            Self::ComponentPresence(key) => stack.has_component(key),
            Self::ComponentValue { key, value } => stack
                .get_effective_value_raw(key)
                .is_some_and(|actual| actual == value.as_ref()),
            Self::Count(range) => range.matches(stack.count()),
            Self::DataPredicate(predicate) => predicate.matches(stack),
            Self::Not(term) => !term.matches(stack),
        }
    }
}

impl DataComponentPredicate {
    fn matches(&self, stack: &ItemStack) -> bool {
        match self {
            Self::Damage(predicate) => predicate.matches(stack),
            Self::Enchantments { stored, predicates } => {
                let enchantments = if *stored {
                    stack.get(vanilla_components::STORED_ENCHANTMENTS)
                } else {
                    stack.get(vanilla_components::ENCHANTMENTS)
                };
                enchantments.is_some_and(|enchantments| {
                    predicates
                        .iter()
                        .all(|predicate| predicate.matches(enchantments))
                })
            }
            Self::AttributeModifiers(predicate) => stack
                .get(vanilla_components::ATTRIBUTE_MODIFIERS)
                .is_some_and(|modifiers| predicate.matches(modifiers)),
        }
    }
}

pub(super) fn parse_item_predicate(
    reader: &mut StringReader<'_>,
) -> Result<ItemPredicate, CommandSyntaxError> {
    let start = reader.checkpoint();
    let result = parse_item_predicate_inner(reader);
    if result.is_err() {
        reader.restore(start);
    }
    result
}

fn parse_item_predicate_inner(
    reader: &mut StringReader<'_>,
) -> Result<ItemPredicate, CommandSyntaxError> {
    let target = parse_target(reader)?;
    let before_whitespace = reader.checkpoint();
    reader.skip_whitespace();
    if reader.peek() != Some('[') {
        reader.restore(before_whitespace);
        return Ok(ItemPredicate {
            target,
            conditions: Vec::new(),
        });
    }

    reader.skip();
    reader.skip_whitespace();
    if reader.peek() == Some(']') {
        reader.skip();
        return Ok(ItemPredicate {
            target,
            conditions: Vec::new(),
        });
    }

    let mut conditions = Vec::new();
    loop {
        conditions.push(parse_condition(reader)?);
        reader.skip_whitespace();
        if reader.peek() != Some(',') {
            reader.expect(']')?;
            break;
        }
        reader.skip();
    }

    Ok(ItemPredicate { target, conditions })
}

fn parse_target(reader: &mut StringReader<'_>) -> Result<ItemPredicateTarget, CommandSyntaxError> {
    reader.skip_whitespace();
    if reader.peek() == Some('*') {
        reader.skip();
        return Ok(ItemPredicateTarget::Any);
    }

    if reader.peek() == Some('#') {
        let start = reader.checkpoint();
        reader.skip();
        let key = parse_identifier(reader)?;
        if REGISTRY.items.get_tag(&key).is_some() {
            return Ok(ItemPredicateTarget::Tag(key));
        }
        reader.restore(start);
        return Err(dynamic_error(reader, format!("Unknown item tag '#{key}'")));
    }

    let start = reader.checkpoint();
    let key = parse_identifier(reader)?;
    let Some(item) = REGISTRY.items.by_key(&key) else {
        reader.restore(start);
        return Err(dynamic_error(reader, format!("Unknown item '{key}'")));
    };
    Ok(ItemPredicateTarget::Item(item))
}

fn parse_condition(
    reader: &mut StringReader<'_>,
) -> Result<ItemPredicateCondition, CommandSyntaxError> {
    let mut alternatives = Vec::new();
    loop {
        alternatives.push(parse_term(reader)?);
        reader.skip_whitespace();
        if reader.peek() != Some('|') {
            break;
        }
        reader.skip();
    }
    Ok(ItemPredicateCondition { alternatives })
}

fn parse_term(reader: &mut StringReader<'_>) -> Result<ItemPredicateTerm, CommandSyntaxError> {
    reader.skip_whitespace();
    if reader.peek() == Some('!') {
        reader.skip();
        return parse_test(reader).map(|term| ItemPredicateTerm::Not(Box::new(term)));
    }
    parse_test(reader)
}

fn parse_test(reader: &mut StringReader<'_>) -> Result<ItemPredicateTerm, CommandSyntaxError> {
    reader.skip_whitespace();
    let key_start = reader.checkpoint();
    let key = parse_identifier(reader)?;
    reader.skip_whitespace();

    match reader.peek() {
        Some('=') => {
            reader.skip();
            parse_component_value_test(reader, key_start, key)
        }
        Some('~') => {
            reader.skip();
            parse_predicate_value_test(reader, key_start, key)
        }
        _ => parse_component_presence_test(reader, key_start, key),
    }
}

fn parse_component_presence_test(
    reader: &mut StringReader<'_>,
    key_start: ReaderCursor,
    key: Identifier,
) -> Result<ItemPredicateTerm, CommandSyntaxError> {
    if is_count_key(&key) {
        return Ok(ItemPredicateTerm::Always);
    }
    if persistent_component(&key).is_some() {
        return Ok(ItemPredicateTerm::ComponentPresence(key));
    }
    reader.restore(key_start);
    Err(dynamic_error(
        reader,
        format!("Unknown item component '{key}'"),
    ))
}

fn parse_component_value_test(
    reader: &mut StringReader<'_>,
    key_start: ReaderCursor,
    key: Identifier,
) -> Result<ItemPredicateTerm, CommandSyntaxError> {
    let tag = read_nbt(reader, "component", &key)?;
    if is_count_key(&key) {
        return parse_int_range(&tag)
            .map(ItemPredicateTerm::Count)
            .ok_or_else(|| malformed_component(reader, &key));
    }

    let Some(entry) = persistent_component(&key) else {
        reader.restore(key_start);
        return Err(dynamic_error(
            reader,
            format!("Unknown item component '{key}'"),
        ));
    };
    if entry.expected_discriminant == ComponentDataDiscriminant::Todo {
        return Err(dynamic_error(
            reader,
            format!("Component value '{key}' is not implemented by Steel yet"),
        ));
    }
    let Some(value) = read_component_value(entry, &tag) else {
        return Err(malformed_component(reader, &key));
    };
    if !component_value_is_valid(&key, &value) {
        return Err(malformed_component(reader, &key));
    }
    Ok(ItemPredicateTerm::ComponentValue {
        key,
        value: Box::new(value),
    })
}

fn parse_predicate_value_test(
    reader: &mut StringReader<'_>,
    key_start: ReaderCursor,
    key: Identifier,
) -> Result<ItemPredicateTerm, CommandSyntaxError> {
    let tag = read_nbt(reader, "predicate", &key)?;
    if is_count_key(&key) {
        return parse_int_range(&tag)
            .map(ItemPredicateTerm::Count)
            .ok_or_else(|| malformed_predicate(reader, &key));
    }

    if is_vanilla_predicate_key(&key) {
        let predicate = parse_supported_data_predicate(&key, &tag)
            .ok_or_else(|| malformed_or_unsupported_predicate(reader, &key))?;
        return Ok(ItemPredicateTerm::DataPredicate(predicate));
    }

    if REGISTRY.data_components.by_key(&key).is_some() {
        if !matches!(tag, NbtTag::Compound(_)) {
            return Err(malformed_predicate(reader, &key));
        }
        return Ok(ItemPredicateTerm::ComponentPresence(key));
    }

    reader.restore(key_start);
    Err(dynamic_error(
        reader,
        format!("Unknown item predicate '{key}'"),
    ))
}

fn read_nbt(
    reader: &mut StringReader<'_>,
    description: &str,
    key: &Identifier,
) -> Result<NbtTag, CommandSyntaxError> {
    reader.skip_whitespace();
    let (tag, consumed) = parse_snbt_argument(reader.remaining()).map_err(|error| {
        reader.advance_bytes(error.cursor());
        dynamic_error(reader, error.component())
    })?;
    if !reader.advance_bytes(consumed) {
        return Err(dynamic_error(
            reader,
            format!("Malformed item {description} '{key}'"),
        ));
    }
    Ok(tag)
}

fn persistent_component(key: &Identifier) -> Option<&'static ComponentEntry> {
    let entry = REGISTRY.data_components.by_key(key)?;
    entry.is_persistent().then_some(entry)
}

fn malformed_component(reader: &StringReader<'_>, key: &Identifier) -> CommandSyntaxError {
    dynamic_error(reader, format!("Malformed item component '{key}'"))
}

fn malformed_predicate(reader: &StringReader<'_>, key: &Identifier) -> CommandSyntaxError {
    dynamic_error(reader, format!("Malformed item predicate '{key}'"))
}

fn malformed_or_unsupported_predicate(
    reader: &StringReader<'_>,
    key: &Identifier,
) -> CommandSyntaxError {
    if is_supported_predicate_key(key) {
        return malformed_predicate(reader, key);
    }
    // TODO: Decode this predicate once Steel implements its backing data component.
    dynamic_error(
        reader,
        format!("Item predicate '{key}' is not implemented by Steel yet"),
    )
}

fn dynamic_error(
    reader: &StringReader<'_>,
    message: impl Into<TextComponent>,
) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message.into())))
}

fn is_count_key(key: &Identifier) -> bool {
    key.namespace == Identifier::VANILLA_NAMESPACE && key.path == "count"
}

fn is_vanilla_predicate_key(key: &Identifier) -> bool {
    key.namespace == Identifier::VANILLA_NAMESPACE
        && VANILLA_DATA_COMPONENT_PREDICATE_KEYS
            .iter()
            .any(|path| key.path == *path)
}

fn is_supported_predicate_key(key: &Identifier) -> bool {
    key.namespace == Identifier::VANILLA_NAMESPACE
        && matches!(
            key.path.as_ref(),
            "damage" | "enchantments" | "stored_enchantments" | "attribute_modifiers"
        )
}

fn parse_supported_data_predicate(
    key: &Identifier,
    tag: &NbtTag,
) -> Option<DataComponentPredicate> {
    match key.path.as_ref() {
        "damage" => parse_damage_predicate(tag).map(DataComponentPredicate::Damage),
        "enchantments" => parse_enchantment_predicates(tag).map(|predicates| {
            DataComponentPredicate::Enchantments {
                stored: false,
                predicates,
            }
        }),
        "stored_enchantments" => parse_enchantment_predicates(tag).map(|predicates| {
            DataComponentPredicate::Enchantments {
                stored: true,
                predicates,
            }
        }),
        "attribute_modifiers" => {
            parse_attribute_modifiers_predicate(tag).map(DataComponentPredicate::AttributeModifiers)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct IntRange {
    min: Option<i32>,
    max: Option<i32>,
}

impl IntRange {
    const ANY: Self = Self {
        min: None,
        max: None,
    };

    fn matches(self, value: i32) -> bool {
        self.min.is_none_or(|minimum| value >= minimum)
            && self.max.is_none_or(|maximum| value <= maximum)
    }

    fn matches_u32(self, value: u32) -> bool {
        let value = i64::from(value);
        self.min.is_none_or(|minimum| value >= i64::from(minimum))
            && self.max.is_none_or(|maximum| value <= i64::from(maximum))
    }

    fn matches_usize(self, value: usize) -> bool {
        let minimum_matches = self.min.is_none_or(|minimum| {
            minimum <= 0 || usize::try_from(minimum).is_ok_and(|minimum| value >= minimum)
        });
        let maximum_matches = self
            .max
            .is_none_or(|maximum| usize::try_from(maximum).is_ok_and(|maximum| value <= maximum));
        minimum_matches && maximum_matches
    }

    const fn is_any(self) -> bool {
        self.min.is_none() && self.max.is_none()
    }
}

fn parse_int_range(tag: &NbtTag) -> Option<IntRange> {
    if let Some(value) = numeric_i32(tag) {
        return Some(IntRange {
            min: Some(value),
            max: Some(value),
        });
    }
    let NbtTag::Compound(compound) = tag else {
        return None;
    };
    let min = parse_optional_field(compound, "min", numeric_i32).ok()?;
    let max = parse_optional_field(compound, "max", numeric_i32).ok()?;
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        return None;
    }
    Some(IntRange { min, max })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DamagePredicate {
    durability: IntRange,
    damage: IntRange,
}

impl DamagePredicate {
    fn matches(self, stack: &ItemStack) -> bool {
        let Some(damage) = stack.get(vanilla_components::DAMAGE).copied() else {
            return false;
        };
        let maximum = stack
            .get(vanilla_components::MAX_DAMAGE)
            .copied()
            .unwrap_or(0);
        self.durability.matches(maximum - damage) && self.damage.matches(damage)
    }
}

fn parse_damage_predicate(tag: &NbtTag) -> Option<DamagePredicate> {
    let NbtTag::Compound(compound) = tag else {
        return None;
    };
    let durability = parse_optional_field(compound, "durability", parse_int_range)
        .ok()?
        .unwrap_or(IntRange::ANY);
    let damage = parse_optional_field(compound, "damage", parse_int_range)
        .ok()?
        .unwrap_or(IntRange::ANY);
    Some(DamagePredicate { durability, damage })
}

#[derive(Clone, Debug, PartialEq)]
struct EnchantmentPredicate {
    enchantments: Option<Vec<EnchantmentRef>>,
    levels: IntRange,
}

impl EnchantmentPredicate {
    fn matches(&self, enchantments: &vanilla_components::ItemEnchantments) -> bool {
        if let Some(expected) = &self.enchantments {
            return expected.iter().any(|enchantment| {
                let level = enchantments.get_level(&enchantment.key);
                level != 0 && self.levels.matches_u32(level)
            });
        }
        if !self.levels.is_any() {
            return enchantments
                .iter()
                .any(|(_, level)| self.levels.matches_u32(*level));
        }
        !enchantments.is_empty()
    }
}

fn parse_enchantment_predicates(tag: &NbtTag) -> Option<Vec<EnchantmentPredicate>> {
    match tag {
        NbtTag::List(NbtList::Empty) => Some(Vec::new()),
        NbtTag::List(NbtList::Compound(compounds)) => {
            compounds.iter().map(parse_enchantment_predicate).collect()
        }
        _ => None,
    }
}

fn parse_enchantment_predicate(compound: &NbtCompound) -> Option<EnchantmentPredicate> {
    let enchantments =
        parse_optional_field(compound, "enchantments", parse_enchantment_holder_set).ok()?;
    let levels = parse_optional_field(compound, "levels", parse_int_range)
        .ok()?
        .unwrap_or(IntRange::ANY);
    Some(EnchantmentPredicate {
        enchantments,
        levels,
    })
}

fn parse_enchantment_holder_set(tag: &NbtTag) -> Option<Vec<EnchantmentRef>> {
    match tag {
        NbtTag::String(value) => parse_enchantment_holder(&value.to_string()),
        NbtTag::List(NbtList::Empty) => Some(Vec::new()),
        NbtTag::List(NbtList::String(values)) => {
            let mut enchantments = Vec::new();
            for value in values {
                enchantments.push(parse_enchantment_reference(&value.to_string())?);
            }
            Some(enchantments)
        }
        _ => None,
    }
}

fn parse_enchantment_holder(value: &str) -> Option<Vec<EnchantmentRef>> {
    if let Some(tag) = value.strip_prefix('#') {
        let key = parse_identifier_with_default_namespace(tag)?;
        return REGISTRY.enchantments.get_tag(&key);
    }
    parse_enchantment_reference(value).map(|enchantment| vec![enchantment])
}

fn parse_enchantment_reference(value: &str) -> Option<EnchantmentRef> {
    let key = parse_identifier_with_default_namespace(value)?;
    REGISTRY.enchantments.by_key(&key)
}

#[derive(Clone, Debug, PartialEq)]
struct AttributeModifiersPredicate {
    modifiers: Option<AttributeModifierCollectionPredicate>,
}

impl AttributeModifiersPredicate {
    fn matches(&self, modifiers: &vanilla_components::ItemAttributeModifiers) -> bool {
        self.modifiers
            .as_ref()
            .is_none_or(|predicate| predicate.matches(&modifiers.modifiers))
    }
}

fn parse_attribute_modifiers_predicate(tag: &NbtTag) -> Option<AttributeModifiersPredicate> {
    let NbtTag::Compound(compound) = tag else {
        return None;
    };
    let modifiers =
        parse_optional_field(compound, "modifiers", parse_attribute_modifier_collection).ok()?;
    Some(AttributeModifiersPredicate { modifiers })
}

#[derive(Clone, Debug, PartialEq)]
struct AttributeModifierCollectionPredicate {
    contains: Vec<AttributeModifierEntryPredicate>,
    counts: Vec<AttributeModifierCountPredicate>,
    size: Option<IntRange>,
}

impl AttributeModifierCollectionPredicate {
    fn matches(&self, modifiers: &[vanilla_components::ItemAttributeModifierEntry]) -> bool {
        let mut matched = vec![false; self.contains.len()];
        for modifier in modifiers {
            for (matched, predicate) in matched.iter_mut().zip(&self.contains) {
                if !*matched && predicate.matches(modifier) {
                    *matched = true;
                }
            }
        }
        matched.into_iter().all(|matched| matched)
            && self.counts.iter().all(|predicate| {
                let count = modifiers
                    .iter()
                    .filter(|modifier| predicate.test.matches(modifier))
                    .count();
                predicate.count.matches_usize(count)
            })
            && self
                .size
                .is_none_or(|range| range.matches_usize(modifiers.len()))
    }
}

fn parse_attribute_modifier_collection(
    tag: &NbtTag,
) -> Option<AttributeModifierCollectionPredicate> {
    let NbtTag::Compound(compound) = tag else {
        return None;
    };
    let contains = parse_optional_field(
        compound,
        "contains",
        parse_attribute_modifier_predicate_list,
    )
    .ok()?
    .unwrap_or_default();
    let counts = parse_optional_field(compound, "count", parse_attribute_modifier_count_list)
        .ok()?
        .unwrap_or_default();
    let size = parse_optional_field(compound, "size", parse_int_range).ok()?;
    Some(AttributeModifierCollectionPredicate {
        contains,
        counts,
        size,
    })
}

fn parse_attribute_modifier_predicate_list(
    tag: &NbtTag,
) -> Option<Vec<AttributeModifierEntryPredicate>> {
    match tag {
        NbtTag::List(NbtList::Empty) => Some(Vec::new()),
        NbtTag::List(NbtList::Compound(compounds)) => compounds
            .iter()
            .map(parse_attribute_modifier_entry)
            .collect(),
        _ => None,
    }
}

fn parse_attribute_modifier_count_list(
    tag: &NbtTag,
) -> Option<Vec<AttributeModifierCountPredicate>> {
    match tag {
        NbtTag::List(NbtList::Empty) => Some(Vec::new()),
        NbtTag::List(NbtList::Compound(compounds)) => compounds
            .iter()
            .map(parse_attribute_modifier_count)
            .collect(),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AttributeModifierCountPredicate {
    test: AttributeModifierEntryPredicate,
    count: IntRange,
}

fn parse_attribute_modifier_count(
    compound: &NbtCompound,
) -> Option<AttributeModifierCountPredicate> {
    let NbtTag::Compound(test) = compound.get("test")? else {
        return None;
    };
    let count = parse_int_range(compound.get("count")?)?;
    Some(AttributeModifierCountPredicate {
        test: parse_attribute_modifier_entry(test)?,
        count,
    })
}

#[derive(Clone, Debug, PartialEq)]
struct AttributeModifierEntryPredicate {
    attributes: Option<Vec<AttributeRef>>,
    id: Option<Identifier>,
    amount: DoubleRange,
    operation: Option<AttributeModifierOperation>,
    slot: Option<EquipmentSlotGroup>,
}

impl AttributeModifierEntryPredicate {
    fn matches(&self, modifier: &vanilla_components::ItemAttributeModifierEntry) -> bool {
        self.attributes.as_ref().is_none_or(|attributes| {
            attributes
                .iter()
                .any(|attribute| attribute.key == modifier.attribute.key)
        }) && self.id.as_ref().is_none_or(|id| id == &modifier.id)
            && self.amount.matches(modifier.amount)
            && self
                .operation
                .is_none_or(|operation| operation == modifier.operation)
            && self.slot.is_none_or(|slot| slot == modifier.slot)
    }
}

fn parse_attribute_modifier_entry(
    compound: &NbtCompound,
) -> Option<AttributeModifierEntryPredicate> {
    let attributes =
        parse_optional_field(compound, "attribute", parse_attribute_holder_set).ok()?;
    let id = parse_optional_field(compound, "id", parse_identifier_tag).ok()?;
    let amount = parse_optional_field(compound, "amount", parse_double_range)
        .ok()?
        .unwrap_or(DoubleRange::ANY);
    let operation =
        parse_optional_field(compound, "operation", parse_attribute_modifier_operation).ok()?;
    let slot = parse_optional_field(compound, "slot", parse_equipment_slot_group).ok()?;
    Some(AttributeModifierEntryPredicate {
        attributes,
        id,
        amount,
        operation,
        slot,
    })
}

fn parse_attribute_holder_set(tag: &NbtTag) -> Option<Vec<AttributeRef>> {
    match tag {
        NbtTag::String(value) => parse_attribute_holder(&value.to_string()),
        NbtTag::List(NbtList::Empty) => Some(Vec::new()),
        NbtTag::List(NbtList::String(values)) => {
            let mut attributes = Vec::new();
            for value in values {
                attributes.extend(parse_attribute_holder(&value.to_string())?);
            }
            Some(attributes)
        }
        _ => None,
    }
}

fn parse_attribute_holder(value: &str) -> Option<Vec<AttributeRef>> {
    if value.starts_with('#') {
        // TODO: Support attribute tags once Steel's attribute registry stores them.
        return None;
    }
    let key = parse_identifier_with_default_namespace(value)?;
    REGISTRY
        .attributes
        .by_key(&key)
        .map(|attribute| vec![attribute])
}

fn parse_identifier_tag(tag: &NbtTag) -> Option<Identifier> {
    let NbtTag::String(value) = tag else {
        return None;
    };
    parse_identifier_with_default_namespace(&value.to_string())
}

fn parse_attribute_modifier_operation(tag: &NbtTag) -> Option<AttributeModifierOperation> {
    let NbtTag::String(value) = tag else {
        return None;
    };
    AttributeModifierOperation::by_name(&value.to_string())
}

fn parse_equipment_slot_group(tag: &NbtTag) -> Option<EquipmentSlotGroup> {
    let NbtTag::String(value) = tag else {
        return None;
    };
    let value = value.to_string();
    EquipmentSlotGroup::by_name(&value).filter(|slot| slot.name() == value)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DoubleRange {
    min: Option<f64>,
    max: Option<f64>,
}

impl DoubleRange {
    const ANY: Self = Self {
        min: None,
        max: None,
    };

    fn matches(self, value: f64) -> bool {
        self.min
            .is_none_or(|minimum| minimum.partial_cmp(&value) != Some(Ordering::Greater))
            && self
                .max
                .is_none_or(|maximum| maximum.partial_cmp(&value) != Some(Ordering::Less))
    }
}

fn parse_double_range(tag: &NbtTag) -> Option<DoubleRange> {
    if let Some(value) = numeric_f64(tag) {
        return Some(DoubleRange {
            min: Some(value),
            max: Some(value),
        });
    }
    let NbtTag::Compound(compound) = tag else {
        return None;
    };
    let min = parse_optional_field(compound, "min", numeric_f64).ok()?;
    let max = parse_optional_field(compound, "max", numeric_f64).ok()?;
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        return None;
    }
    Some(DoubleRange { min, max })
}

fn numeric_f64(tag: &NbtTag) -> Option<f64> {
    match tag {
        NbtTag::Byte(value) => Some(f64::from(*value)),
        NbtTag::Short(value) => Some(f64::from(*value)),
        NbtTag::Int(value) => Some(f64::from(*value)),
        NbtTag::Long(value) => Some(*value as f64),
        NbtTag::Float(value) => Some(f64::from(*value)),
        NbtTag::Double(value) => Some(*value),
        _ => None,
    }
}

fn parse_optional_field<T>(
    compound: &NbtCompound,
    key: &str,
    parser: impl FnOnce(&NbtTag) -> Option<T>,
) -> Result<Option<T>, ()> {
    match compound.get(key) {
        Some(tag) => parser(tag).map(Some).ok_or(()),
        None => Ok(None),
    }
}

fn parse_identifier_with_default_namespace(value: &str) -> Option<Identifier> {
    let (namespace, path) = value.split_once(':').map_or(
        (Identifier::VANILLA_NAMESPACE, value),
        |(namespace, path)| {
            if namespace.is_empty() {
                (Identifier::VANILLA_NAMESPACE, path)
            } else {
                (namespace, path)
            }
        },
    );
    (!namespace.is_empty() && !path.is_empty() && Identifier::validate(namespace, path))
        .then(|| Identifier::new(namespace.to_owned(), path.to_owned()))
}

pub(super) fn suggest_item_predicate(builder: &mut SuggestionsBuilder<'_>) {
    let input = builder.remaining();
    let Some(component_start) = input.find('[') else {
        suggest_item_targets(input, builder);
        if valid_target(input) {
            builder.suggest(format!("{input}["));
        }
        return;
    };
    if !valid_target(input[..component_start].trim_end()) {
        return;
    }

    let Some((current_start, current)) = current_term(&input[component_start + 1..]) else {
        return;
    };
    if current.contains(['=', '~']) {
        return;
    }
    let trimmed = current.trim_start();
    let whitespace = current.len() - trimmed.len();
    let prefix_end = component_start + 1 + current_start + whitespace;
    let prefix = &input[..prefix_end];
    let resource_prefix = trimmed.strip_prefix('!').unwrap_or(trimmed);

    for entry in
        (0..REGISTRY.data_components.len()).filter_map(|id| REGISTRY.data_components.by_id(id))
    {
        if entry.is_persistent() && resource_matches(resource_prefix, &entry.key) {
            builder.suggest(format!("{prefix}{}", entry.key));
        }
    }
    let count = Identifier::vanilla_static("count");
    if resource_matches(resource_prefix, &count) {
        builder.suggest(format!("{prefix}{count}"));
    }
    for path in VANILLA_DATA_COMPONENT_PREDICATE_KEYS {
        let key = Identifier::vanilla_static(path);
        if resource_matches(resource_prefix, &key) {
            builder.suggest(format!("{prefix}{key}"));
        }
    }
}

fn suggest_item_targets(input: &str, builder: &mut SuggestionsBuilder<'_>) {
    if let Some(prefix) = input.strip_prefix('#') {
        for key in REGISTRY.items.tag_keys() {
            if resource_matches(prefix, key) {
                builder.suggest(format!("#{key}"));
            }
        }
        return;
    }
    for (_, item) in REGISTRY.items.iter() {
        if resource_matches(input, &item.key) {
            builder.suggest(item.key.to_string());
        }
    }
    if "*".starts_with(input) {
        builder.suggest("*");
    }
}

fn valid_target(input: &str) -> bool {
    if input == "*" {
        return true;
    }
    if let Some(tag) = input.strip_prefix('#') {
        return parse_identifier_text(tag)
            .is_some_and(|tag| REGISTRY.items.get_tag(&tag).is_some());
    }
    parse_identifier_text(input).is_some_and(|key| REGISTRY.items.by_key(&key).is_some())
}

fn parse_identifier_text(input: &str) -> Option<Identifier> {
    let mut reader = StringReader::new(input);
    let key = parse_identifier(&mut reader).ok()?;
    (!reader.can_read()).then_some(key)
}

fn resource_matches(pattern: &str, key: &Identifier) -> bool {
    let pattern = pattern.strip_prefix("minecraft:").unwrap_or(pattern);
    if pattern.contains(':') {
        return matches_substring(pattern, &key.to_string());
    }
    matches_substring(pattern, key.namespace.as_ref())
        || matches_substring(pattern, key.path.as_ref())
}

fn current_term(conditions: &str) -> Option<(usize, &str)> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0usize;

    for (index, character) in conditions.char_indices() {
        if let Some(terminator) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == terminator {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '{' | '[' => depth += 1,
            '}' | ']' if depth > 0 => depth -= 1,
            ']' => return None,
            ',' | '|' if depth == 0 => start = index + character.len_utf8(),
            '!' if depth == 0 && conditions[start..index].trim().is_empty() => {
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    Some((start, &conditions[start..]))
}
