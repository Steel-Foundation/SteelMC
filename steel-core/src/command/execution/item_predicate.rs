//! Vanilla item-predicate command argument parsing.
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::{
    REGISTRY, RegistryExt as _, TaggedRegistryExt as _,
    data_component_predicate::DataComponentPredicateData,
    data_components::{ComponentData, ComponentEntry},
    item_predicate::IntBounds,
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
    Count(IntBounds),
    DataPredicate(DataComponentPredicateData),
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

    if REGISTRY
        .data_component_predicate_types
        .by_key(&key)
        .is_some()
    {
        let predicate = DataComponentPredicateData::from_persistent_entry(&key, &tag)
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

fn parse_int_range(tag: &NbtTag) -> Option<IntBounds> {
    if let Some(value) = numeric_i32(tag) {
        return Some(IntBounds::exactly(value));
    }
    let NbtTag::Compound(compound) = tag else {
        return None;
    };
    let min = parse_optional_field(compound, "min", numeric_i32).ok()?;
    let max = parse_optional_field(compound, "max", numeric_i32).ok()?;
    IntBounds::new(min, max)
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
    for (_, predicate_type) in REGISTRY.data_component_predicate_types.iter() {
        if resource_matches(resource_prefix, &predicate_type.key) {
            builder.suggest(format!("{prefix}{}", predicate_type.key));
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
