//! Command parsing errors.

use std::{error::Error, fmt};

use text_components::TextComponent;

const CONTEXT_AMOUNT: usize = 10;

/// Identifies a Brigadier parsing or command execution error.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CommandSyntaxErrorKind {
    /// No executable command matched the input.
    UnknownCommand,
    /// A command matched, but trailing input did not.
    UnknownArgument,
    /// A command supplied a rich runtime failure message.
    Dynamic(Box<TextComponent>),
    /// A quoted string did not start with a quote.
    ExpectedStartOfQuote,
    /// A quoted string reached the end of its input.
    ExpectedEndOfQuote,
    /// A quoted string contained an unsupported escape.
    InvalidEscape(char),
    /// A boolean did not contain `true` or `false`.
    InvalidBool(Box<str>),
    /// An integer could not be parsed.
    InvalidInt(Box<str>),
    /// No integer was present.
    ExpectedInt,
    /// A long could not be parsed.
    InvalidLong(Box<str>),
    /// No long was present.
    ExpectedLong,
    /// A double could not be parsed.
    InvalidDouble(Box<str>),
    /// No double was present.
    ExpectedDouble,
    /// A float could not be parsed.
    InvalidFloat(Box<str>),
    /// No float was present.
    ExpectedFloat,
    /// No boolean was present.
    ExpectedBool,
    /// An expected symbol was not present.
    ExpectedSymbol(char),
    /// A literal node did not match its configured text.
    LiteralIncorrect(Box<str>),
    /// An integer was below its configured minimum.
    IntegerTooLow { found: i32, minimum: i32 },
    /// An integer was above its configured maximum.
    IntegerTooHigh { found: i32, maximum: i32 },
    /// A long was below its configured minimum.
    LongTooLow { found: i64, minimum: i64 },
    /// A long was above its configured maximum.
    LongTooHigh { found: i64, maximum: i64 },
    /// A float was below its configured minimum.
    FloatTooLow { found: f32, minimum: f32 },
    /// A float was above its configured maximum.
    FloatTooHigh { found: f32, maximum: f32 },
    /// A double was below its configured minimum.
    DoubleTooLow { found: f64, minimum: f64 },
    /// A double was above its configured maximum.
    DoubleTooHigh { found: f64, maximum: f64 },
    /// A parsed argument had trailing non-whitespace data.
    ExpectedArgumentSeparator,
}

impl fmt::Display for CommandSyntaxErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand => formatter.write_str("Unknown command"),
            Self::UnknownArgument => formatter.write_str("Incorrect argument for command"),
            Self::Dynamic(message) => write!(formatter, "{message}"),
            Self::ExpectedStartOfQuote => formatter.write_str("Expected quote to start a string"),
            Self::ExpectedEndOfQuote => formatter.write_str("Unclosed quoted string"),
            Self::InvalidEscape(character) => write!(
                formatter,
                "Invalid escape sequence '{character}' in quoted string"
            ),
            Self::InvalidBool(value) => write!(
                formatter,
                "Invalid bool, expected true or false but found '{value}'"
            ),
            Self::InvalidInt(value) => write!(formatter, "Invalid integer '{value}'"),
            Self::ExpectedInt => formatter.write_str("Expected integer"),
            Self::InvalidLong(value) => write!(formatter, "Invalid long '{value}'"),
            Self::ExpectedLong => formatter.write_str("Expected long"),
            Self::InvalidDouble(value) => write!(formatter, "Invalid double '{value}'"),
            Self::ExpectedDouble => formatter.write_str("Expected double"),
            Self::InvalidFloat(value) => write!(formatter, "Invalid float '{value}'"),
            Self::ExpectedFloat => formatter.write_str("Expected float"),
            Self::ExpectedBool => formatter.write_str("Expected bool"),
            Self::ExpectedSymbol(symbol) => write!(formatter, "Expected '{symbol}'"),
            Self::LiteralIncorrect(expected) => write!(formatter, "Expected literal {expected}"),
            Self::IntegerTooLow { found, minimum } => write!(
                formatter,
                "Integer must not be less than {minimum}, found {found}"
            ),
            Self::IntegerTooHigh { found, maximum } => write!(
                formatter,
                "Integer must not be more than {maximum}, found {found}"
            ),
            Self::LongTooLow { found, minimum } => write!(
                formatter,
                "Long must not be less than {minimum}, found {found}"
            ),
            Self::LongTooHigh { found, maximum } => write!(
                formatter,
                "Long must not be more than {maximum}, found {found}"
            ),
            Self::FloatTooLow { found, minimum } => write!(
                formatter,
                "Float must not be less than {minimum}, found {found}"
            ),
            Self::FloatTooHigh { found, maximum } => write!(
                formatter,
                "Float must not be more than {maximum}, found {found}"
            ),
            Self::DoubleTooLow { found, minimum } => write!(
                formatter,
                "Double must not be less than {minimum}, found {found}"
            ),
            Self::DoubleTooHigh { found, maximum } => write!(
                formatter,
                "Double must not be more than {maximum}, found {found}"
            ),
            Self::ExpectedArgumentSeparator => formatter
                .write_str("Expected whitespace to end one argument, but found trailing data"),
        }
    }
}

