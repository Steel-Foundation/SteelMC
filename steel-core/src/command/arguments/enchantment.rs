//! An enchantment argument
use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};
use steel_registry::{REGISTRY, RegistryExt, enchantment::EnchantmentRef};
use steel_utils::Identifier;

use crate::command::{
    arguments::{CommandArgument, SuggestionContext},
    context::CommandContext,
};

/// An enchantment argument that resolves to an `EnchantmentRef`.
pub struct EnchantmentArgument;

impl CommandArgument for EnchantmentArgument {
    type Output = EnchantmentRef;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = arg.first()?;
        let key = s.strip_prefix("minecraft:").unwrap_or(s).to_owned();

        REGISTRY
            .enchantments
            .by_key(&Identifier::vanilla(key))
            .map(|e| (&arg[1..], e))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (
            ArgumentType::Resource {
                identifier: "minecraft:enchantment",
            },
            Some(SuggestionType::AskServer),
        )
    }

    fn suggest(&self, prefix: &str, _suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        let stripped_prefix = prefix.strip_prefix("minecraft:").unwrap_or(prefix);
        REGISTRY
            .enchantments
            .iter()
            .map(|(_, e)| SuggestionEntry::new(e.key.to_string()))
            .filter(|s| {
                s.text
                    .strip_prefix("minecraft:")
                    .unwrap_or(&s.text)
                    .starts_with(stripped_prefix)
            })
            .collect()
    }
}
