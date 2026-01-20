//! A float argument.
use steel_protocol::packets::game::{ArgumentType, SuggestionType};

use crate::command::arguments::CommandArgument;
use crate::command::context::CommandContext;

/// A float argument that parses a 32-bit floating point number.
///
/// Can optionally have minimum and maximum bounds.
pub struct FloatArgument {
    min: Option<f32>,
    max: Option<f32>,
}

impl FloatArgument {
    /// Creates a new unbounded float argument.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            min: None,
            max: None,
        }
    }

    /// Creates a new float argument with bounds.
    #[must_use]
    pub const fn bounded(min: Option<f32>, max: Option<f32>) -> Self {
        Self { min, max }
    }
}

impl Default for FloatArgument {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandArgument for FloatArgument {
    type Output = f32;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let s = arg.first()?;
        let value: f32 = s.parse().ok()?;

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
            ArgumentType::Float {
                min: self.min,
                max: self.max,
            },
            None,
        )
    }
}
