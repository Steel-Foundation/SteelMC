//! Biome registry command arguments.

use steel_registry::{
    BIOMES_REGISTRY, REGISTRY, RegistryExt as _, TaggedRegistryExt as _, biome::BiomeRef,
};
use steel_utils::{Identifier, translations};

use super::argument::{identifier_matches, parse_identifier, unknown_resource};
use crate::command::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, StringReader, SuggestionsBuilder,
};

/// A registered biome or biome tag retained by a command argument.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BiomeOrTag {
    Biome(BiomeRef),
    Tag(Identifier),
}

impl BiomeOrTag {
    pub(crate) fn matches(&self, biome: BiomeRef) -> bool {
        match self {
            Self::Biome(expected) => *expected == biome,
            Self::Tag(tag) => biome.has_tag(tag),
        }
    }
}

pub(super) fn parse_biome_or_tag(
    reader: &mut StringReader<'_>,
) -> Result<BiomeOrTag, CommandSyntaxError> {
    if reader.peek() != Some('#') {
        let key = parse_identifier(reader)?;
        return REGISTRY.biomes.by_key(&key).map_or_else(
            || Err(unknown_resource(reader, &key, &BIOMES_REGISTRY)),
            |biome| Ok(BiomeOrTag::Biome(biome)),
        );
    }

    let start = reader.checkpoint();
    reader.skip();
    let key = match parse_identifier(reader) {
        Ok(key) => key,
        Err(error) => {
            reader.restore(start);
            return Err(error);
        }
    };
    if REGISTRY.biomes.tag_keys().any(|tag| tag == &key) {
        return Ok(BiomeOrTag::Tag(key));
    }

    let message = translations::ARGUMENT_RESOURCE_TAG_NOT_FOUND
        .message([key.to_string(), BIOMES_REGISTRY.to_string()])
        .component();
    let error = reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message)));
    reader.restore(start);
    Err(error)
}

pub(super) fn suggest_biomes(builder: &mut SuggestionsBuilder<'_>) {
    let remaining = builder.remaining_lowercase();
    let suggestions = if let Some(tag_prefix) = remaining.strip_prefix('#') {
        REGISTRY
            .biomes
            .tag_keys()
            .filter(|tag| identifier_matches(tag_prefix, tag))
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
    } else {
        REGISTRY
            .biomes
            .iter()
            .filter(|(_, biome)| identifier_matches(remaining, &biome.key))
            .map(|(_, biome)| biome.key.to_string())
            .chain(
                REGISTRY
                    .biomes
                    .tag_keys()
                    .filter(|tag| identifier_matches(remaining, tag))
                    .map(|tag| format!("#{tag}")),
            )
            .collect()
    };
    for suggestion in suggestions {
        builder.suggest(suggestion);
    }
}
