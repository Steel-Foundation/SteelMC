//! Block-state and block-entity predicates used by commands.

mod suggestions;

use std::sync::Arc;

pub(super) use self::suggestions::{suggest_block_inputs, suggest_blocks};
use super::argument::parse_identifier;
use crate::{
    command::brigadier::{CommandSyntaxError, CommandSyntaxErrorKind, StringReader},
    world::World,
};
use simdnbt::owned::NbtCompound;
use steel_registry::{
    REGISTRY, RegistryExt as _, TaggedRegistryExt as _,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
};
use steel_utils::{
    BlockPos, BlockStateId, Identifier,
    nbt::{compare_nbt_compounds, nbt_compounds_equal, parse_snbt_compound_argument},
    translations,
    types::UpdateFlags,
};
use text_components::TextComponent;

type BlockProperties = Vec<(Box<str>, Box<str>)>;

/// A parsed concrete block state and optional block-entity data.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlockInput {
    state: BlockStateId,
    properties: BlockProperties,
    nbt: Option<NbtCompound>,
}

impl BlockInput {
    pub(crate) const fn from_state(state: BlockStateId) -> Self {
        Self {
            state,
            properties: Vec::new(),
            nbt: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn state(&self) -> BlockStateId {
        self.state
    }

    #[cfg(test)]
    pub(crate) const fn nbt(&self) -> Option<&NbtCompound> {
        self.nbt.as_ref()
    }

    /// Places this input with Vanilla's command block-state semantics.
    pub(crate) fn place(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        flags: UpdateFlags,
    ) -> Result<bool, simdnbt::Error> {
        let mut state = if flags.contains(UpdateFlags::UPDATE_KNOWN_SHAPE) {
            self.state
        } else {
            world.update_from_neighbor_shapes(self.state, pos)
        };
        if state.is_air() {
            state = self.state;
        }
        for (name, value) in &self.properties {
            state = REGISTRY
                .blocks
                .try_set_property_by_name(state, name, value)
                .unwrap_or(state);
        }

        let mut affected = world.set_block(pos, state, flags);
        if let Some(nbt) = &self.nbt
            && let Some(block_entity) = world.get_block_entity(pos)
        {
            let before = block_entity.save_without_metadata();
            block_entity.load_with_owned_components(nbt)?;
            let after = block_entity.save_without_metadata();
            if !nbt_compounds_equal(&before, &after) {
                affected = true;
                block_entity.set_changed();
                world.send_block_updated(pos);
            }
        }
        Ok(affected)
    }
}

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
    pub(crate) fn matches(&self, world: &World, pos: BlockPos) -> bool {
        if !self.matches_state(world.get_block_state(pos)) {
            return false;
        }
        let Some(expected_nbt) = self.nbt() else {
            return true;
        };
        let Some(block_entity) = world.get_block_entity(pos) else {
            return false;
        };
        compare_nbt_compounds(expected_nbt, &block_entity.save_with_full_metadata(), true)
    }
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
        return parse_tag_predicate(reader);
    }
    parse_concrete_block_predicate(reader)
}

fn parse_concrete_block_predicate(
    reader: &mut StringReader<'_>,
) -> Result<BlockPredicate, CommandSyntaxError> {
    let parsed = parse_concrete_block(reader)?;
    Ok(BlockPredicate::Block {
        block: parsed.block,
        properties: parsed.properties,
        nbt: parsed.nbt,
    })
}

pub(super) fn parse_block_input(
    reader: &mut StringReader<'_>,
) -> Result<BlockInput, CommandSyntaxError> {
    if reader.peek() == Some('#') {
        return Err(dynamic_error(
            reader,
            TextComponent::from(&translations::ARGUMENT_BLOCK_TAG_DISALLOWED),
        ));
    }
    let parsed = parse_concrete_block(reader)?;
    let properties = parsed
        .properties
        .iter()
        .map(|(name, value)| (name.as_ref(), value.as_ref()));
    let Some(state) = REGISTRY
        .blocks
        .state_id_from_block_defaulted_properties(parsed.block, properties)
    else {
        return Err(dynamic_error(
            reader,
            "Parsed block properties did not resolve to a registered state",
        ));
    };
    Ok(BlockInput {
        state,
        properties: parsed.properties,
        nbt: parsed.nbt,
    })
}

struct ParsedConcreteBlock {
    block: BlockRef,
    properties: BlockProperties,
    nbt: Option<NbtCompound>,
}

fn parse_concrete_block(
    reader: &mut StringReader<'_>,
) -> Result<ParsedConcreteBlock, CommandSyntaxError> {
    let start = reader.checkpoint();
    let key = parse_identifier(reader)?;
    let Some(block) = REGISTRY.blocks.by_key(&key) else {
        reader.restore(start);
        return Err(unknown_block(reader, &key));
    };
    let properties = if reader.peek() == Some('[') {
        parse_properties(reader, Some(block), &key.to_string())?
    } else {
        Vec::new()
    };
    let nbt = parse_optional_nbt(reader)?;
    Ok(ParsedConcreteBlock {
        block,
        properties,
        nbt,
    })
}