/// A Brigadier-compatible parsing error with input context.
///
/// Dynamic floating-point values use Rust's standard display formatting; parsing and bounds
/// behavior remain Brigadier-compatible.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommandSyntaxError {
    kind: CommandSyntaxErrorKind,
    context: Option<CommandErrorContext>,
}

#[derive(Clone, Debug, PartialEq)]
struct CommandErrorContext {
    input: Box<str>,
    cursor: usize,
    byte_cursor: usize,
}

impl CommandSyntaxError {
    pub(super) fn new(
        kind: CommandSyntaxErrorKind,
        input: &str,
        cursor: usize,
        byte_cursor: usize,
    ) -> Self {
        Self {
            kind,
            context: Some(CommandErrorContext {
                input: input.into(),
                cursor,
                byte_cursor,
            }),
        }
    }

    /// Creates a runtime command failure without parser input context.
    pub(crate) fn dynamic(message: impl Into<TextComponent>) -> Self {
        Self {
            kind: CommandSyntaxErrorKind::Dynamic(Box::new(message.into())),
            context: None,
        }
    }

    /// Returns the specific built-in error.
    pub(crate) const fn kind(&self) -> &CommandSyntaxErrorKind {
        &self.kind
    }

    /// Returns the command input that failed.
    pub(crate) fn input(&self) -> Option<&str> {
        self.context.as_ref().map(|context| context.input.as_ref())
    }

    /// Returns the failure position in UTF-16 code units.
    pub(crate) const fn cursor(&self) -> Option<usize> {
        match &self.context {
            Some(context) => Some(context.cursor),
            None => None,
        }
    }

    /// Returns the error message without input context.
    pub(crate) fn raw_message(&self) -> String {
        self.kind.to_string()
    }

    /// Returns the input immediately before the error marker.
    pub(crate) fn context(&self) -> Option<String> {
        let context = self.context.as_ref()?;
        let input_before_cursor = &context.input[..context.byte_cursor];
        let mut context_start = context.byte_cursor;
        let mut context_length = 0;

        for (byte_index, character) in input_before_cursor.char_indices().rev() {
            let character_length = character.len_utf16();
            if context_length + character_length > CONTEXT_AMOUNT {
                break;
            }
            context_length += character_length;
            context_start = byte_index;
        }

        let prefix = if context.cursor > CONTEXT_AMOUNT {
            "..."
        } else {
            ""
        };
        Some(format!(
            "{prefix}{}<--[HERE]",
            &context.input[context_start..context.byte_cursor]
        ))
    }
}

impl fmt::Display for CommandSyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(context) = &self.context else {
            return self.kind.fmt(formatter);
        };
        let Some(display_context) = self.context() else {
            return self.kind.fmt(formatter);
        };
        write!(
            formatter,
            "{} at position {}: {display_context}",
            self.kind, context.cursor
        )
    }
}

impl Error for CommandSyntaxError {}
