//! A text argument.
use crate::command::arguments::CommandArgument;
use crate::command::context::CommandContext;
use steel_protocol::packets::game::{ArgumentType, SuggestionType};
use text_components::TextComponent;

/// A text argument.
pub struct TextComponentArgument;

impl CommandArgument for TextComponentArgument {
    type Output = TextComponent;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        match TextComponent::from_snbt(&arg.join(" ")) {
            Ok(component) => Some((&[], component)),
            Err(e) => {
                log::warn!("{e}");
                None
            }
        }
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Component, None)
    }
}
