//! Built-in Brigadier argument parsing.

use super::{CommandSyntaxError, CommandSyntaxErrorKind, StringReader};

/// The parsing mode for a string argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StringType {
    Word,
    QuotablePhrase,
    GreedyPhrase,
}

/// A built-in Brigadier argument parser configuration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ArgumentType {
    /// A lowercase boolean.
    Bool,
    /// A bounded signed 32-bit integer.
    Integer { minimum: i32, maximum: i32 },
    /// A bounded signed 64-bit integer.
    Long { minimum: i64, maximum: i64 },
    /// A bounded 32-bit floating-point number.
    Float { minimum: f32, maximum: f32 },
    /// A bounded 64-bit floating-point number.
    Double { minimum: f64, maximum: f64 },
    /// A word, quotable phrase, or greedy phrase.
    String(StringType),
}

impl ArgumentType {
    /// Creates a boolean argument parser.
    pub(crate) const fn bool() -> Self {
        Self::Bool
    }

    /// Creates a bounded integer argument parser.
    pub(crate) const fn integer(minimum: i32, maximum: i32) -> Self {
        Self::Integer { minimum, maximum }
    }

    /// Creates a bounded long argument parser.
    pub(crate) const fn long(minimum: i64, maximum: i64) -> Self {
        Self::Long { minimum, maximum }
    }

    /// Creates a bounded float argument parser.
    pub(crate) const fn float(minimum: f32, maximum: f32) -> Self {
        Self::Float { minimum, maximum }
    }

    /// Creates a bounded double argument parser.
    pub(crate) const fn double(minimum: f64, maximum: f64) -> Self {
        Self::Double { minimum, maximum }
    }

    /// Creates a single-word string argument parser.
    pub(crate) const fn word() -> Self {
        Self::String(StringType::Word)
    }

    /// Creates a quoted or unquoted phrase argument parser.
    pub(crate) const fn string() -> Self {
        Self::String(StringType::QuotablePhrase)
    }

    /// Creates an argument parser that consumes the remaining input.
    pub(crate) const fn greedy_string() -> Self {
        Self::String(StringType::GreedyPhrase)
    }

    pub(super) fn parse(
        &self,
        reader: &mut StringReader<'_>,
    ) -> Result<ParsedValue, CommandSyntaxError> {
        match *self {
            Self::Bool => reader.read_boolean().map(ParsedValue::Bool),
            Self::Integer { minimum, maximum } => {
                let start = reader.checkpoint();
                let value = reader.read_int()?;
                if value < minimum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::IntegerTooLow {
                        found: value,
                        minimum,
                    }));
                }
                if value > maximum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::IntegerTooHigh {
                        found: value,
                        maximum,
                    }));
                }
                Ok(ParsedValue::Integer(value))
            }
            Self::Long { minimum, maximum } => {
                let start = reader.checkpoint();
                let value = reader.read_long()?;
                if value < minimum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::LongTooLow {
                        found: value,
                        minimum,
                    }));
                }
                if value > maximum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::LongTooHigh {
                        found: value,
                        maximum,
                    }));
                }
                Ok(ParsedValue::Long(value))
            }
            Self::Float { minimum, maximum } => {
                let start = reader.checkpoint();
                let value = reader.read_float()?;
                if value < minimum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::FloatTooLow {
                        found: value,
                        minimum,
                    }));
                }
                if value > maximum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::FloatTooHigh {
                        found: value,
                        maximum,
                    }));
                }
                Ok(ParsedValue::Float(value))
            }
            Self::Double { minimum, maximum } => {
                let start = reader.checkpoint();
                let value = reader.read_double()?;
                if value < minimum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::DoubleTooLow {
                        found: value,
                        minimum,
                    }));
                }
                if value > maximum {
                    reader.restore(start);
                    return Err(reader.error(CommandSyntaxErrorKind::DoubleTooHigh {
                        found: value,
                        maximum,
                    }));
                }
                Ok(ParsedValue::Double(value))
            }
            Self::String(StringType::Word) => {
                Ok(ParsedValue::String(reader.read_unquoted_string().into()))
            }
            Self::String(StringType::QuotablePhrase) => reader
                .read_string()
                .map(String::into_boxed_str)
                .map(ParsedValue::String),
            Self::String(StringType::GreedyPhrase) => {
                Ok(ParsedValue::String(reader.read_remaining().into()))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ParsedValue {
    Bool(bool),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(Box<str>),
}
