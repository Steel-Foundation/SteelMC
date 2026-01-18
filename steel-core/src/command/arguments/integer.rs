//! An integer argument.
use steel_protocol::packets::game::{ArgumentType, SuggestionType};

use crate::command::arguments::CommandArgument;
use crate::command::context::CommandContext;

/// An integer argument that parses a 32-bit signed integer.
///
/// Can optionally have minimum and maximum bounds.
pub struct IntegerArgument {
    min: Option<i32>,
    max: Option<i32>,
}

impl IntegerArgument {
    /// Creates a new unbounded integer argument.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            min: None,
            max: None,
        }
    }

    /// Creates a new integer argument with bounds.
    #[must_use]
    pub const fn bounded(min: Option<i32>, max: Option<i32>) -> Self {
        Self { min, max }
    }
}

impl Default for IntegerArgument {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandArgument for IntegerArgument {
    type Output = i32;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = arg.first()?;
        let value: i32 = s.parse().ok()?;

        // Check bounds
        if let Some(min) = self.min
            && value < min
        {
            return None;
        }
        if let Some(max) = self.max
            && value > max
        {
            return None;
        }

        Some((&arg[1..], value))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (
            ArgumentType::Integer {
                min: self.min,
                max: self.max,
            },
            None,
        )
    }
}
