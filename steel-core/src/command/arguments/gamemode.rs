//! A gamemode argument.
use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};
use steel_utils::types::GameType;

use crate::command::arguments::CommandArgument;
use crate::command::context::CommandContext;

/// A gamemode argument.
pub struct GameModeArgument;

impl CommandArgument for GameModeArgument {
    type Output = GameType;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = arg.first()?;

        let gamemode = match s.to_lowercase().as_str() {
            "survival" | "0" => GameType::Survival,
            "creative" | "1" => GameType::Creative,
            "adventure" | "2" => GameType::Adventure,
            "spectator" | "3" => GameType::Spectator,
            _ => return None,
        };

        Some((&arg[1..], gamemode))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Gamemode, None)
    }

    /// ONLY FOR THE CONSOLE\
    /// (If you want to also suggest to the client,
    /// put the `SuggestionType` to `AskServer`)
    fn suggest(
        &self,
        prefix: &str,
        _suggestion_ctx: &super::SuggestionContext,
    ) -> Vec<SuggestionEntry> {
        let mut suggestions = vec![
            SuggestionEntry {
                text: String::from("survival"),
                tooltip: None,
            },
            SuggestionEntry {
                text: String::from("creative"),
                tooltip: None,
            },
            SuggestionEntry {
                text: String::from("adventure"),
                tooltip: None,
            },
            SuggestionEntry {
                text: String::from("spectator"),
                tooltip: None,
            },
        ];

        suggestions.retain(|s| s.text.starts_with(prefix));
        suggestions
    }
}
