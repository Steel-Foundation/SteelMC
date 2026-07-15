//! Block-state and block-entity predicates used by commands.

use simdnbt::owned::NbtCompound;
use steel_registry::{
    BLOCKS_REGISTRY, REGISTRY, RegistryExt as _, TaggedRegistryExt as _, blocks::BlockRef,
};
use steel_utils::{BlockStateId, Identifier, nbt::parse_snbt_compound_argument};
use text_components::TextComponent;

use super::argument::{matches_substring, parse_identifier, unknown_resource};
use crate::command::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, StringReader, SuggestionsBuilder,
};

type BlockProperties = Vec<(Box<str>, Box<str>)>;

/// A concrete block or block tag with optional state and block-entity constraints.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BlockPredicate {
    Block {
        block: BlockRef,
        properties: BlockProperties,
        nbt: Option<NbtCompound>,
    },
    Tag {
        tag: Identifier,
        properties: BlockProperties,
        nbt: Option<NbtCompound>,
    },
}

impl BlockPredicate {
    pub(crate) fn matches_state(&self, state: BlockStateId) -> bool {
        let Some(actual) = REGISTRY.blocks.by_state_id(state) else {
            return false;
        };
        let properties = match self {
            Self::Block {
                block, properties, ..
            } => {
                if actual != *block {
                    return false;
                }
                properties
            }
            Self::Tag {
                tag, properties, ..
            } => {
                if !actual.has_tag(tag) {
                    return false;
                }
                properties
            }
        };
        state_properties_match(state, properties)
    }

    pub(crate) const fn nbt(&self) -> Option<&NbtCompound> {
        match self {
            Self::Block { nbt, .. } | Self::Tag { nbt, .. } => nbt.as_ref(),
        }
    }
}

fn state_properties_match(state: BlockStateId, expected: &BlockProperties) -> bool {
    let actual = REGISTRY.blocks.get_properties(state);
    expected.iter().all(|(name, value)| {
        actual.iter().any(|(actual_name, actual_value)| {
            *actual_name == name.as_ref() && *actual_value == value.as_ref()
        })
    })
}

pub(super) fn parse_block_predicate(
    reader: &mut StringReader<'_>,
) -> Result<BlockPredicate, CommandSyntaxError> {
    if reader.peek() == Some('#') {
        reader.skip();
        return parse_tag_predicate(reader);
    }
    parse_concrete_block_predicate(reader)
}

fn parse_concrete_block_predicate(
    reader: &mut StringReader<'_>,
) -> Result<BlockPredicate, CommandSyntaxError> {
    let key = parse_identifier(reader)?;
    let Some(block) = REGISTRY.blocks.by_key(&key) else {
        return Err(unknown_resource(reader, &key, &BLOCKS_REGISTRY));
    };
    let properties = if reader.peek() == Some('[') {
        parse_properties(reader, Some(block))?
    } else {
        Vec::new()
    };
    let nbt = parse_optional_nbt(reader)?;
    Ok(BlockPredicate::Block {
        block,
        properties,
        nbt,
    })
}

fn parse_tag_predicate(
    reader: &mut StringReader<'_>,
) -> Result<BlockPredicate, CommandSyntaxError> {
    let key = parse_identifier(reader)?;
    if !REGISTRY.blocks.tag_keys().any(|tag| tag == &key) {
        return Err(dynamic_error(reader, format!("Unknown block tag '#{key}'")));
    }
    let properties = if reader.peek() == Some('[') {
        parse_properties(reader, None)?
    } else {
        Vec::new()
    };
    let nbt = parse_optional_nbt(reader)?;
    Ok(BlockPredicate::Tag {
        tag: key,
        properties,
        nbt,
    })
}

fn parse_properties(
    reader: &mut StringReader<'_>,
    block: Option<BlockRef>,
) -> Result<BlockProperties, CommandSyntaxError> {
    reader.expect('[')?;
    reader.skip_whitespace();
    let mut properties = BlockProperties::new();

    while reader.can_read() && reader.peek() != Some(']') {
        reader.skip_whitespace();
        let key = reader.read_string()?;
        if key.is_empty() {
            return Err(dynamic_error(reader, "Expected block property name"));
        }
        if properties
            .iter()
            .any(|(existing, _)| existing.as_ref() == key)
        {
            return Err(dynamic_error(
                reader,
                format!("Duplicate block property '{key}'"),
            ));
        }
        let property = block.and_then(|block| {
            block
                .properties
                .iter()
                .copied()
                .find(|property| property.get_name() == key)
        });
        if block.is_some() && property.is_none() {
            return Err(dynamic_error(
                reader,
                format!("Unknown property '{key}' for block predicate"),
            ));
        }

        reader.skip_whitespace();
        reader.expect('=')?;
        reader.skip_whitespace();
        let value = reader.read_string()?;
        if let Some(property) = property
            && !property
                .get_possible_value_names()
                .contains(&value.as_str())
        {
            return Err(dynamic_error(
                reader,
                format!("Invalid value '{value}' for block property '{key}'"),
            ));
        }
        properties.push((key.into(), value.into()));

        reader.skip_whitespace();
        match reader.peek() {
            Some(',') => {
                reader.skip();
            }
            Some(']') => {}
            _ => return Err(dynamic_error(reader, "Expected ',' or ']'")),
        }
    }

    reader.expect(']')?;
    Ok(properties)
}

fn parse_optional_nbt(
    reader: &mut StringReader<'_>,
) -> Result<Option<NbtCompound>, CommandSyntaxError> {
    if reader.peek() != Some('{') {
        return Ok(None);
    }
    let parsed = parse_snbt_compound_argument(reader.remaining());
    let (nbt, consumed) = match parsed {
        Ok(value) => value,
        Err(error) => {
            if !reader.advance_bytes(error.cursor()) {
                return Err(dynamic_error(reader, "Invalid block entity NBT cursor"));
            }
            return Err(dynamic_error(reader, error.component()));
        }
    };
    if !reader.advance_bytes(consumed) {
        return Err(dynamic_error(reader, "Invalid block entity NBT cursor"));
    }
    Ok(Some(nbt))
}

fn dynamic_error(
    reader: &StringReader<'_>,
    message: impl Into<TextComponent>,
) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message.into())))
}

pub(super) fn suggest_blocks(builder: &mut SuggestionsBuilder<'_>) {
    let remaining = builder.remaining_lowercase().to_owned();
    if remaining.contains(['[', '{']) {
        return;
    }
    if let Some(prefix) = remaining.strip_prefix('#') {
        for tag in REGISTRY
            .blocks
            .tag_keys()
            .filter(|tag| identifier_matches(prefix, tag))
        {
            builder.suggest(format!("#{tag}"));
        }
        return;
    }
    for block in REGISTRY
        .blocks
        .iter()
        .map(|(_, block)| &block.key)
        .filter(|key| identifier_matches(&remaining, key))
    {
        builder.suggest(block.to_string());
    }
    for tag in REGISTRY
        .blocks
        .tag_keys()
        .filter(|tag| identifier_matches(&remaining, tag))
    {
        builder.suggest(format!("#{tag}"));
    }
}

fn identifier_matches(pattern: &str, identifier: &Identifier) -> bool {
    if pattern.contains(':') {
        matches_substring(pattern, &identifier.to_string())
    } else {
        matches_substring(pattern, identifier.namespace.as_ref())
            || matches_substring(pattern, identifier.path.as_ref())
    }
}
