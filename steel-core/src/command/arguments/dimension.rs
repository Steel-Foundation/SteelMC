//! Argument that resolves a dimension identifier to a loaded world.

use std::sync::Arc;

use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};
use steel_utils::Identifier;

use crate::{
    command::{
        arguments::{CommandArgument, SuggestionContext},
        context::CommandContext,
    },
    world::World,
};

/// Parses a dimension argument into a loaded [`World`].
///
/// Accepts shorthand aliases (`overworld`, `nether`, `end`) as well as full identifiers
/// (e.g. `minecraft:overworld`). Suggestions list all currently loaded worlds by identifier.
pub struct DimensionArgument;

/// Maps shorthand aliases to their full Minecraft identifiers.
fn resolve_alias(s: &str) -> Option<Identifier> {
    match s {
        "overworld" => Some(Identifier::vanilla_static("overworld")),
        "nether" => Some(Identifier::vanilla_static("the_nether")),
        "end" => Some(Identifier::vanilla_static("the_end")),
        _ => None,
    }
}

impl CommandArgument for DimensionArgument {
    type Output = Arc<World>;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = *arg.first()?;

        let key = resolve_alias(s).or_else(|| s.parse().ok())?;
        let world = context.server.worlds.get(&key)?.clone();

        Some((&arg[1..], world))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Dimension, Some(SuggestionType::AskServer))
    }

    fn suggest(&self, prefix: &str, suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        let mut suggestions: Vec<SuggestionEntry> = suggestion_ctx
            .server
            .worlds
            .keys()
            .map(|id| SuggestionEntry::new(id.to_string()))
            .collect();

        // Add shorthand aliases for vanilla dimensions
        for alias in ["overworld", "nether", "end"] {
            suggestions.push(SuggestionEntry::new(alias));
        }

        suggestions.retain(|s| s.text.starts_with(prefix));
        suggestions
    }
}
