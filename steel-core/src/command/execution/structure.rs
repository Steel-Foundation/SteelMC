//! Structure registry key command arguments.

use steel_registry::{REGISTRY, RegistryExt as _, TaggedRegistryExt as _, structure::StructureRef};
use steel_utils::Identifier;

use super::argument::{identifier_matches, parse_identifier};
use crate::command::brigadier::{CommandSyntaxError, StringReader, SuggestionsBuilder};

/// A structure resource key or tag key retained until command execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StructureOrTagKey {
    Structure(Identifier),
    Tag(Identifier),
}

impl StructureOrTagKey {
    pub(crate) fn resolve(&self) -> Option<Vec<StructureRef>> {
        match self {
            Self::Structure(key) => REGISTRY
                .structures
                .by_key(key)
                .map(|structure| vec![structure]),
            Self::Tag(key) => REGISTRY.structures.get_tag(key),
        }
    }

    pub(crate) fn as_printable(&self) -> String {
        match self {
            Self::Structure(key) => key.to_string(),
            Self::Tag(key) => format!("#{key}"),
        }
    }

    pub(crate) fn found_name(&self, found_structure: &Identifier) -> String {
        match self {
            Self::Structure(key) => key.to_string(),
            Self::Tag(key) => format!("#{key} ({found_structure})"),
        }
    }
}

pub(super) fn parse_structure_or_tag_key(
    reader: &mut StringReader<'_>,
) -> Result<StructureOrTagKey, CommandSyntaxError> {
    if reader.peek() != Some('#') {
        return parse_identifier(reader).map(StructureOrTagKey::Structure);
    }

    let start = reader.checkpoint();
    reader.skip();
    match parse_identifier(reader) {
        Ok(key) => Ok(StructureOrTagKey::Tag(key)),
        Err(error) => {
            reader.restore(start);
            Err(error)
        }
    }
}

pub(super) fn suggest_structures(builder: &mut SuggestionsBuilder<'_>) {
    let remaining = builder.remaining_lowercase();
    let suggestions = if let Some(tag_prefix) = remaining.strip_prefix('#') {
        REGISTRY
            .structures
            .tag_keys()
            .filter(|tag| identifier_matches(tag_prefix, tag))
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
    } else {
        REGISTRY
            .structures
            .iter()
            .filter(|(_, structure)| identifier_matches(remaining, &structure.key))
            .map(|(_, structure)| structure.key.to_string())
            .chain(
                REGISTRY
                    .structures
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
