//! This module contains types and utilities for parsing command arguments.
pub mod anchor;
pub mod bool;
pub mod gamemode;
pub mod integer;
pub mod rotation;
pub mod time;
pub mod vector2;
pub mod vector3;

use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};

use crate::command::context::CommandContext;

/// Context passed to suggestion methods containing previously parsed arguments.
#[derive(Default, Clone, Debug)]
pub struct SuggestionContext {
    /// Previously parsed argument values stored by name.
    /// Used for context-dependent suggestions (e.g., gamerule value depends on rule type).
    parsed_values: Vec<(&'static str, ParsedValue)>,
}

/// A parsed value that can be stored in suggestion context.
#[derive(Clone, Debug)]
pub enum ParsedValue {
    /// A string value (e.g., game rule name).
    String(String),
    /// A boolean value.
    Bool(bool),
    /// An integer value.
    Int(i32),
}

impl SuggestionContext {
    /// Creates a new empty suggestion context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a parsed value with its argument name.
    pub fn set(&mut self, name: &'static str, value: ParsedValue) {
        self.parsed_values.push((name, value));
    }

    /// Gets a parsed string value by argument name.
    #[must_use]
    pub fn get_string(&self, name: &str) -> Option<&str> {
        self.parsed_values.iter().find_map(|(n, v)| {
            if *n == name
                && let ParsedValue::String(s) = v
            {
                return Some(s.as_str());
            }
            None
        })
    }
}

/// A trait that defines a command argument parser.
pub trait CommandArgument: Send + Sync {
    /// The type of the parsed output.
    type Output;

    /// Parses from the given arguments the expected type and returns the remaining unconsumed arguments and the parsed output.
    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)>;

    /// Returns the parser ID associated with this argument.
    fn usage(&self) -> (ArgumentType, Option<SuggestionType>);

    /// Returns suggestions for this argument based on the current input prefix.
    /// Only needs to be implemented for arguments using `SuggestionType::AskServer`.
    /// `prefix` is the partial text being typed for this argument.
    /// `suggestion_ctx` contains previously parsed arguments for context-dependent suggestions.
    /// Default implementation returns no suggestions.
    fn suggest(&self, _prefix: &str, _suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        Vec::new()
    }

    /// Returns the value to store in `SuggestionContext` after parsing.
    /// This allows downstream arguments to make context-dependent suggestions.
    /// Returns `None` by default (don't store anything).
    fn parsed_value(&self, _args: &[&str], _context: &mut CommandContext) -> Option<ParsedValue> {
        None
    }
}

struct Helper;

impl Helper {
    pub fn parse_relative_coordinate<const IS_Y: bool>(
        s: &str,
        origin: Option<f64>,
    ) -> Option<f64> {
        if let Some(s) = s.strip_prefix('~') {
            let origin = origin?;
            let offset = if s.is_empty() { 0.0 } else { s.parse().ok()? };
            Some(origin + offset)
        } else {
            let mut v = s.parse().ok()?;

            // set position to block center if no decimal place is given
            if !IS_Y && !s.contains('.') {
                v += 0.5;
            }

            Some(v)
        }
    }
}
