//! A boolean argument.
use steel_protocol::packets::game::{ArgumentType, SuggestionType};

use crate::command::arguments::CommandArgument;
use crate::command::context::CommandContext;

/// A boolean argument that parses "true" or "false".
pub struct BoolArgument;

impl CommandArgument for BoolArgument {
    type Output = bool;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = arg.first()?;

        let value = match s.to_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => return None,
        };

        Some((&arg[1..], value))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Bool, None)
    }
}
