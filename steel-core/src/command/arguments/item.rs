//! An item argument
use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};
use steel_registry::{REGISTRY, items::ItemRef};
use steel_utils::Identifier;

use crate::command::{
    arguments::{CommandArgument, SuggestionContext},
    context::CommandContext,
};

/// An item stack argument
pub struct ItemStackArgument;

impl CommandArgument for ItemStackArgument {
    type Output = ItemRef;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        if arg.is_empty() {
            return None;
        }
        let key = arg[0]
            .strip_prefix("minecraft:")
            .unwrap_or(arg[0])
            .to_owned();

        // TODO: Also read snbt data for custom item components

        REGISTRY
            .items
            .by_key(&Identifier::vanilla(key))
            .map(|it| (&arg[1..], it))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::ItemStack, Some(SuggestionType::AskServer))
    }

    fn suggest(&self, prefix: &str, _suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        let mut suggestions: Vec<SuggestionEntry> = REGISTRY
            .items
            .iter()
            .map(|it| SuggestionEntry::new(it.1.key.to_string()))
            .collect();

        suggestions.retain(|s| {
            s.text
                .strip_prefix("minecraft:")
                .unwrap_or(&s.text)
                .starts_with(prefix.strip_prefix("minecraft:").unwrap_or(prefix))
        });
        suggestions
    }
}
