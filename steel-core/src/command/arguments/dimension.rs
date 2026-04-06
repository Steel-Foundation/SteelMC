//! Argument that resolves a dimension identifier to a loaded world.
//!
//! Accepts full identifiers (`minecraft:overworld`) and path-only shorthands
//! (`the_nether`). Shorthands are resolved against the namespace of the
//! sender's current world, so a player in `mymod:lobby` typing `arena`
//! resolves to `mymod:arena`.

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
pub struct DimensionArgument;

impl CommandArgument for DimensionArgument {
    type Output = Arc<World>;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = *arg.first()?;

        // Try as a full identifier first (e.g. "minecraft:the_nether")
        if let Some(world) = s
            .parse::<Identifier>()
            .ok()
            .and_then(|key| context.server.worlds.get(&key).cloned())
        {
            return Some((&arg[1..], world));
        }

        // Fall back to path-only shorthand using the sender's current namespace
        let ns = &context.world.dimension.key.namespace;
        let key = Identifier::new(ns.clone(), s.to_owned());
        let world = context.server.worlds.get(&key)?.clone();

        Some((&arg[1..], world))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Dimension, Some(SuggestionType::AskServer))
    }

    fn suggest(&self, prefix: &str, suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        let player_ns = &suggestion_ctx.world.dimension.key.namespace;

        let mut suggestions: Vec<SuggestionEntry> = suggestion_ctx
            .server
            .worlds
            .keys()
            .map(|id| SuggestionEntry::new(id.to_string()))
            .collect();

        // For worlds sharing the sender's namespace, also suggest the path-only shorthand
        for id in suggestion_ctx.server.worlds.keys() {
            if id.namespace == *player_ns {
                let path = id.path.as_ref();
                if !suggestions.iter().any(|s| s.text == path) {
                    suggestions.push(SuggestionEntry::new(path));
                }
            }
        }

        suggestions.retain(|s| s.text.starts_with(prefix));
        suggestions
    }
}
