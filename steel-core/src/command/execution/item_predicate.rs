//! Vanilla item-predicate command argument parsing.
use crate::item_predicate::{
    AttributeModifierCollectionPredicate, AttributeModifierCountPredicate,
    AttributeModifierEntryPredicate, AttributeModifiersPredicate, DamagePredicate,
    DataComponentPredicate, DoubleRange, EnchantmentPredicate, IntRange,
};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::{
    REGISTRY, RegistryExt as _, TaggedRegistryExt as _,
    attribute::{AttributeModifierOperation, AttributeRef},
    data_component_predicate as registered,
    data_component_predicate::DataComponentPredicateCodec as _,
    data_components::{ComponentData, ComponentEntry, vanilla_components},
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
    argument::parse_identifier,
    item::{component_value_is_valid, numeric_i32, read_component_value},
    suggestions::matches_substring,
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
            .ok_or_else(|| malformed_predicate(reader, &key))?;
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

fn parse_supported_data_predicate(
    key: &Identifier,
    tag: &NbtTag,
) -> Option<DataComponentPredicate> {
    match key.path.as_ref() {
        "custom_data" => vanilla_components::CustomData::from_nbt_value(tag)
            .map(DataComponentPredicate::CustomData),
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
        "potion_contents" => {
            registered::PotionsPredicate::from_nbt_value(tag).map(DataComponentPredicate::Potions)
        }
        "container" => registered::ContainerPredicate::from_nbt_value(tag)
            .map(DataComponentPredicate::Container),
        "bundle_contents" => {
            registered::BundlePredicate::from_nbt_value(tag).map(DataComponentPredicate::Bundle)
        }
        "firework_explosion" => registered::FireworkExplosionPredicate::from_nbt_value(tag)
            .map(DataComponentPredicate::FireworkExplosion),
        "fireworks" => registered::FireworksPredicate::from_nbt_value(tag)
            .map(DataComponentPredicate::Fireworks),
        "writable_book_content" => registered::WritableBookPredicate::from_nbt_value(tag)
            .map(DataComponentPredicate::WritableBook),
        "written_book_content" => registered::WrittenBookPredicate::from_nbt_value(tag)
            .map(DataComponentPredicate::WrittenBook),
        "trim" => registered::TrimPredicate::from_nbt_value(tag).map(DataComponentPredicate::Trim),
        "jukebox_playable" => registered::JukeboxPlayablePredicate::from_nbt_value(tag)
            .map(DataComponentPredicate::JukeboxPlayable),
        "villager/variant" => registered::VillagerTypePredicate::from_nbt_value(tag)
            .map(DataComponentPredicate::VillagerType),
        _ => None,
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

fn parse_attribute_modifiers_predicate(tag: &NbtTag) -> Option<AttributeModifiersPredicate> {
    let NbtTag::Compound(compound) = tag else {
        return None;
    };
    let modifiers =
        parse_optional_field(compound, "modifiers", parse_attribute_modifier_collection).ok()?;
    Some(AttributeModifiersPredicate { modifiers })
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
