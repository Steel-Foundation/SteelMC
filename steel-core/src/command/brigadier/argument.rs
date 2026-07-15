//! Built-in Brigadier argument parsing.

use super::{
    ArgumentSuggestionContext, CommandSyntaxError, CommandSyntaxErrorKind, StringReader,
    SuggestionsBuilder,
};

/// A parser and parsed-value representation stored by one command runtime.
pub(crate) trait CommandArgumentParser<S>: PartialEq + Send + Sync + 'static {
    /// The value retained in the parsed command context.
    type Value: Clone + Send + Sync + 'static;

    /// Parses one value from the reader at its current cursor.
    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &S,
    ) -> Result<Self::Value, CommandSyntaxError>;

    /// Adds completions for a partially entered value.
    fn list_suggestions(
        &self,
        context: &ArgumentSuggestionContext<'_, S, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    );
}

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

    pub(crate) fn parse_value(
        &self,
        reader: &mut StringReader<'_>,
    ) -> Result<PrimitiveArgumentValue, CommandSyntaxError> {
        match *self {
            Self::Bool => reader.read_boolean().map(PrimitiveArgumentValue::Bool),
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
                Ok(PrimitiveArgumentValue::Integer(value))
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
                Ok(PrimitiveArgumentValue::Long(value))
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
                Ok(PrimitiveArgumentValue::Float(value))
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
                Ok(PrimitiveArgumentValue::Double(value))
            }
            Self::String(StringType::Word) => Ok(PrimitiveArgumentValue::String(
                reader.read_unquoted_string().into(),
            )),
            Self::String(StringType::QuotablePhrase) => reader
                .read_string()
                .map(String::into_boxed_str)
                .map(PrimitiveArgumentValue::String),
            Self::String(StringType::GreedyPhrase) => Ok(PrimitiveArgumentValue::String(
                reader.read_remaining().into(),
            )),
        }
    }

    pub(crate) fn suggest(&self, builder: &mut SuggestionsBuilder<'_>) {
        if *self != Self::Bool {
            return;
        }

        let remaining = builder.remaining_lowercase();
        let suggest_true = "true".starts_with(remaining);
        let suggest_false = "false".starts_with(remaining);
        if suggest_true {
            builder.suggest("true");
        }
        if suggest_false {
            builder.suggest("false");
        }
    }
}

impl<S> CommandArgumentParser<S> for ArgumentType {
    type Value = PrimitiveArgumentValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &S,
    ) -> Result<Self::Value, CommandSyntaxError> {
        self.parse_value(reader)
    }

    fn list_suggestions(
        &self,
        _context: &ArgumentSuggestionContext<'_, S, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        self.suggest(builder);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PrimitiveArgumentValue {
    Bool(bool),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(Box<str>),
}

/// Provides primitive Brigadier accessors for a runtime's parsed value.
pub(crate) trait ContainsPrimitiveArgumentValue {
    /// Returns the primitive value when this runtime value contains one.
    fn primitive_value(&self) -> Option<&PrimitiveArgumentValue>;
}

impl ContainsPrimitiveArgumentValue for PrimitiveArgumentValue {
    fn primitive_value(&self) -> Option<&PrimitiveArgumentValue> {
        Some(self)
    }
}
