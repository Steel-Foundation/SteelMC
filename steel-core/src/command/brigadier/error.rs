//! Command parsing errors.

use std::{error::Error, fmt};

const CONTEXT_AMOUNT: usize = 10;

/// Identifies a built-in Brigadier command parsing error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CommandSyntaxErrorKind {
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
}

impl fmt::Display for CommandSyntaxErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
        }
    }
}

/// A Brigadier-compatible parsing error with input context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandSyntaxError {
    kind: CommandSyntaxErrorKind,
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
            input: input.into(),
            cursor,
            byte_cursor,
        }
    }

    /// Returns the specific built-in error.
    pub(crate) const fn kind(&self) -> &CommandSyntaxErrorKind {
        &self.kind
    }

    /// Returns the command input that failed.
    pub(crate) fn input(&self) -> &str {
        &self.input
    }

    /// Returns the failure position in UTF-16 code units.
    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the error message without input context.
    pub(crate) fn raw_message(&self) -> String {
        self.kind.to_string()
    }

    /// Returns the input immediately before the error marker.
    pub(crate) fn context(&self) -> String {
        let input_before_cursor = &self.input[..self.byte_cursor];
        let mut context_start = self.byte_cursor;
        let mut context_length = 0;

        for (byte_index, character) in input_before_cursor.char_indices().rev() {
            let character_length = character.len_utf16();
            if context_length + character_length > CONTEXT_AMOUNT {
                break;
            }
            context_length += character_length;
            context_start = byte_index;
        }

        let prefix = if self.cursor > CONTEXT_AMOUNT {
            "..."
        } else {
            ""
        };
        format!(
            "{prefix}{}<--[HERE]",
            &self.input[context_start..self.byte_cursor]
        )
    }
}

impl fmt::Display for CommandSyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at position {}: {}",
            self.kind,
            self.cursor,
            self.context()
        )
    }
}

impl Error for CommandSyntaxError {}
