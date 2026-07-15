//! Vanilla item-stack command argument parsing.

use rustc_hash::FxHashSet;
use simdnbt::owned::NbtTag;
use steel_registry::{
    REGISTRY, RegistryExt as _,
    data_components::{ComponentData, ComponentEntry, DataComponentPatch, vanilla_components},
    item_stack::ItemStack,
    items::ItemRef,
};
use steel_utils::{
    Identifier,
    nbt::{NbtNumeric as _, parse_snbt_argument},
    translations,
};

use crate::command::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, StringReader, SuggestionsBuilder,
};

use super::argument::{matches_substring, parse_identifier};

pub(super) fn parse_item_stack(
    reader: &mut StringReader<'_>,
) -> Result<ItemStack, CommandSyntaxError> {
    let start = reader.checkpoint();
    let result = parse_item_stack_inner(reader);
    if result.is_err() {
        reader.restore(start);
    }
    result
}

fn parse_item_stack_inner(reader: &mut StringReader<'_>) -> Result<ItemStack, CommandSyntaxError> {
    let item_start = reader.checkpoint();
    let item_key = parse_identifier(reader)?;
    let Some(item) = REGISTRY.items.by_key(&item_key) else {
        reader.restore(item_start);
        let message = translations::ARGUMENT_ITEM_ID_INVALID
            .message([item_key.to_string()])
            .component();
        return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))));
    };

    let mut patch = DataComponentPatch::new();
    if reader.peek() == Some('[') {
        parse_components(reader, &mut patch)?;
    }

    let stack = ItemStack::with_count_and_patch(item, 1, patch);
    validate_item_stack(reader, &stack)?;
    Ok(stack)
}

fn parse_components(
    reader: &mut StringReader<'_>,
    patch: &mut DataComponentPatch,
) -> Result<(), CommandSyntaxError> {
    reader.expect('[')?;
    let mut visited = FxHashSet::default();

    loop {
        reader.skip_whitespace();
        if reader.peek() == Some(']') {
            reader.skip();
            return Ok(());
        }
        if !reader.can_read() {
            return Err(expected_component(reader));
        }

        let removed = if reader.peek() == Some('!') {
            reader.skip();
            true
        } else {
            false
        };
        let key = parse_component_key(reader)?;
        if !visited.insert(key.clone()) {
            let message = translations::ARGUMENTS_ITEM_COMPONENT_REPEATED
                .message([key.to_string()])
                .component();
            return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))));
        }

        if removed {
            patch.remove_raw(key);
        } else {
            parse_component_value(reader, patch, key)?;
        }

        reader.skip_whitespace();
        if reader.peek() != Some(',') {
            reader.expect(']')?;
            return Ok(());
        }
        reader.skip();
        reader.skip_whitespace();
        if !reader.can_read() {
            return Err(expected_component(reader));
        }
    }
}

fn parse_component_key(reader: &mut StringReader<'_>) -> Result<Identifier, CommandSyntaxError> {
    let start = reader.checkpoint();
    let key = parse_identifier(reader)?;
    let Some(entry) = REGISTRY.data_components.by_key(&key) else {
        reader.restore(start);
        return Err(unknown_component(reader, &key));
    };
    if !entry.is_persistent() {
        reader.restore(start);
        return Err(unknown_component(reader, &key));
    }
    Ok(key)
}

fn parse_component_value(
    reader: &mut StringReader<'_>,
    patch: &mut DataComponentPatch,
    key: Identifier,
) -> Result<(), CommandSyntaxError> {
    reader.skip_whitespace();
    reader.expect('=')?;
    reader.skip_whitespace();

    let Some(entry) = REGISTRY.data_components.by_key(&key) else {
        return Err(unknown_component(reader, &key));
    };
    let (tag, consumed) = parse_snbt_argument(reader.remaining()).map_err(|error| {
        reader.advance_bytes(error.cursor());
        reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(error.component())))
    })?;
    if !reader.advance_bytes(consumed) {
        return Err(malformed_component(
            reader,
            &key,
            "component value ended at an invalid UTF-8 boundary",
        ));
    }
    let Some(value) = read_component_value(entry, &tag) else {
        return Err(malformed_component(
            reader,
            &key,
            "value does not match the component codec",
        ));
    };
    if !component_value_is_valid(&key, &value) {
        return Err(malformed_component(
            reader,
            &key,
            "value is outside the vanilla component range",
        ));
    }
    if !patch.set_raw(key.clone(), value) {
        return Err(malformed_component(
            reader,
            &key,
            "value does not match the registered component type",
        ));
    }
    Ok(())
}

pub(super) fn read_component_value(entry: &ComponentEntry, tag: &NbtTag) -> Option<ComponentData> {
    entry.read_nbt_owned(tag)
}

pub(super) fn numeric_i32(tag: &NbtTag) -> Option<i32> {
    tag.codec_i32()
}

pub(super) fn component_value_is_valid(key: &Identifier, value: &ComponentData) -> bool {
    if key == vanilla_components::MAX_STACK_SIZE.key() {
        return value
            .downcast_ref::<i32>()
            .is_some_and(|value| (1..=99).contains(value));
    }
    if key == vanilla_components::MAX_DAMAGE.key() {
        return value.downcast_ref::<i32>().is_some_and(|value| *value > 0);
    }
    if key == vanilla_components::DAMAGE.key() || key == vanilla_components::REPAIR_COST.key() {
        return value.downcast_ref::<i32>().is_some_and(|value| *value >= 0);
    }
    if key == vanilla_components::MINIMUM_ATTACK_CHARGE.key() {
        return value
            .downcast_ref::<f32>()
            .is_some_and(|value| value.is_finite() && !value.is_sign_negative() && *value <= 1.0);
    }
    if key == vanilla_components::POTION_DURATION_SCALE.key() {
        return value
            .downcast_ref::<f32>()
            .is_some_and(|value| value.is_finite() && !value.is_sign_negative());
    }

    true
}

