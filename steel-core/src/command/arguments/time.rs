//! A time argument.
use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};

use crate::command::arguments::CommandArgument;
use crate::command::context::CommandContext;

/// A time argument.
pub struct TimeArgument;

impl CommandArgument for TimeArgument {
    type Output = i32;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = arg.first()?;

        let (number, unit) = s
            .find(|c: char| c.is_alphabetic())
            .map_or((*s, "t"), |pos| (&s[..pos], &s[pos..]));

        let number = number.parse::<f32>().ok()?;
        if number < 0.0 {
            return None;
        }

        let ticks = match unit {
            "d" => number * 24000.0,
            "s" => number * 20.0,
            "t" => number,
            _ => return None,
        };

        Some((&arg[1..], ticks.round() as i32))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Time { min: 0 }, None)
    }

    /// ONLY FOR THE CONSOLE\
    /// (If you want to also suggest to the client,
    /// put the `SuggestionType` to `AskServer`)
    fn suggest(
        &self,
        prefix: &str,
        _suggestion_ctx: &super::SuggestionContext,
    ) -> Vec<SuggestionEntry> {
        // Check if prefix already has a unit suffix
        let has_unit = prefix.chars().any(char::is_alphabetic);
        if !prefix.is_empty() && !has_unit {
            return vec![
                SuggestionEntry {
                    text: format!("{prefix}d"),
                    tooltip: None,
                },
                SuggestionEntry {
                    text: format!("{prefix}s"),
                    tooltip: None,
                },
                SuggestionEntry {
                    text: format!("{prefix}t"),
                    tooltip: None,
                },
            ];
        }
        vec![]
    }
}