fn parse_tag_predicate(
    reader: &mut StringReader<'_>,
) -> Result<BlockPredicate, CommandSyntaxError> {
    let start = reader.checkpoint();
    reader.skip();
    let key = parse_identifier(reader)?;
    if !REGISTRY.blocks.tag_keys().any(|tag| tag == &key) {
        reader.restore(start);
        return Err(unknown_block_tag(reader, &key));
    }
    let properties = if reader.peek() == Some('[') {
        parse_properties(reader, None, "minecraft:")?
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
    block_name: &str,
) -> Result<BlockProperties, CommandSyntaxError> {
    reader.expect('[')?;
    reader.skip_whitespace();
    let mut properties = BlockProperties::new();
    let mut vague_value_start = None;

    while reader.can_read() && reader.peek() != Some(']') {
        reader.skip_whitespace();
        let key_start = reader.checkpoint();
        let key = reader.read_string()?;
        if properties
            .iter()
            .any(|(existing, _)| existing.as_ref() == key)
        {
            reader.restore(key_start);
            return Err(duplicate_property(reader, block_name, &key));
        }
        let property = block.and_then(|block| {
            block
                .properties
                .iter()
                .copied()
                .find(|property| property.get_name() == key)
        });
        if block.is_some() && property.is_none() {
            reader.restore(key_start);
            return Err(unknown_property(reader, block_name, &key));
        }

        reader.skip_whitespace();
        if reader.peek() != Some('=') {
            if block.is_none() {
                reader.restore(key_start);
            }
            return Err(expected_property_value(reader, block_name, &key));
        }
        reader.skip();
        reader.skip_whitespace();
        let value_start = reader.checkpoint();
        let value = reader.read_string()?;
        if let Some(property) = property
            && !property
                .get_possible_value_names()
                .contains(&value.as_str())
        {
            reader.restore(value_start);
            return Err(invalid_property_value(reader, block_name, &key, &value));
        }
        vague_value_start = block.is_none().then_some(value_start);
        properties.push((key.into(), value.into()));

        reader.skip_whitespace();
        match reader.peek() {
            Some(',') => {
                reader.skip();
                vague_value_start = None;
            }
            Some(']') => {}
            Some(_) => return Err(unclosed_properties(reader)),
            None => break,
        }
    }

    if reader.peek() != Some(']') {
        if let Some(value_start) = vague_value_start {
            reader.restore(value_start);
        }
        return Err(unclosed_properties(reader));
    }
    reader.skip();
    Ok(properties)
}

fn unknown_block(reader: &StringReader<'_>, key: &Identifier) -> CommandSyntaxError {
    let message = translations::ARGUMENT_BLOCK_ID_INVALID
        .message([key.to_string()])
        .component();
    dynamic_error(reader, message)
}

fn unknown_block_tag(reader: &StringReader<'_>, key: &Identifier) -> CommandSyntaxError {
    let message = translations::ARGUMENTS_BLOCK_TAG_UNKNOWN
        .message([key.to_string()])
        .component();
    dynamic_error(reader, message)
}

fn unknown_property(reader: &StringReader<'_>, block: &str, property: &str) -> CommandSyntaxError {
    let message = translations::ARGUMENT_BLOCK_PROPERTY_UNKNOWN
        .message([block.to_owned(), property.to_owned()])
        .component();
    dynamic_error(reader, message)
}

fn duplicate_property(
    reader: &StringReader<'_>,
    block: &str,
    property: &str,
) -> CommandSyntaxError {
    let message = translations::ARGUMENT_BLOCK_PROPERTY_DUPLICATE
        .message([property.to_owned(), block.to_owned()])
        .component();
    dynamic_error(reader, message)
}

fn invalid_property_value(
    reader: &StringReader<'_>,
    block: &str,
    property: &str,
    value: &str,
) -> CommandSyntaxError {
    let message = translations::ARGUMENT_BLOCK_PROPERTY_INVALID
        .message([block.to_owned(), value.to_owned(), property.to_owned()])
        .component();
    dynamic_error(reader, message)
}

fn expected_property_value(
    reader: &StringReader<'_>,
    block: &str,
    property: &str,
) -> CommandSyntaxError {
    let message = translations::ARGUMENT_BLOCK_PROPERTY_NOVALUE
        .message([property.to_owned(), block.to_owned()])
        .component();
    dynamic_error(reader, message)
}

fn unclosed_properties(reader: &StringReader<'_>) -> CommandSyntaxError {
    dynamic_error(
        reader,
        TextComponent::from(&translations::ARGUMENT_BLOCK_PROPERTY_UNCLOSED),
    )
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
