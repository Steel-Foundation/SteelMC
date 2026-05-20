//! Argument that resolves a configured domain name.

use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};

use crate::command::{
    arguments::{CommandArgument, SuggestionContext},
    context::CommandContext,
};

/// Parses a domain name.
pub struct DomainArgument;

impl CommandArgument for DomainArgument {
    type Output = String;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let domain = *arg.first()?;
        if !context.server.worlds.has_domain(domain) {
            return None;
        }
        Some((&arg[1..], domain.to_owned()))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (
            ArgumentType::ResourceLocation,
            Some(SuggestionType::AskServer),
        )
    }

    fn suggest(&self, prefix: &str, suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        suggestion_ctx
            .server
            .worlds
            .domain_names()
            .filter(|domain| domain.starts_with(prefix))
            .map(SuggestionEntry::new)
            .collect()
    }
}