fn validate_item_stack(
    reader: &StringReader<'_>,
    stack: &ItemStack,
) -> Result<(), CommandSyntaxError> {
    stack
        .validate_strict()
        .map_err(|error| malformed_item(reader, &error.to_string()))
}

fn expected_component(reader: &StringReader<'_>) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        (&translations::ARGUMENTS_ITEM_COMPONENT_EXPECTED).into(),
    )))
}

fn unknown_component(reader: &StringReader<'_>, key: &Identifier) -> CommandSyntaxError {
    let message = translations::ARGUMENTS_ITEM_COMPONENT_UNKNOWN
        .message([key.to_string()])
        .component();
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message)))
}

fn malformed_component(
    reader: &StringReader<'_>,
    key: &Identifier,
    error: &str,
) -> CommandSyntaxError {
    let message = translations::ARGUMENTS_ITEM_COMPONENT_MALFORMED
        .message([key.to_string(), error.to_owned()])
        .component();
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message)))
}

fn malformed_item(reader: &StringReader<'_>, error: &str) -> CommandSyntaxError {
    let message = translations::ARGUMENTS_ITEM_MALFORMED
        .message([error.to_owned()])
        .component();
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message)))
}

pub(super) fn suggest_item_stack(builder: &mut SuggestionsBuilder<'_>) {
    let input = builder.remaining();
    let Some(component_start) = input.find('[') else {
        suggest_items(input, builder);
        if item_by_input(input).is_some() {
            builder.suggest(format!("{input}["));
        }
        return;
    };
    if item_by_input(&input[..component_start]).is_none() {
        return;
    }

    let components = &input[component_start + 1..];
    let Some(current_start) = current_component_start(components) else {
        return;
    };
    let current = &components[current_start..];
    let trimmed = current.trim_start();
    let whitespace = current.len() - trimmed.len();
    let current = trimmed;
    let prefix = &input[..component_start + 1 + current_start + whitespace];

    if let Some(value_start) = current.find('=') {
        suggest_component_delimiters(input, &current[value_start + 1..], builder);
        return;
    }

    let (removed, component_prefix) = current
        .strip_prefix('!')
        .map_or((false, current), |prefix| (true, prefix));
    let component_key = component_prefix.trim_end();
    if component_key.len() != component_prefix.len() && component_key.is_empty() {
        return;
    }
    if component_by_input(component_key).is_some() {
        if removed {
            suggest_operation_delimiters(input, builder);
            return;
        }
        builder.suggest(format!("{input}="));
        return;
    }
    let visited = visited_component_keys(&components[..current_start]);
    for entry in
        (0..REGISTRY.data_components.len()).filter_map(|id| REGISTRY.data_components.by_id(id))
    {
        if !entry.is_persistent()
            || visited.contains(&entry.key)
            || !resource_matches(component_prefix, &entry.key)
        {
            continue;
        }
        let operation = if removed { "!" } else { "" };
        let suffix = if removed { "" } else { "=" };
        builder.suggest(format!("{prefix}{operation}{}{suffix}", entry.key));
    }
    if current.is_empty() {
        builder.suggest(format!("{prefix}!"));
    }
}

fn suggest_items(input: &str, builder: &mut SuggestionsBuilder<'_>) {
    for (_, item) in REGISTRY.items.iter() {
        if resource_matches(input, &item.key) {
            builder.suggest(item.key.to_string());
        }
    }
}

fn item_by_input(input: &str) -> Option<ItemRef> {
    let key = parse_identifier_text(input)?;
    REGISTRY.items.by_key(&key)
}

fn component_by_input(input: &str) -> Option<&'static ComponentEntry> {
    let key = parse_identifier_text(input)?;
    let entry = REGISTRY.data_components.by_key(&key)?;
    entry.is_persistent().then_some(entry)
}

fn parse_identifier_text(input: &str) -> Option<Identifier> {
    let mut reader = StringReader::new(input);
    let key = parse_identifier(&mut reader).ok()?;
    (!reader.can_read()).then_some(key)
}

fn resource_matches(pattern: &str, key: &Identifier) -> bool {
    let pattern = pattern.to_lowercase();
    if pattern.contains(':') {
        return matches_substring(&pattern, &key.to_string());
    }
    matches_substring(&pattern, key.namespace.as_ref())
        || matches_substring(&pattern, key.path.as_ref())
}

fn current_component_start(components: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut current_start = 0usize;

    for (index, character) in components.char_indices() {
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
            ',' if depth == 0 => current_start = index + character.len_utf8(),
            _ => {}
        }
    }
    Some(current_start)
}

fn visited_component_keys(components: &str) -> FxHashSet<Identifier> {
    components
        .split(',')
        .filter_map(|component| {
            let component = component.trim().trim_start_matches('!');
            let key = component.split_once('=').map_or(component, |(key, _)| key);
            parse_identifier_text(key.trim())
        })
        .collect()
}

fn suggest_component_delimiters(input: &str, value: &str, builder: &mut SuggestionsBuilder<'_>) {
    let Ok((_, consumed)) = parse_snbt_argument(value) else {
        return;
    };
    if value[consumed..].trim().is_empty() {
        suggest_operation_delimiters(input, builder);
    }
}

fn suggest_operation_delimiters(input: &str, builder: &mut SuggestionsBuilder<'_>) {
    builder.suggest(format!("{input},"));
    builder.suggest(format!("{input}]"));
}
