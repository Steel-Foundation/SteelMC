//! A structure argument.

use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};
use steel_registry::TaggedRegistryExt;
use steel_registry::{REGISTRY, RegistryExt, structure::StructureRef};
use steel_utils::Identifier;

use crate::command::{
    arguments::{CommandArgument, SuggestionContext},
    context::CommandContext,
};

/// A structure argument that resolves to either a structure or structure tag.
pub struct StructureArgument;

/// Structure command argument value: either one structure or a structure tag.
pub enum StructureArgumentValue {
    /// A single structure key.
    Structure(StructureRef),
    /// A structure tag and its resolved entries.
    Tag {
        /// Tag key without the leading `#`.
        key: Identifier,
        /// Structures in the tag.
        structures: Vec<StructureRef>,
    },
}

impl StructureArgumentValue {
    /// Structure keys to scan.
    #[must_use]
    pub fn structure_keys(&self) -> Vec<Identifier> {
        match self {
            Self::Structure(structure) => vec![structure.key.clone()],
            Self::Tag { structures, .. } => structures
                .iter()
                .map(|structure| structure.key.clone())
                .collect(),
        }
    }

    /// Printable command target name.
    #[must_use]
    pub fn printable_name(&self, found_structure: &Identifier) -> String {
        match self {
            Self::Structure(structure) => structure.key.to_string(),
            Self::Tag { key, .. } => format!("#{key} ({found_structure})"),
        }
    }

    /// Printable command target without resolved found entry.
    #[must_use]
    pub fn query_name(&self) -> String {
        match self {
            Self::Structure(structure) => structure.key.to_string(),
            Self::Tag { key, .. } => format!("#{key}"),
        }
    }
}

impl CommandArgument for StructureArgument {
    type Output = StructureArgumentValue;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = arg.first()?;
        if let Some(tag) = s.strip_prefix('#') {
            let key = parse_identifier(tag)?;
            let structures = REGISTRY.structures.get_tag(&key)?;
            if structures.is_empty() {
                return None;
            }
            return Some((&arg[1..], StructureArgumentValue::Tag { key, structures }));
        }

        let key = parse_identifier(s)?;

        REGISTRY
            .structures
            .by_key(&key)
            .map(|structure| (&arg[1..], StructureArgumentValue::Structure(structure)))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (
            ArgumentType::ResourceOrTagKey {
                identifier: "minecraft:worldgen/structure",
            },
            Some(SuggestionType::AskServer),
        )
    }

    fn suggest(&self, prefix: &str, _suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        let mut suggestions = Vec::new();
        if prefix.starts_with('#') {
            let stripped_prefix = prefix
                .strip_prefix("#minecraft:")
                .or_else(|| prefix.strip_prefix('#'))
                .unwrap_or(prefix);
            suggestions.extend(REGISTRY.structures.tag_keys().filter_map(|key| {
                let key = key.to_string();
                let text = key.strip_prefix("minecraft:").unwrap_or(&key);
                text.starts_with(stripped_prefix)
                    .then(|| SuggestionEntry::new(format!("#{key}")))
            }));
            return suggestions;
        }

        let stripped_prefix = prefix.strip_prefix("minecraft:").unwrap_or(prefix);
        suggestions.extend(
            REGISTRY
                .structures
                .iter()
                .map(|(_, structure)| SuggestionEntry::new(structure.key.to_string()))
                .filter(|suggestion| {
                    suggestion
                        .text
                        .strip_prefix("minecraft:")
                        .unwrap_or(&suggestion.text)
                        .starts_with(stripped_prefix)
                }),
        );
        suggestions.extend(
            REGISTRY
                .structures
                .tag_keys()
                .map(|key| SuggestionEntry::new(format!("#{key}")))
                .filter(|suggestion| {
                    suggestion
                        .text
                        .strip_prefix("#minecraft:")
                        .unwrap_or(&suggestion.text)
                        .starts_with(stripped_prefix)
                }),
        );
        suggestions
    }
}

fn parse_identifier(s: &str) -> Option<Identifier> {
    let (namespace, path) = s
        .split_once(':')
        .unwrap_or((Identifier::VANILLA_NAMESPACE, s));
    Identifier::validate(namespace, path)
        .then(|| Identifier::new(namespace.to_owned(), path.to_owned()))
}
